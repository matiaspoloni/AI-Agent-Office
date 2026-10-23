//! Platform details that only matter on macOS (no-ops elsewhere).

use std::path::Path;

const TRANSLOCATED: &str = "macOS is running Agent Office from a temporary copy (the app was \
     opened where it was downloaded). Hooks pointing there would stop working. Move Agent Office \
     to the Applications folder, open it from there and install the integration again.";
const DISK_IMAGE: &str = "Agent Office is running from its disk image, which disappears when \
     it is ejected. Drag Agent Office to the Applications folder, open it from there and install \
     the integration again.";

/// An app opened from the Dock or Finder is started by `launchd` with a
/// minimal environment; adopt the login shell's (see
/// `ao_detect::login_env`). Returns how many variables were set, `None` when
/// nothing was done. Must run before any other thread starts.
#[cfg(target_os = "macos")]
pub fn adopt_login_shell_env() -> Option<usize> {
    use ao_detect::login_env;
    // SAFETY: getppid has no preconditions.
    if unsafe { libc::getppid() } != 1 {
        // Started from a terminal: it already has the user's environment.
        return None;
    }
    let shell = login_env::login_shell();
    let env = login_env::read(&shell, std::time::Duration::from_secs(5))?;
    let changes = login_env::changes(&env, |key| {
        std::env::var_os(key).map(|v| v.to_string_lossy().into_owned())
    });
    for (key, value) in &changes {
        std::env::set_var(key, value);
    }
    Some(changes.len())
}

#[cfg(not(target_os = "macos"))]
pub fn adopt_login_shell_env() -> Option<usize> {
    None
}

/// Why hook entries pointing at `program` would break later, if they would.
/// macOS runs a downloaded app that was not moved from a temporary read-only
/// copy ("App Translocation"), and a disk image disappears when ejected.
pub fn relay_location_problem(program: &Path) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    location_problem(program, is_read_only_volume)
}

fn location_problem(program: &Path, read_only: impl Fn(&Path) -> bool) -> Option<String> {
    if program.to_string_lossy().contains("/AppTranslocation/") {
        return Some(TRANSLOCATED.into());
    }
    if program.starts_with("/Volumes") && read_only(program) {
        return Some(DISK_IMAGE.into());
    }
    None
}

#[cfg(target_os = "macos")]
fn is_read_only_volume(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `stat` is plain data filled in by statfs; `c_path` is a valid
    // NUL-terminated string for the duration of the call.
    unsafe {
        let mut stat: libc::statfs = std::mem::zeroed();
        libc::statfs(c_path.as_ptr(), &mut stat) == 0 && stat.f_flags & libc::MNT_RDONLY as u32 != 0
    }
}

#[cfg(not(target_os = "macos"))]
fn is_read_only_volume(_path: &Path) -> bool {
    false
}

/// Why the `git` at `path` must not be run, if so. On macOS `/usr/bin/git`
/// is a stub that, without Apple's Command Line Tools, opens an installation
/// dialog every time it runs.
pub fn git_problem(path: &Path) -> Option<String> {
    if !cfg!(target_os = "macos") || path != Path::new("/usr/bin/git") {
        return None;
    }
    developer_tools_missing().then(|| {
        "Git needs Apple's Command Line Tools: run `xcode-select --install` in Terminal, \
         then restart Agent Office."
            .to_string()
    })
}

#[cfg(target_os = "macos")]
fn developer_tools_missing() -> bool {
    use std::process::{Command, Stdio};
    // `xcode-select -p` only reads the setting; it never opens the dialog.
    match Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => {
            let dir = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
            !dir.join("usr").join("bin").join("git").is_file()
        }
        _ => true,
    }
}

#[cfg(not(target_os = "macos"))]
fn developer_tools_missing() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_locations_are_recognised() {
        let writable = |_: &Path| false;
        let read_only = |_: &Path| true;
        let translocated = Path::new(
            "/private/var/folders/x1/abc/T/AppTranslocation/0A1B/d/Agent Office.app/Contents/MacOS/agent-office",
        );
        assert_eq!(
            location_problem(translocated, writable).as_deref(),
            Some(TRANSLOCATED)
        );
        let mounted =
            Path::new("/Volumes/Agent Office/Agent Office.app/Contents/MacOS/agent-office");
        assert_eq!(
            location_problem(mounted, read_only).as_deref(),
            Some(DISK_IMAGE)
        );
        // An external drive the user keeps apps on is fine.
        assert_eq!(location_problem(mounted, writable), None);
        let installed = Path::new("/Applications/Agent Office.app/Contents/MacOS/agent-office");
        assert_eq!(location_problem(installed, read_only), None);
    }

    #[test]
    fn nothing_changes_outside_macos() {
        if cfg!(target_os = "macos") {
            return;
        }
        assert_eq!(adopt_login_shell_env(), None);
        assert_eq!(
            relay_location_problem(Path::new("/Volumes/X/AppTranslocation/a")),
            None
        );
        assert_eq!(git_problem(Path::new("/usr/bin/git")), None);
    }
}
