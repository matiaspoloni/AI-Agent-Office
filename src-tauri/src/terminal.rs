//! "Open terminal": opens the user's own terminal in a session's folder.
//!
//! The terminal is started detached and is never managed: Agent Office does
//! not read it, type into it, run commands in it or stop it. Paths are passed
//! as single arguments (never through a shell), so nothing in a folder name
//! can become a command.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One way to open a terminal on this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCommand {
    pub program: &'static str,
    pub args: Vec<OsString>,
    /// Windows: give the program its own console window.
    pub new_console: bool,
}

impl TerminalCommand {
    fn new(program: &'static str, args: Vec<OsString>) -> Self {
        Self {
            program,
            args,
            new_console: false,
        }
    }
}

/// The terminals to try, in order, on `os` (`std::env::consts::OS`). Every
/// command is also started with `dir` as its working directory.
pub fn candidates(os: &str, dir: &Path) -> Vec<TerminalCommand> {
    let dir = dir.as_os_str().to_owned();
    let with = |flag: &str| {
        let mut arg = OsString::from(flag);
        arg.push(&dir);
        arg
    };
    match os {
        "windows" => vec![
            // Windows Terminal splits its command line at `;`, which is legal
            // in folder names: escape it.
            TerminalCommand::new(
                "wt.exe",
                vec!["-d".into(), escape_wt(&dir.to_string_lossy()).into()],
            ),
            TerminalCommand {
                program: "powershell.exe",
                args: vec!["-NoLogo".into(), "-NoExit".into()],
                new_console: true,
            },
            TerminalCommand {
                program: "cmd.exe",
                args: vec![],
                new_console: true,
            },
        ],
        "macos" => vec![TerminalCommand::new(
            "open",
            vec!["-a".into(), "Terminal".into(), dir.clone()],
        )],
        _ => vec![
            TerminalCommand::new("x-terminal-emulator", vec![]),
            TerminalCommand::new("gnome-terminal", vec![with("--working-directory=")]),
            TerminalCommand::new("konsole", vec!["--workdir".into(), dir.clone()]),
            TerminalCommand::new("xfce4-terminal", vec![with("--working-directory=")]),
            TerminalCommand::new("xterm", vec![]),
        ],
    }
}

fn escape_wt(path: &str) -> String {
    path.replace(';', "\\;")
}

/// Checks that `folder` is an existing directory and returns its full path.
pub fn resolve_folder(folder: &str) -> Result<PathBuf, String> {
    let path = Path::new(folder.trim());
    if folder.trim().is_empty() {
        return Err("This session has no folder.".into());
    }
    if !path.is_absolute() {
        return Err(format!("The session folder is not a full path: {folder}"));
    }
    if !path.is_dir() {
        return Err(format!(
            "The folder {folder} does not exist on this computer."
        ));
    }
    Ok(path.to_path_buf())
}

/// Opens a terminal in `dir`, trying each candidate until one starts.
/// Returns the program that was started.
pub fn open_in(dir: &Path) -> Result<&'static str, String> {
    let mut tried = Vec::new();
    for candidate in candidates(std::env::consts::OS, dir) {
        let mut command = Command::new(candidate.program);
        command
            .args(&candidate.args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        if candidate.new_console {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
            command.creation_flags(CREATE_NEW_CONSOLE);
        }
        match command.spawn() {
            Ok(mut child) => {
                // Not managed: only reaped when it exits, never stopped.
                let _ = std::thread::Builder::new()
                    .name("terminal-reaper".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
                tracing::info!(program = candidate.program, "opened a terminal");
                return Ok(candidate.program);
            }
            Err(err) => tried.push(format!("{} ({err})", candidate.program)),
        }
    }
    Err(format!(
        "No terminal could be opened. Tried: {}",
        tried.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_prefers_windows_terminal_and_escapes_semicolons() {
        let list = candidates("windows", Path::new(r"C:\work\a;b c"));
        assert_eq!(list[0].program, "wt.exe");
        assert_eq!(
            list[0].args,
            vec![OsString::from("-d"), OsString::from(r"C:\work\a\;b c")]
        );
        assert!(!list[0].new_console);
        // Fallbacks open in their own console, in the working directory.
        assert_eq!(list[1].program, "powershell.exe");
        assert!(list[1].new_console);
        assert!(list[1].args.iter().all(|a| a != r"C:\work\a;b c"));
        assert_eq!(list[2].program, "cmd.exe");
        assert!(list[2].args.is_empty());
    }

    #[test]
    fn no_candidate_runs_a_command_in_the_terminal() {
        for os in ["windows", "macos", "linux"] {
            for candidate in candidates(os, Path::new("/tmp/x")) {
                for arg in &candidate.args {
                    let arg = arg.to_string_lossy();
                    for flag in ["-c", "-e", "/c", "/k", "-Command", "--command", "-x"] {
                        assert_ne!(arg, flag, "{os}: {} runs a command", candidate.program);
                    }
                }
            }
        }
    }

    #[test]
    fn other_systems_pass_the_folder_as_one_argument() {
        let dir = Path::new("/home/me/my project; rm -rf x");
        let mac = candidates("macos", dir);
        assert_eq!(mac[0].args.last().unwrap(), dir.as_os_str());
        let linux = candidates("linux", dir);
        let gnome = linux
            .iter()
            .find(|c| c.program == "gnome-terminal")
            .unwrap();
        assert_eq!(
            gnome.args,
            vec![OsString::from(
                "--working-directory=/home/me/my project; rm -rf x"
            )]
        );
    }

    #[test]
    fn folders_must_exist_and_be_full_paths() {
        assert!(resolve_folder("").is_err());
        assert!(resolve_folder("relative/path").is_err());
        let missing = std::env::temp_dir().join("agent-office-no-such-folder-7d");
        assert!(resolve_folder(&missing.to_string_lossy())
            .unwrap_err()
            .contains("does not exist"));
        let here = std::env::temp_dir();
        assert_eq!(resolve_folder(&here.to_string_lossy()).unwrap(), here);
        // A file is not a folder.
        let file = here.join(format!("agent-office-terminal-{}", std::process::id()));
        std::fs::write(&file, "x").unwrap();
        assert!(resolve_folder(&file.to_string_lossy()).is_err());
        let _ = std::fs::remove_file(file);
    }
}
