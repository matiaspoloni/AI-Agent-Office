//! "Open project" and "Show changed file": the system's file manager, at a
//! folder or with a file selected.
//!
//! A file is never opened with its default program: on Windows that would
//! *run* scripts and programs (`.js` files open in Windows Script Host,
//! `.bat`, `.exe`, …), and an agent can create any file. Folders are opened
//! only after checking they are folders (Explorer would run a program given
//! instead). Paths are single arguments, never shell text.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: &'static str,
    pub args: Vec<OsString>,
    /// Windows: an argument passed exactly as written (Explorer parses
    /// `/select,"path"` itself and does not follow the usual quoting rules).
    pub raw_arg: Option<String>,
}

/// Opens `dir` (an existing folder) in the file manager of `os`.
pub fn open_folder_command(os: &str, dir: &Path) -> Launch {
    let dir = dir.as_os_str().to_owned();
    match os {
        "windows" => Launch {
            program: "explorer.exe",
            args: vec![dir],
            raw_arg: None,
        },
        // `open` would launch an `.app` folder; `-R` only shows it.
        "macos" => Launch {
            program: "open",
            args: vec!["-R".into(), dir],
            raw_arg: None,
        },
        _ => Launch {
            program: "xdg-open",
            args: vec![dir],
            raw_arg: None,
        },
    }
}

/// Shows `file` selected in its folder.
pub fn reveal_command(os: &str, file: &Path) -> Launch {
    match os {
        "windows" => Launch {
            program: "explorer.exe",
            args: vec![],
            // Windows paths cannot contain `"`, so the quotes cannot be escaped.
            raw_arg: Some(format!("/select,\"{}\"", file.display())),
        },
        "macos" => Launch {
            program: "open",
            args: vec!["-R".into(), file.as_os_str().to_owned()],
            raw_arg: None,
        },
        // No portable "select": open the folder that contains it.
        _ => open_folder_command(os, file.parent().unwrap_or(file)),
    }
}

fn spawn(launch: Launch) -> Result<(), String> {
    let mut command = Command::new(launch.program);
    command
        .args(&launch.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    if let Some(raw) = &launch.raw_arg {
        use std::os::windows::process::CommandExt;
        command.raw_arg(raw);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", launch.program))?;
    // Not managed: only reaped when it exits.
    let _ = std::thread::Builder::new()
        .name("reveal-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

/// Opens an existing folder in the file manager.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("The folder {} does not exist.", dir.display()));
    }
    spawn(open_folder_command(std::env::consts::OS, dir))
}

/// Shows `path` (relative to `folder`, or absolute inside it) in the file
/// manager. A deleted file shows its folder instead.
pub fn reveal(folder: &Path, path: &str) -> Result<(), String> {
    let target = inside(folder, path)
        .ok_or_else(|| format!("{path} is not inside {}.", folder.display()))?;
    if target.exists() {
        return spawn(reveal_command(std::env::consts::OS, &target));
    }
    match target.parent() {
        Some(parent) if parent.is_dir() => open_folder(parent),
        _ => Err(format!("{path} no longer exists.")),
    }
}

/// `path` resolved against `folder`, if it stays inside it (no `..` escape).
pub fn inside(folder: &Path, path: &str) -> Option<PathBuf> {
    let candidate = Path::new(path);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        folder.join(candidate)
    };
    let mut clean = PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other),
        }
    }
    let (a, b) = (
        clean.to_string_lossy().replace('\\', "/"),
        folder.to_string_lossy().replace('\\', "/"),
    );
    let b = b.trim_end_matches('/');
    let within = if cfg!(windows) {
        a.len() > b.len() && a[..b.len()].eq_ignore_ascii_case(b) && a.as_bytes()[b.len()] == b'/'
    } else {
        a.len() > b.len() && a.starts_with(b) && a.as_bytes()[b.len()] == b'/'
    };
    within.then_some(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_only_selected_never_opened() {
        let win = reveal_command("windows", Path::new(r"C:\work\app\run me.js"));
        assert_eq!(win.program, "explorer.exe");
        assert!(win.args.is_empty());
        assert_eq!(
            win.raw_arg.as_deref(),
            Some(r#"/select,"C:\work\app\run me.js""#)
        );
        let mac = reveal_command("macos", Path::new("/w/app/x.sh"));
        assert_eq!(mac.args[0], "-R");
        let linux = reveal_command("linux", Path::new("/w/app/x.sh"));
        assert_eq!(linux.program, "xdg-open");
        assert_eq!(linux.args, vec![OsString::from("/w/app")]);
        assert_eq!(
            open_folder_command("macos", Path::new("/w/My.app")).args[0],
            "-R"
        );
    }

    #[test]
    fn paths_must_stay_inside_the_folder() {
        let root = Path::new("/work/app");
        assert_eq!(
            inside(root, "src/main.rs"),
            Some(PathBuf::from("/work/app/src/main.rs"))
        );
        assert_eq!(
            inside(root, "/work/app/a/../b.txt"),
            Some(PathBuf::from("/work/app/b.txt"))
        );
        assert_eq!(inside(root, "../other/secret.txt"), None);
        assert_eq!(inside(root, "/work/application/x"), None);
        assert_eq!(inside(root, "/etc/passwd"), None);
        assert_eq!(inside(root, ""), None);
        assert_eq!(inside(root, "."), None);
    }

    #[test]
    fn missing_things_are_errors_not_launches() {
        let tmp = std::env::temp_dir().join(format!("ao-reveal-{}", std::process::id()));
        assert!(open_folder(&tmp.join("missing")).is_err());
        assert!(reveal(&tmp.join("missing"), "a/b.txt").is_err());
        assert!(reveal(Path::new("/work/app"), "../x").is_err());
        // A file given as a folder is refused (Explorer would run it).
        std::fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("tool.exe");
        std::fs::write(&file, "x").unwrap();
        assert!(open_folder(&file).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
