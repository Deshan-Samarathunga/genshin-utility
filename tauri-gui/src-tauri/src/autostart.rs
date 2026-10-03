//! "Open with Genshin": a logon scheduled task starts the app hidden (elevated, so no UAC prompt), and a
//! watcher shows the window whenever GenshinImpact.exe starts. While enabled, closing hides to the tray.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, System, SystemExt};
use tauri::{AppHandle, Manager};

const TASK_NAME: &str = "GenshinUtility-OpenWithGenshin";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const GAME_EXES: [&str; 2] = ["GenshinImpact.exe", "YuanShen.exe"];
/// Passed by the scheduled task: start without showing the window.
pub const BACKGROUND_ARG: &str = "--background";

/// Cached so the close handler doesn't have to ask Task Scheduler.
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn schtasks(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("schtasks").args(args).creation_flags(CREATE_NO_WINDOW).output()
}

/// Reads the real state from Task Scheduler (and refreshes the cache).
pub fn refresh() -> bool {
    let exists = schtasks(&["/Query", "/TN", TASK_NAME]).map(|o| o.status.success()).unwrap_or(false);
    ENABLED.store(exists, Ordering::Relaxed);
    exists
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Task definition: at logon, run this exe elevated with --background; allowed on battery, no time limit.
fn task_xml(exe: &str, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Genshin Impact Utility in the background so it opens when Genshin Impact starts.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT10S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <Enabled>true</Enabled>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{BACKGROUND_ARG}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        exe = xml_escape(exe),
        user = xml_escape(user),
    )
}

pub fn set_enabled(enabled: bool) -> Result<bool, String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap_or_default(),
            std::env::var("USERNAME").map_err(|_| "Couldn't read the Windows user name")?
        );
        // Task Scheduler expects the XML file as UTF-16 LE with a BOM.
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(task_xml(&exe.to_string_lossy(), &user).encode_utf16().flat_map(u16::to_le_bytes));
        let path = std::env::temp_dir().join("genshin-utility-task.xml");
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let output = schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &path.to_string_lossy(), "/F"]);
        let _ = std::fs::remove_file(&path);
        let output = output.map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!("Couldn't create the startup task: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
    } else if refresh() {
        let output = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]).map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!("Couldn't remove the startup task: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
    }
    Ok(refresh())
}

/// Shows (and un-minimizes) the main window. With `activate` false the game keeps keyboard focus.
pub fn show_main_window(app: &AppHandle, activate: bool) {
    let Some(window) = app.get_webview_window("main") else { return };
    if activate {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, ShowWindow, SW_SHOWNOACTIVATE};
    if let Ok(hwnd) = window.hwnd() {
        let hwnd = HWND(hwnd.0);
        unsafe {
            // SW_SHOWNOACTIVATE also restores a minimized window to its previous size.
            if !window.is_visible().unwrap_or(false) || IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }
}

/// Polls for the game every few seconds and shows the window each time it starts.
pub fn spawn_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut sys = System::new();
        let mut was_running = false;
        loop {
            sys.refresh_processes_specifics(ProcessRefreshKind::new());
            let running = GAME_EXES.iter().any(|exe| sys.processes_by_exact_name(exe).next().is_some());
            if running && !was_running && is_enabled() {
                show_main_window(&app, false);
            }
            was_running = running;
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_escapes_and_sets_battery_and_time_limits() {
        let xml = task_xml(r"C:\Apps\A & B\app.exe", r"PC\me");
        assert!(xml.contains(r"<Command>C:\Apps\A &amp; B\app.exe</Command>"));
        assert!(xml.contains("<Arguments>--background</Arguments>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
    }
}
