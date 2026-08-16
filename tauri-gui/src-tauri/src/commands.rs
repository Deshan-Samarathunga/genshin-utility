use tauri::State;
use crate::state::AppState;

#[tauri::command]
pub async fn start_script(script_name: String, args: Option<Vec<String>>, state: State<'_, AppState>) -> Result<String, String> {
    let mut macro_state = state.0.lock().unwrap();

    match script_name.as_str() {
        "auto_dialogue.ahk" => {
            macro_state.auto_dialogue = true;
            // auto_dialogue_active toggles via F4, but we can reset it to false
            macro_state.auto_dialogue_active = false;
        }
        "genshin_batch_artifact_remover.ahk" => {
            macro_state.artifact_remover = true;
        }
        "auto_message.ahk" => {
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
        _ => return Err(format!("Unknown script: {}", script_name)),
    }

    Ok(format!("Enabled {}", script_name))
}

#[tauri::command]
pub async fn stop_script(script_name: String, state: State<'_, AppState>) -> Result<String, String> {
    let mut macro_state = state.0.lock().unwrap();

    match script_name.as_str() {
        "auto_dialogue.ahk" => {
            macro_state.auto_dialogue = false;
            macro_state.auto_dialogue_active = false;
        }
        "genshin_batch_artifact_remover.ahk" => {
            macro_state.artifact_remover = false;
        }
        "auto_message.ahk" => {
            macro_state.auto_message = false;
        }
        _ => return Err(format!("Unknown script: {}", script_name)),
    }

    Ok(format!("Disabled {}", script_name))
}

#[tauri::command]
pub async fn check_status(script_name: String, state: State<'_, AppState>) -> Result<bool, String> {
    let macro_state = state.0.lock().unwrap();

    let status = match script_name.as_str() {
        "auto_dialogue.ahk" => macro_state.auto_dialogue,
        "genshin_batch_artifact_remover.ahk" => macro_state.artifact_remover,
        "auto_message.ahk" => macro_state.auto_message,
        _ => false,
    };

    Ok(status)
}
