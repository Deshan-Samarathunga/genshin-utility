pub mod commands;
pub mod hook;
pub mod macros;
pub mod state;

use state::AppState;
use tauri::Manager;
use tokio::time::{sleep, Duration};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_state = AppState::new();
            app.manage(app_state.clone());
            
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
