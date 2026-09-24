//! Builds and locates workspace binaries from inside tests (like `escargot`).
//! Used to drive fake provider CLIs and the hook relay without installing
//! anything or touching real accounts.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

fn built() -> &'static Mutex<HashSet<String>> {
    static BUILT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    BUILT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// `target/<profile>` of the running test binary.
pub fn target_profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current test exe");
    // target/<profile>/deps/<test-binary>
    exe.parent()
        .and_then(|deps| deps.parent())
        .expect("test binary lives in target/<profile>/deps")
        .to_path_buf()
}

/// Builds `bin` from `package` (once per test process) and returns its path.
pub fn cargo_bin(package: &str, bin: &str) -> PathBuf {
    let dir = target_profile_dir();
    let path = dir.join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    let key = format!("{package}/{bin}");
    let mut done = built().lock().unwrap_or_else(|e| e.into_inner());
    if !done.contains(&key) {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.args(["build", "-q", "-p", package, "--bin", bin]);
        if dir.file_name().is_some_and(|n| n == "release") {
            cmd.arg("--release");
        }
        let status = cmd.status().expect("run cargo build");
        assert!(
            status.success(),
            "cargo build -p {package} --bin {bin} failed"
        );
        done.insert(key);
    }
    assert!(path.exists(), "{} was not built", path.display());
    path
}
