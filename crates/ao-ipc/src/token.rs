//! Per-user IPC token stored in `<data dir>/ipc.token`.
//! On Windows the folder lives under `%LOCALAPPDATA%`, protected by the user
//! profile ACL; on Unix the file is created with mode 0600.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ipc.token")
}

pub fn read_token(data_dir: &Path) -> Option<String> {
    std::fs::read_to_string(token_path(data_dir))
        .ok()
        .map(|t| t.trim().to_owned())
        .filter(|t| t.len() >= 32)
}

/// Returns the existing token or creates a new random one.
pub fn ensure_token(data_dir: &Path) -> io::Result<String> {
    if let Some(token) = read_token(data_dir) {
        return Ok(token);
    }
    std::fs::create_dir_all(data_dir)?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let path = token_path(data_dir);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_and_reuses() {
        let dir = std::env::temp_dir().join(format!("ao-ipc-token-{}", uuid::Uuid::new_v4()));
        assert!(read_token(&dir).is_none());
        let a = ensure_token(&dir).unwrap();
        let b = ensure_token(&dir).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(token_path(&dir))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
