//! The environment of the user's login shell.
//!
//! On macOS an app opened from the Dock or Finder is started by `launchd` with
//! a minimal environment (`PATH=/usr/bin:/bin:/usr/sbin:/sbin`, none of the
//! variables set in `~/.zshrc`/`~/.zprofile`). Agent CLIs installed with
//! Homebrew, npm or nvm, and settings such as `CLAUDE_CONFIG_DIR`, would then
//! be invisible to Agent Office although the same user's terminal sees them.
//! As other editors do, the login shell is asked once, at startup, for its
//! environment: `$SHELL -l -i -c 'echo BEGIN; /usr/bin/env; echo END'`.
//!
//! Nothing is stored or logged: the values only go into this process's
//! environment (which the agents Agent Office starts inherit).

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const BEGIN: &str = "__AGENT_OFFICE_ENV_BEGIN__";
pub const END: &str = "__AGENT_OFFICE_ENV_END__";

/// Variables that describe one terminal session, never copied.
const SESSION_ONLY: &[&str] = &[
    "_",
    "COLORTERM",
    "COLUMNS",
    "ITERM_PROFILE",
    "ITERM_SESSION_ID",
    "LINES",
    "OLDPWD",
    "PS1",
    "PS2",
    "PS3",
    "PS4",
    "PWD",
    "SHLVL",
    "SSH_TTY",
    "STY",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "TMUX",
    "TMUX_PANE",
    "WINDOW",
];

/// The user's login shell: `$SHELL`, else the account's shell, else zsh
/// (the macOS default).
pub fn login_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
        .or_else(account_shell)
        .unwrap_or_else(|| PathBuf::from("/bin/zsh"))
}

fn account_shell() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: `passwd` is plain data that getpwuid_r fills in; `buf` outlives
    // every pointer into it, and `pw_shell` is read only when the call
    // succeeded and returned our struct.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 4096];
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(
            libc::getuid(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() || pwd.pw_shell.is_null() {
            return None;
        }
        let shell = std::ffi::CStr::from_ptr(pwd.pw_shell).to_bytes();
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(shell));
        (path.is_absolute() && path.is_file()).then_some(path)
    }
}

/// Runs the login shell and returns its environment, or `None` if it fails
/// or does not print it within `timeout`. A shell that times out is stopped
/// with everything it started (they are in their own process group, created
/// here); one that printed the environment is left to finish on its own.
pub fn read(shell: &Path, timeout: Duration) -> Option<Vec<(String, String)>> {
    use std::os::unix::process::CommandExt;
    let script = format!("echo {BEGIN}; /usr/bin/env; echo {END}");
    let spawn = || {
        Command::new(shell)
            .args(["-l", "-i", "-c", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
    };
    // "Text file busy": the shell binary is being written (e.g. updated)
    // right now; it clears within moments.
    let mut child = None;
    for _ in 0..5 {
        match spawn() {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            other => {
                child = other.ok();
                break;
            }
        }
    }
    let mut child = child?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Up to the end marker: a process the shell left running may keep
        // the pipe open long after.
        let mut out = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&chunk[..n]),
            }
            if String::from_utf8_lossy(&out)
                .lines()
                .any(|l| l.trim() == END)
            {
                break;
            }
        }
        let _ = tx.send(out);
    });
    match rx.recv_timeout(timeout) {
        Ok(output) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            parse(&String::from_utf8_lossy(&output))
        }
        Err(_) => {
            // SAFETY: plain syscall on the process group created above.
            unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
            let _ = child.wait();
            None
        }
    }
}

/// The `KEY=value` lines between the markers (any shell greeting before or
/// after them is ignored).
pub fn parse(output: &str) -> Option<Vec<(String, String)>> {
    let mut lines = output.lines();
    lines.by_ref().find(|l| l.trim() == BEGIN)?;
    let mut vars = Vec::new();
    for line in lines {
        if line.trim() == END {
            return Some(vars);
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let valid = key
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if valid {
            vars.push((key.to_owned(), value.to_owned()));
        }
    }
    None
}

/// What to change in the current environment: `PATH` becomes the shell's
/// entries followed by the current ones it lacks; every other variable is
/// added only when missing (what the app was given always wins).
pub fn changes(
    shell_env: &[(String, String)],
    current: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, value) in shell_env {
        if SESSION_ONLY.contains(&key.as_str()) {
            continue;
        }
        if key == "PATH" {
            let merged = merge_path(value, &current("PATH").unwrap_or_default());
            if current("PATH").as_deref() != Some(merged.as_str()) {
                out.push((key.clone(), merged));
            }
        } else if current(key).is_none() {
            out.push((key.clone(), value.clone()));
        }
    }
    out
}

/// `preferred` entries first, then the `current` ones not already present;
/// only absolute folders, no duplicates.
pub fn merge_path(preferred: &str, current: &str) -> String {
    let mut seen = HashSet::new();
    preferred
        .split(':')
        .chain(current.split(':'))
        .filter(|entry| entry.starts_with('/') && seen.insert(*entry))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ao-login-env-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for zsh/bash: prints a greeting, then runs the `-c` script.
    fn fake_shell(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn reads_the_environment_between_the_markers() {
        let dir = temp_dir("read");
        // $4 is the script after `-l -i -c`.
        let shell = fake_shell(
            &dir,
            r#"echo "Welcome back"; export AO_TEST_ONLY_IN_SHELL=yes PATH="/opt/tools/bin:$PATH"; eval "$4"; echo bye"#,
        );
        let env = read(&shell, Duration::from_secs(10)).expect("environment");
        assert!(env.contains(&("AO_TEST_ONLY_IN_SHELL".into(), "yes".into())));
        let path = &env.iter().find(|(k, _)| k == "PATH").unwrap().1;
        assert!(path.starts_with("/opt/tools/bin:"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Whether `pid` still runs (a zombie nobody reaped counts as gone).
    fn running(pid: i32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } != 0 {
            return false;
        }
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat
                .rsplit_once(')')
                .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z')),
            Err(_) => true,
        }
    }

    #[test]
    fn a_shell_that_hangs_is_stopped_with_what_it_started() {
        let dir = temp_dir("hang");
        let pid_file = dir.join("child.pid");
        // Starts a child in its group and never prints the end marker.
        let shell = fake_shell(
            &dir,
            &format!(
                "echo {BEGIN}; sh -c 'echo $$ > \"{}\"; sleep 30' & sleep 30",
                pid_file.display()
            ),
        );
        let started = std::time::Instant::now();
        assert_eq!(read(&shell, Duration::from_millis(500)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while running(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!running(pid), "the shell's child was stopped too");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_process_left_running_does_not_delay_the_answer() {
        let dir = temp_dir("daemon");
        // A background process keeps the output pipe open after the shell ends.
        let shell = fake_shell(&dir, r#"(sleep 3) & eval "$4""#);
        let started = std::time::Instant::now();
        assert!(read(&shell, Duration::from_secs(10)).is_some());
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failing_shell_gives_nothing() {
        let dir = temp_dir("fail");
        let shell = fake_shell(&dir, "exit 1");
        assert_eq!(read(&shell, Duration::from_secs(5)), None);
        assert_eq!(read(&dir.join("missing"), Duration::from_secs(5)), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parse_ignores_noise_and_odd_lines() {
        let out = format!(
            "Last login: today\n{BEGIN}\nPATH=/a:/b\nnot a variable\nBASH_FUNC_x%%=() {{ echo; }}\nEMPTY=\nURL=a=b\n{END}\nlogout\n"
        );
        assert_eq!(
            parse(&out).unwrap(),
            vec![
                ("PATH".into(), "/a:/b".into()),
                ("EMPTY".into(), String::new()),
                ("URL".into(), "a=b".into()),
            ]
        );
        assert_eq!(parse("no markers"), None);
        assert_eq!(parse(&format!("{BEGIN}\nA=1\n")), None, "cut short");
    }

    #[test]
    fn the_app_environment_wins_except_for_path() {
        let shell = vec![
            ("PATH".to_string(), "/opt/homebrew/bin:/usr/bin".to_string()),
            ("CLAUDE_CONFIG_DIR".into(), "/Users/me/.claude-work".into()),
            ("LANG".into(), "es_AR.UTF-8".into()),
            ("PWD".into(), "/Users/me".into()),
            ("TMPDIR".into(), "/somewhere/else".into()),
        ];
        let current = |key: &str| match key {
            "PATH" => Some("/usr/bin:/bin:/usr/sbin:/sbin".to_string()),
            "TMPDIR" => Some("/var/folders/x/T/".to_string()),
            _ => None,
        };
        assert_eq!(
            changes(&shell, current),
            vec![
                (
                    "PATH".to_string(),
                    "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string()
                ),
                ("CLAUDE_CONFIG_DIR".into(), "/Users/me/.claude-work".into()),
                ("LANG".into(), "es_AR.UTF-8".into()),
            ]
        );
    }

    #[test]
    fn merge_path_keeps_order_and_drops_duplicates_and_relative_entries() {
        assert_eq!(merge_path("/a:/b:.:/a", "/c:/b:"), "/a:/b:/c");
        assert_eq!(merge_path("", "/usr/bin"), "/usr/bin");
    }

    #[test]
    fn the_login_shell_is_an_absolute_file() {
        let shell = login_shell();
        assert!(shell.is_absolute());
    }
}
