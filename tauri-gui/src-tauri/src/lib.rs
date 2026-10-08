pub mod artifacts;
pub mod autostart;
pub mod backup;
pub mod commands;
pub mod hook;
pub mod hotkeys;
pub mod macros;
pub mod state;
pub mod story;
pub mod updater;
pub mod voice;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;
use voice::{VoiceHandle, VoiceState};
use tokio::time::{sleep, Duration};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch just brings up the running instance (one hook, one controller reader).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A launch from the Genshin task shouldn't steal focus from the game.
            let from_genshin = args.iter().any(|a| a == autostart::FROM_GENSHIN_ARG);
            autostart::show_main_window(app, !from_genshin);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::PendingUpdate::default())
        .setup(|app| {
            let from_genshin = std::env::args().any(|a| a == autostart::FROM_GENSHIN_ARG);
            autostart::show_main_window(app.handle(), !from_genshin);
            let upgrade_handle = app.handle().clone();
            std::thread::spawn(move || autostart::upgrade_if_outdated(&upgrade_handle));

            let app_state = AppState::new();
            app.manage(app_state.clone());

            let voice: VoiceHandle = Arc::new(VoiceState::new(app.path().app_config_dir()?, app.path().app_data_dir()?));
            app.manage(voice.clone());
            voice::start(app.handle().clone(), app_state.clone(), voice);
            if let Err(e) = voice::create_overlay(app.handle()) {
                eprintln!("voice: couldn't create overlay: {e}");
            }
            
            artifacts::set_app(app.handle().clone());
            let story: story::StoryHandle = Default::default();
            story.load_history(app.path().app_config_dir()?.join("story_history.json"));
            app.manage(story.clone());

            let handle = app.handle().clone();
            
            // Spawn tokio task for auto_dialogue loop and hook init
            tauri::async_runtime::spawn(async move {
                // Initialize global keyboard hook using the tokio runtime handle
                let tokio_handle = tokio::runtime::Handle::current();
                hook::init_hook(app_state, tokio_handle);
                
                let mut was_active = false;
                loop {
                    let (active, story_mode, speed) = {
                        let state = handle.state::<AppState>();
                        let guard = state.0.lock().unwrap();
                        let speed = if guard.story_mode { guard.story_speed } else { guard.auto_dialogue_speed };
                        (guard.auto_dialogue_active, guard.story_mode, speed)
                    };

                    let mut wait = speed;
                    if active && story_mode {
                        if hook::is_genshin_active() {
                            wait = story::step(&handle, &story, speed).await;
                        }
                    } else if active {
                        macros::loot_loop_step().await;
                    }
                    if was_active && !(active && story_mode) {
                        // Story run stopped (F4, the toggle, or a switch to plain Auto Dialogue).
                        tauri::async_runtime::spawn(story::finish(handle.clone(), story.clone()));
                    }
                    was_active = active && story_mode;
                    
                    sleep(Duration::from_millis(wait)).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_script,
            commands::stop_script,
            commands::check_status,
            commands::set_dialogue_speed,
            commands::set_story_speed,
            commands::get_voice_settings,
            commands::save_voice_settings,
            commands::list_input_devices,
            commands::voice_engine_status,
            commands::voice_download,
            commands::get_auto_open,
            commands::set_auto_open,
            commands::export_settings,
            commands::import_settings,
            updater::check_update,
            story::set_story_settings,
            story::story_history,
            story::story_delete,
            story::story_transcript,
            story::story_merge,
            artifacts::set_artifact_speed,
            hotkeys::set_hotkeys,
            hotkeys::default_hotkeys,
            story::story_retry_pending,
            updater::install_update,
            updater::is_portable,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => {
                if let Some(voice) = app.try_state::<VoiceHandle>() {
                    voice.kill_server_now();
                }
            }
            // Closing the main window quits the app (the overlay window would otherwise keep it alive).
            tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { .. }, .. }
                if label == "main" =>
            {
                app.exit(0);
            }
            _ => {}
        });
}

