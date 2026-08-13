use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::ahk;

const DETACHED_PROCESS: u32 = 0x00000008;
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[tauri::command]
pub async fn start_script(script_name: String, args: Option<Vec<String>>, app_handle: tauri::AppHandle) -> Result<String, String> {
    let ahk_exe = ahk::resolve_ahk_exe().ok_or("AutoHotkey v2 not found. Please install it from autohotkey.com.")?;
    
    let script_path = ahk::get_ahk_dir(&app_handle).join(&script_name);
    if !script_path.exists() {
        return Err(format!("Script not found: {}", script_path.display()));
    }
    
    // Kill any existing instances first to avoid AHK's "Could not close previous instance" prompt
    let pids = ahk::pids_for_script(&script_name);
    for pid in pids {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    
    let mut cmd = Command::new(ahk_exe);
    cmd.arg(&script_path);
    
    if let Some(args_list) = args {
        for arg in args_list {
            cmd.arg(arg);
        }
    }
    
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to start script: {}", e))?;
        
    ahk::invalidate_cache();
    Ok(format!("Started {}", script_name))
}

#[tauri::command]
pub async fn stop_script(script_name: String) -> Result<String, String> {
    let pids = ahk::pids_for_script(&script_name);
    if pids.is_empty() {
        return Ok(format!("Stopped {} (was not running)", script_name));
    }
    
    for pid in pids {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    
    ahk::invalidate_cache();
    Ok(format!("Stopped {}", script_name))
}

#[tauri::command]
pub async fn check_status(script_name: String) -> Result<bool, String> {
    Ok(!ahk::pids_for_script(&script_name).is_empty())
}
