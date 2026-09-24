//! Finds provider executables and reads their versions without a shell.
//!
//! * Searches `PATH` (honouring `PATHEXT` on Windows) and then well-known
//!   install locations passed by each adapter.
//! * Runs `<exe> --version` with a timeout and no console window.
//! * `.cmd` / `.bat` shims (npm global installs on Windows) are supported;
//!   the standard library runs them through `cmd.exe` with safe escaping.

use ao_core::provider::InstallationInfo;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(8);

static VERSION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.\-]+)?").expect("valid version regex")
});

/// Extracts the first semantic-looking version (`2.1.281`, `0.156.1-alpha.3`).
pub fn parse_version(output: &str) -> Option<String> {
    VERSION.find(output).map(|m| m.as_str().to_owned())
}

/// Expands `%VAR%` (Windows style) and `$VAR` / `~` prefixes in candidate paths.
pub fn expand_path(template: &str) -> Option<PathBuf> {
    let mut out = String::new();
    let mut rest = template;
    if let Some(stripped) = rest.strip_prefix('~') {
        out.push_str(&home_dir()?.to_string_lossy());
        rest = stripped;
    }
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let name: String = chars.by_ref().take_while(|&c| c != '%').collect();
            out.push_str(&std::env::var(&name).ok()?);
        } else if c == '$' {
            let mut name = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_alphanumeric() || n == '_' {
                    name.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push_str(&std::env::var(&name).ok()?);
        } else {
            out.push(c);
        }
    }
    Some(PathBuf::from(out))
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        [".exe", ".cmd", ".bat", ""]
            .iter()
            .map(|ext| dir.join(format!("{name}{ext}")))
            .collect()
    } else {
        vec![dir.join(name)]
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Locates the first executable among `names`, first on `PATH`, then in
/// `extra_dirs` (templates like `%USERPROFILE%\.local\bin`).
pub fn find_executable(names: &[String], extra_dirs: &[&str]) -> Option<PathBuf> {
    for name in names {
        if let Ok(path) = which::which(name) {
            return Some(path);
        }
    }
    for template in extra_dirs {
        let Some(dir) = expand_path(template) else {
            continue;
        };
        for name in names {
            if let Some(found) = executable_candidates(&dir, name)
                .into_iter()
                .find(|p| is_executable_file(p))
            {
                return Some(found);
            }
        }
    }
    None
}

/// Builds a command for `exe args…` without opening a console window.
///
/// Windows `.cmd`/`.bat` shims (npm global installs) work too: the Rust
/// standard library runs them through `cmd.exe` with safe argument escaping.
pub fn command_for(exe: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.kill_on_drop(true);
    cmd
}

/// Runs `exe args…` and returns trimmed stdout (or stderr if stdout is empty).
pub async fn run_capture(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut cmd = command_for(exe, args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", exe.display()))?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => Err(format!(
            "{} did not answer within {}s",
            exe.display(),
            timeout.as_secs()
        )),
        Ok(Err(e)) => Err(format!("failed to run {}: {e}", exe.display())),
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if !output.status.success() && stdout.is_empty() {
                return Err(format!(
                    "{} exited with {}: {}",
                    exe.display(),
                    output.status,
                    stderr.lines().next().unwrap_or_default()
                ));
            }
            Ok(if stdout.is_empty() { stderr } else { stdout })
        }
    }
}

/// Full detection: locate the executable and read its version.
pub async fn detect(
    names: &[String],
    extra_dirs: &[&str],
    version_args: &[&str],
) -> InstallationInfo {
    let Some(path) = find_executable(names, extra_dirs) else {
        return InstallationInfo {
            installed: false,
            error: Some(format!(
                "`{}` was not found on PATH or in known install folders",
                names.join("` / `")
            )),
            ..Default::default()
        };
    };
    match run_capture(&path, version_args, DEFAULT_TIMEOUT).await {
        Ok(output) => InstallationInfo {
            installed: true,
            executable_path: Some(path.display().to_string()),
            version: parse_version(&output),
            version_output: output.lines().next().map(str::to_owned),
            error: None,
        },
        Err(error) => InstallationInfo {
            installed: true,
            executable_path: Some(path.display().to_string()),
            version: None,
            version_output: None,
            error: Some(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions_from_real_outputs() {
        assert_eq!(
            parse_version("2.1.281 (Claude Code)").as_deref(),
            Some("2.1.281")
        );
        assert_eq!(
            parse_version("codex-cli 0.156.1").as_deref(),
            Some("0.156.1")
        );
        assert_eq!(
            parse_version("0.158.0-alpha.8").as_deref(),
            Some("0.158.0-alpha.8")
        );
        assert_eq!(
            parse_version("git version 2.43.0.windows.1").as_deref(),
            Some("2.43.0")
        );
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn expands_environment_templates() {
        std::env::set_var("AO_DETECT_TEST_DIR", "/opt/tools");
        assert_eq!(
            expand_path("%AO_DETECT_TEST_DIR%/bin"),
            Some(PathBuf::from("/opt/tools/bin"))
        );
        assert_eq!(
            expand_path("$AO_DETECT_TEST_DIR/bin"),
            Some(PathBuf::from("/opt/tools/bin"))
        );
        assert_eq!(expand_path("%AO_DETECT_MISSING_VAR%/bin"), None);
    }

    #[tokio::test]
    async fn reports_missing_executables() {
        let info = detect(
            &["definitely-not-an-agent-cli-xyz".into()],
            &[],
            &["--version"],
        )
        .await;
        assert!(!info.installed);
        assert!(info.error.unwrap().contains("not found"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detects_executables_in_extra_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ao-detect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("fake-agent");
        std::fs::write(&exe, "#!/bin/sh\necho 'fake-agent 1.2.3'\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();

        let dir_str = dir.display().to_string();
        let info = detect(&["fake-agent".into()], &[dir_str.as_str()], &["--version"]).await;
        assert!(info.installed, "{info:?}");
        assert_eq!(info.version.as_deref(), Some("1.2.3"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
