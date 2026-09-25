//! Helper process for ao-process tests. Portable (no shell needed).
//!
//! Modes:
//!   print <n> <exit_code>   print n stdout lines + 1 stderr line, then exit
//!   echo                    echo stdin lines until EOF
//!   tree <pidfile>          start a grandchild, write "<pid> <grandchild>" to pidfile, sleep
//!   orphan <pidfile>        start a grandchild, write "<pid> <grandchild>" to pidfile, exit 0
//!   args [..]               print each argument on its own line, then exit
//!   sleep                   sleep for 10 minutes

use std::io::{BufRead, Write};
use std::time::Duration;

// The `tree` mode intentionally leaves its grandchild running: the test kills it.
#[allow(clippy::zombie_processes)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("print") => {
            let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
            let code: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let mut out = std::io::stdout().lock();
            for i in 0..n {
                writeln!(out, "line {i}").unwrap();
            }
            out.flush().unwrap();
            eprintln!("err 0");
            std::process::exit(code);
        }
        Some("echo") => {
            let stdin = std::io::stdin();
            let mut out = std::io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line.unwrap();
                writeln!(out, "echo: {line}").unwrap();
                out.flush().unwrap();
            }
        }
        Some("args") => {
            let mut out = std::io::stdout().lock();
            for arg in &args[1..] {
                writeln!(out, "arg: {arg}").unwrap();
            }
        }
        Some(mode @ ("tree" | "orphan")) => {
            let pidfile = args.get(1).expect("pidfile");
            let grandchild = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("sleep")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn grandchild");
            std::fs::write(
                pidfile,
                format!("{} {}", std::process::id(), grandchild.id()),
            )
            .unwrap();
            println!("ready");
            if mode == "orphan" {
                // Leave the grandchild running: the process manager must stop it.
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_secs(600));
        }
        _ => std::thread::sleep(Duration::from_secs(600)),
    }
}
