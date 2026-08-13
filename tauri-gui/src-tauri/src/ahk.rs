use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{PidExt, ProcessExt, System, SystemExt};

static AHK_EXE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

pub fn resolve_ahk_exe() -> Option<PathBuf> {
    AHK_EXE
        .get_or_init(|| {
            let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
            let mut candidates = vec![
                PathBuf::from(r"C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe"),
                PathBuf::from(r"C:\Program Files\AutoHotkey\v2\AutoHotkey32.exe"),
                PathBuf::from(r"C:\Program Files (x86)\AutoHotkey\v2\AutoHotkey64.exe"),
                PathBuf::from(r"C:\Program Files\AutoHotkey\AutoHotkey.exe"),
            ];
            
            if !local_app_data.is_empty() {
                candidates.push(
                    Path::new(&local_app_data)
                        .join(r"Programs\AutoHotkey\v2\AutoHotkey64.exe")
                );
            }

            candidates.into_iter().find(|p| p.exists())
        })
        .clone()
}

pub fn get_ahk_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;

    // In production, resources are bundled next to the exe
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let bundled = resource_dir.join("ahk");
        if bundled.exists() {
            return bundled;
        }
    }

    // In dev, walk up from the exe (target/debug/) to find the project root's ahk/ folder
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            let candidate = d.join("ahk");
            if candidate.exists() {
                return candidate;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    // Last resort: check two levels up from src-tauri (the repo root)
    let fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()  // tauri-gui/
        .and_then(|p| p.parent())  // genshin-autohotkey/
        .map(|p| p.join("ahk"))
        .unwrap_or_else(|| PathBuf::from("ahk"));
    
    fallback
}

#[derive(Clone)]
pub struct AhkProcess {
    pub pid: u32,
    pub cmd: String,
}

static PROC_CACHE: Mutex<Option<(Instant, Vec<AhkProcess>)>> = Mutex::new(None);

pub fn list_ahk_processes() -> Vec<AhkProcess> {
    let mut cache = PROC_CACHE.lock().unwrap();
    let now = Instant::now();
    
    if let Some((last_update, procs)) = cache.as_ref() {
        if now.duration_since(*last_update) < Duration::from_millis(800) {
            return procs.clone();
        }
    }
    
    let mut sys = System::new_all();
    sys.refresh_processes();
    
    let mut procs = Vec::new();
    for (pid, process) in sys.processes() {
        if process.name().to_lowercase().starts_with("autohotkey") {
            let cmd = process.cmd().join(" ");
            procs.push(AhkProcess {
                pid: pid.as_u32(),
                cmd,
            });
        }
    }
    
    *cache = Some((now, procs.clone()));
    procs
}

pub fn pids_for_script(script_name: &str) -> Vec<u32> {
    let needle = script_name.to_lowercase();
    list_ahk_processes()
        .into_iter()
        .filter(|p| p.cmd.to_lowercase().contains(&needle))
        .map(|p| p.pid)
        .collect()
}

pub fn invalidate_cache() {
    if let Ok(mut cache) = PROC_CACHE.lock() {
        *cache = None;
    }
}
