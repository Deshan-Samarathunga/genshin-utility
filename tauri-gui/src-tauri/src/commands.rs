use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};
use crate::state::AppState;
use crate::voice::settings::VoiceSettings;
use crate::voice::{setup, VoiceHandle};

#[tauri::command]
pub async fn start_script(script_name: String, args: Option<Vec<String>>, state: State<'_, AppState>) -> Result<String, String> {
    let mut macro_state = state.0.lock().unwrap();

    match script_name.as_str() {
        "auto_dialogue" => {
            macro_state.auto_dialogue = true;
            macro_state.story_mode = false;
            // auto_dialogue_active toggles via F4, but we can reset it to false
            macro_state.auto_dialogue_active = false;
        }
        "story_mode" => {
            macro_state.story_mode = true;
            macro_state.auto_dialogue = false;
            macro_state.auto_dialogue_active = false;
        }
        "artifact_remover" => {
            macro_state.artifact_remover = true;
        }
        "auto_message" => {
            macro_state.auto_message = true;
            if let Some(args_list) = args {
                if args_list.len() >= 1 {
                    macro_state.auto_message_text = args_list[0].clone();
                }
                if args_list.len() >= 2 {
                    if let Ok(count) = args_list[1].parse::<u32>() {
                        macro_state.auto_message_count = count;
                    }
                }
            }
        }
        "voice_chat" => {
            macro_state.voice_chat = true;
        }
        _ => return Err(format!("Unknown script: {}", script_name)),
    }

    Ok(format!("Enabled {}", script_name))
}

#[tauri::command]
pub async fn stop_script(script_name: String, state: State<'_, AppState>) -> Result<String, String> {
    let mut macro_state = state.0.lock().unwrap();

    match script_name.as_str() {
        "auto_dialogue" => {
            macro_state.auto_dialogue = false;
            macro_state.auto_dialogue_active = false;
        }
        "story_mode" => {
            macro_state.story_mode = false;
            macro_state.auto_dialogue_active = false;
        }
        "artifact_remover" => {
            macro_state.artifact_remover = false;
        }
        "auto_message" => {
            macro_state.auto_message = false;
        }
        "voice_chat" => {
            macro_state.voice_chat = false;
        }
        _ => return Err(format!("Unknown script: {}", script_name)),
    }

    Ok(format!("Disabled {}", script_name))
}

#[tauri::command]
pub async fn check_status(script_name: String, state: State<'_, AppState>) -> Result<bool, String> {
    let macro_state = state.0.lock().unwrap();

    let status = match script_name.as_str() {
        "auto_dialogue" => macro_state.auto_dialogue,
        "story_mode" => macro_state.story_mode,
        "artifact_remover" => macro_state.artifact_remover,
        "auto_message" => macro_state.auto_message,
        "voice_chat" => macro_state.voice_chat,
        _ => false,
    };

    Ok(status)
}

#[tauri::command]
pub async fn set_dialogue_speed(speed: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut macro_state = state.0.lock().unwrap();
    macro_state.auto_dialogue_speed = speed;
    Ok(())
}

#[tauri::command]
pub async fn set_story_speed(speed: u64, state: State<'_, AppState>) -> Result<(), String> {
    state.0.lock().unwrap().story_speed = speed.max(50);
    Ok(())
}

/// Saves all settings to a JSON file the user picks. `ui` carries the page-side settings.
#[tauri::command]
pub async fn export_settings(
    app: AppHandle,
    ui: serde_json::Value,
    exported_at: String,
    file_name: String,
) -> Result<Option<String>, String> {
    crate::backup::export(app, ui, exported_at, file_name).await
}

/// Loads a settings file the user picks; returns the page-side settings to apply.
#[tauri::command]
pub async fn import_settings(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    crate::backup::import(app).await
}

/// Whether "Open with Genshin" is on (reads Task Scheduler).
#[tauri::command]
pub fn get_auto_open() -> bool {
    crate::autostart::is_enabled()
}

#[derive(serde::Serialize)]
pub struct AutoOpenResult {
    enabled: bool,
    /// The game exe the launch task watches (set when just enabled).
    game_path: Option<String>,
}

/// Turns "Open with Genshin" on/off. May show a file picker if the game can't be found.
#[tauri::command]
pub async fn set_auto_open(enabled: bool, app: AppHandle) -> Result<AutoOpenResult, String> {
    let (enabled, game_path) = tauri::async_runtime::spawn_blocking(move || crate::autostart::set_enabled(&app, enabled))
        .await
        .map_err(|e| e.to_string())??;
    Ok(AutoOpenResult { enabled, game_path })
}

#[tauri::command]
pub fn get_voice_settings(voice: State<'_, VoiceHandle>) -> VoiceSettings {
    voice.settings()
}

#[tauri::command]
pub fn save_voice_settings(settings: VoiceSettings, voice: State<'_, VoiceHandle>) -> Result<(), String> {
    voice.save_settings(settings)
}

#[tauri::command]
pub fn list_input_devices() -> Vec<String> {
    crate::voice::audio::input_device_names()
}

#[derive(serde::Serialize)]
pub struct VoiceEngineStatus {
    engine_cpu: bool,
    engine_gpu: bool,
    downloaded_models: Vec<String>,
    models: Vec<String>,
}

#[tauri::command]
pub fn voice_engine_status(voice: State<'_, VoiceHandle>) -> VoiceEngineStatus {
    let root = &voice.data_root;
    let models_dir = setup::models_dir(root);
    VoiceEngineStatus {
        engine_cpu: setup::find_server_exe(&setup::engine_dir(root, false)).is_some(),
        engine_gpu: setup::find_server_exe(&setup::engine_dir(root, true)).is_some(),
        downloaded_models: setup::MODELS
            .iter()
            .filter(|m| crate::voice::stt::model_path(&models_dir, m).exists())
            .map(|m| m.to_string())
            .collect(),
        models: setup::MODELS.iter().map(|m| m.to_string()).collect(),
    }
}

/// Downloads the local engine ("engine") or the selected model ("model"), emitting `voice-download` progress.
#[tauri::command]
pub async fn voice_download(what: String, app: AppHandle, voice: State<'_, VoiceHandle>) -> Result<String, String> {
    let voice = voice.inner().clone();
    if voice.downloading.swap(true, Ordering::SeqCst) {
        return Err("A download is already running".into());
    }
    let settings = voice.settings();
    let mut last_emit = 0u64;
    let progress = |done: u64, total: Option<u64>| {
        if done - last_emit >= 512 * 1024 || Some(done) == total {
            last_emit = done;
            let _ = app.emit("voice-download", serde_json::json!({ "what": what, "done": done, "total": total }));
        }
    };
    let result = match what.as_str() {
        "engine" => setup::download_engine(&voice.client, &voice.data_root, settings.local_gpu, progress)
            .await
            .map(|_| format!("Engine ({}) ready", if settings.local_gpu { "GPU" } else { "CPU" })),
        "model" => setup::download_model(&voice.client, &voice.data_root, &settings.local_model, progress)
            .await
            .map(|_| format!("Model {} ready", settings.local_model)),
        other => Err(format!("Unknown download: {other}")),
    };
    voice.downloading.store(false, Ordering::SeqCst);
    result
}
