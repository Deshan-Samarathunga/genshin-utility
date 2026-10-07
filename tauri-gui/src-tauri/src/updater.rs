//! In-app updates from GitHub Releases. A release carries a signed installer plus `latest.json`
//! (see `scripts/updater-manifest.mjs`); the updater plugin checks the signature against the public
//! key in `tauri.conf.json` before running the installer.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The update found by the last check, kept so "install" doesn't have to look it up again.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

#[derive(Serialize, Clone)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
pub async fn check_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Result<Option<UpdateInfo>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("Couldn't check for updates: {e}"))?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
    });
    *pending.0.lock().unwrap() = update;
    Ok(info)
}

/// Downloads and runs the installer. On Windows the installer closes this app and starts the new
/// version when it's done, so on success this never returns.
#[tauri::command]
pub async fn install_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Result<(), String> {
    let update = pending.0.lock().unwrap().take().ok_or("No update to install — check again.")?;
    let mut downloaded = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit("update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    // The installer exits this process directly, so RunEvent::Exit cleanup won't run.
    if let Some(voice) = app.try_state::<crate::voice::VoiceHandle>() {
        voice.kill_server_now();
    }
    update.install(bytes).map_err(|e| format!("Couldn't start the installer: {e}"))?;
    app.restart();
}

/// Portable copies (file name contains "portable") update by downloading the new portable exe;
/// running the installer would put a second, installed copy on the PC.
#[tauri::command]
pub fn is_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase().contains("portable")))
        .unwrap_or(false)
}
