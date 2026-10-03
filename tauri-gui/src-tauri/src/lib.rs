pub mod autostart;
pub mod backup;
pub mod commands;
pub mod hook;
pub mod macros;
pub mod state;
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
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            autostart::show_main_window(app, true);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            autostart::refresh();
            let background = std::env::args().any(|a| a == autostart::BACKGROUND_ARG);
            if !background {
                autostart::show_main_window(app.handle(), true);
            }
            create_tray(app.handle())?;
            autostart::spawn_watcher(app.handle().clone());

            let app_state = AppState::new();
            app.manage(app_state.clone());

            let voice: VoiceHandle = Arc::new(VoiceState::new(app.path().app_config_dir()?, app.path().app_data_dir()?));
            app.manage(voice.clone());
            voice::start(app.handle().clone(), app_state.clone(), voice);
            if let Err(e) = voice::create_overlay(app.handle()) {
                eprintln!("voice: couldn't create overlay: {e}");
            }
            
            let handle = app.handle().clone();
            
            // Spawn tokio task for auto_dialogue loop and hook init
            tauri::async_runtime::spawn(async move {
                // Initialize global keyboard hook using the tokio runtime handle
                let tokio_handle = tokio::runtime::Handle::current();
                hook::init_hook(app_state, tokio_handle);
                
                loop {
                    let (active, speed) = {
                        let state = handle.state::<AppState>();
                        let guard = state.0.lock().unwrap();
                        (guard.auto_dialogue_active, guard.auto_dialogue_speed)
                    };
                    
                    if active {
                        macros::loot_loop_step().await;
                    }
                    
                    sleep(Duration::from_millis(speed)).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_script,
            commands::stop_script,
            commands::check_status,
            commands::set_dialogue_speed,
            commands::get_voice_settings,
            commands::save_voice_settings,
            commands::list_input_devices,
            commands::voice_engine_status,
            commands::voice_download,
            commands::get_auto_open,
            commands::set_auto_open,
            commands::export_settings,
            commands::import_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => {
                if let Some(voice) = app.try_state::<VoiceHandle>() {
                    voice.kill_server_now();
                }
            }
            // With "Open with Genshin" on, closing hides to the tray so the app can pop up next time the
            // game starts; otherwise it quits (the overlay window would otherwise keep it alive).
            tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. }
                if label == "main" =>
            {
                if autostart::is_enabled() {
                    api.prevent_close();
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                } else {
                    app.exit(0);
                }
            }
            _ => {}
        });
}

/// Tray icon: click (or "Open") shows the window, "Quit" exits even when closing only hides.
fn create_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Genshin Impact Utility")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => autostart::show_main_window(app, true),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                autostart::show_main_window(tray.app_handle(), true);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
