//! Settings backup: export everything to one JSON file and import it after a fresh install.

use crate::voice::settings::VoiceSettings;
use crate::voice::VoiceHandle;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const APP_ID: &str = "genshin-utility";
const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct SettingsBackup {
    app: String,
    version: u32,
    #[serde(default)]
    exported_at: String,
    #[serde(default)]
    open_with_genshin: bool,
    /// Page-side settings (dialogue delay, auto-message text/count), owned by the frontend.
    #[serde(default)]
    ui: serde_json::Value,
    voice: VoiceSettings,
}

impl SettingsBackup {
    fn parse(json: &str) -> Result<Self, String> {
        let backup: Self = serde_json::from_str(json).map_err(|e| format!("Not a valid settings file: {e}"))?;
        if backup.app != APP_ID {
            return Err("This file isn't a Genshin Utility settings backup".into());
        }
        if backup.version > FORMAT_VERSION {
            return Err("This backup was made by a newer version of the app".into());
        }
        Ok(backup)
    }
}

fn dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let mut builder = app.dialog().file().add_filter("Settings backup", &["json"]);
    if let Some(window) = app.get_webview_window("main") {
        builder = builder.set_parent(&window);
    }
    builder
}

/// Asks where to save, then writes the backup. Returns the saved path, or None if cancelled.
pub async fn export(app: AppHandle, ui: serde_json::Value, exported_at: String, file_name: String) -> Result<Option<String>, String> {
    let backup = SettingsBackup {
        app: APP_ID.into(),
        version: FORMAT_VERSION,
        exported_at,
        open_with_genshin: crate::autostart::refresh(),
        ui,
        voice: app.state::<VoiceHandle>().settings(),
    };
    let json = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;

    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog(&picker).set_title("Export settings").set_file_name(file_name).blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Couldn't write the file: {e}"))?;
    Ok(Some(path.display().to_string()))
}

/// Asks for a backup file and applies it. Returns the page-side settings for the frontend, or None if cancelled.
pub async fn import(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    let picker = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || dialog(&picker).set_title("Import settings").blocking_pick_file())
        .await
        .map_err(|e| e.to_string())?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let json = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read the file: {e}"))?;
    let backup = SettingsBackup::parse(&json)?;

    app.state::<VoiceHandle>().save_settings(backup.voice)?;
    crate::autostart::set_enabled(backup.open_with_genshin)?;
    Ok(Some(backup.ui))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_foreign_files() {
        let backup = SettingsBackup {
            app: APP_ID.into(),
            version: FORMAT_VERSION,
            exported_at: "2026-10-03T12:00:00Z".into(),
            open_with_genshin: true,
            ui: serde_json::json!({ "dialogue_speed": 800 }),
            voice: VoiceSettings::default(),
        };
        let json = serde_json::to_string(&backup).unwrap();
        let parsed = SettingsBackup::parse(&json).unwrap();
        assert!(parsed.open_with_genshin);
        assert_eq!(parsed.ui["dialogue_speed"], 800);
        assert_eq!(parsed.voice.engine, "local");

        assert!(SettingsBackup::parse(r#"{"app":"other","version":1,"voice":{}}"#).is_err());
        assert!(SettingsBackup::parse(r#"{"app":"genshin-utility","version":99,"voice":{}}"#).is_err());
        assert!(SettingsBackup::parse("not json").is_err());
    }
}
