//! Windows notifications (WinRT toasts). Clicking one while it is on screen
//! brings Agent Office to the front and opens the agent it is about.
//!
//! Windows shows a toast only for a known application id (AppUserModelID).
//! The installed app has one: the installer's Start Menu shortcut carries the
//! bundle identifier. A development build (run from `target\debug` or
//! `target\release`) has none, so it borrows PowerShell's, like Tauri's own
//! notification plugin does; those toasts say "Windows PowerShell".
//!
//! Other systems (development only) do not show notifications; the notice is
//! written to the log.

use crate::notify::Notice;
use std::path::Path;

/// PowerShell's application id, for development builds.
pub const DEV_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// The application id to show toasts under, from the folder the executable
/// runs from.
pub fn app_id_for(exe_dir: &Path, identifier: &str) -> String {
    let dir = exe_dir.to_string_lossy().replace('\\', "/");
    let dir = dir.trim_end_matches('/');
    if dir.ends_with("/target/debug") || dir.ends_with("/target/release") {
        DEV_APP_ID.to_owned()
    } else {
        identifier.to_owned()
    }
}

/// Shows `notice`; `on_click` runs when the user clicks it.
#[cfg(windows)]
pub fn show(
    app_id: &str,
    notice: &Notice,
    mut on_click: impl FnMut() + Send + 'static,
) -> Result<(), String> {
    use tauri_winrt_notification::{Duration, Sound, Toast};
    Toast::new(app_id)
        .title(&notice.title)
        .text1(&notice.body)
        .duration(Duration::Short)
        .sound(Some(Sound::Default))
        .on_activated(move |_| {
            on_click();
            Ok(())
        })
        .show()
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn show(
    _app_id: &str,
    notice: &Notice,
    _on_click: impl FnMut() + Send + 'static,
) -> Result<(), String> {
    tracing::info!(title = %notice.title, body = %notice.body, "notification (shown only on Windows)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_builds_borrow_powershells_id() {
        let id = "com.agentoffice.desktop";
        assert_eq!(
            app_id_for(Path::new(r"C:\src\AI-Agent-Office\target\debug"), id),
            DEV_APP_ID
        );
        assert_eq!(
            app_id_for(Path::new("/home/me/AI-Agent-Office/target/release/"), id),
            DEV_APP_ID
        );
        assert_eq!(
            app_id_for(Path::new(r"C:\Users\me\AppData\Local\Agent Office"), id),
            id
        );
    }
}
