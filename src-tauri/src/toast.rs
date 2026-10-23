//! Desktop notifications. Clicking one brings Agent Office to the front and
//! opens the agent it is about.
//!
//! **Windows** (WinRT toasts): a toast is shown only for a known application
//! id (AppUserModelID). The installed app has one: the installer's Start
//! Menu shortcut carries the bundle identifier. A development build (run
//! from `target\debug` or `target\release`) has none, so it borrows
//! PowerShell's, like Tauri's own notification plugin does; those toasts say
//! "Windows PowerShell".
//!
//! **macOS**: see [`macos`] (only `Agent Office.app` can show them).
//!
//! Elsewhere the notice is written to the log.

use crate::notify::Notice;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;

/// The bundle identifier (`identifier` in `tauri.conf.json`).
pub const IDENTIFIER: &str = "com.agentoffice.desktop";

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

/// What this build can do on this system: (shown, development build, note).
pub fn support() -> (bool, bool, Option<String>) {
    #[cfg(target_os = "macos")]
    {
        let bundled = macos::in_app_bundle();
        let note = if bundled {
            macos::permission_note()
        } else {
            Some(
                "This is a development build: macOS shows notifications only for the installed \
                 app (Agent Office.app). Here they are written to the application log."
                    .to_string(),
            )
        };
        (bundled, !bundled, note)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let development_build = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .map(|dir| app_id_for(dir, IDENTIFIER) == DEV_APP_ID)
            })
            .unwrap_or(false);
        let note = (cfg!(windows) && development_build).then(|| {
            "This is a development build: its notifications appear as “Windows PowerShell”. \
             The installed app shows its own name."
                .to_string()
        });
        (cfg!(windows), development_build, note)
    }
}

/// Startup work (macOS: click handler, and the permission prompt when
/// notifications are on). Main thread.
pub fn init(notifications_enabled: bool) {
    #[cfg(target_os = "macos")]
    macos::init(notifications_enabled);
    #[cfg(not(target_os = "macos"))]
    let _ = notifications_enabled;
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

#[cfg(target_os = "macos")]
pub fn show(
    _app_id: &str,
    notice: &Notice,
    on_click: impl FnMut() + Send + 'static,
) -> Result<(), String> {
    macos::show(notice, on_click)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn show(
    _app_id: &str,
    notice: &Notice,
    _on_click: impl FnMut() + Send + 'static,
) -> Result<(), String> {
    tracing::info!(title = %notice.title, body = %notice.body, "notification (shown only on Windows and macOS)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_matches_the_tauri_configuration() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], IDENTIFIER);
    }

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
