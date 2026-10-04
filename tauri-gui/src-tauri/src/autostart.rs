//! "Open with Genshin": Windows launches the app whenever the game starts, with nothing of ours running
//! in the background. A Task Scheduler task is triggered by Security event 4688 ("a new process has been
//! created") for the game's exact .exe path. That event needs the "Audit Process Creation" policy, which
//! is turned on here if needed and turned back off on disable if we were the ones who enabled it.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use sysinfo::{ProcessExt, ProcessRefreshKind, System, SystemExt};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const TASK_NAME: &str = "GenshinUtility-OpenWithGenshin";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const GAME_EXES: [&str; 2] = ["GenshinImpact.exe", "YuanShen.exe"];
/// "Process Creation" audit subcategory (GUID form works on every Windows language).
const AUDIT_PROCESS_CREATION: &str = "{0CCE922B-69AE-11D9-BED3-505054503030}";
/// Marker file: present when this app turned process-creation auditing on.
const AUDIT_MARKER: &str = "audit-enabled-by-app";
/// Passed by the task: show the window without taking focus from the game.
pub const FROM_GENSHIN_ARG: &str = "--from-genshin";

/// Common install locations, relative to a drive root.
const KNOWN_LOCATIONS: &[&str] = &[
    r"Program Files\HoYoPlay\games\Genshin Impact game",
    r"HoYoPlay\games\Genshin Impact game",
    r"Program Files\Genshin Impact\Genshin Impact game",
    r"Genshin Impact\Genshin Impact game",
    r"Games\Genshin Impact game",
    r"Games\Genshin Impact\Genshin Impact game",
    r"Program Files\HoYoPlay\games\YuanShen game",
];

fn run(program: &str, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Couldn't run {program}: {e}"))
}

fn check(output: std::process::Output, what: &str) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(if output.stderr.is_empty() { &output.stdout } else { &output.stderr });
        Err(format!("{what}: {}", detail.trim()))
    }
}

/// Whether the launch-on-Genshin task exists.
pub fn is_enabled() -> bool {
    run("schtasks", &["/Query", "/TN", TASK_NAME]).map(|o| o.status.success()).unwrap_or(false)
}

/// The on-disk spelling of a path (the audit event logs real casing, and the trigger match is exact).
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\").to_string()))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Finds the game: a running copy first, then the usual install folders on every drive.
fn detect_game() -> Option<PathBuf> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessRefreshKind::new());
    for exe in GAME_EXES {
        if let Some(process) = sys.processes_by_exact_name(exe).next() {
            if process.exe().is_file() {
                return Some(canonical(process.exe()));
            }
        }
    }
    for drive in 'C'..='Z' {
        for location in KNOWN_LOCATIONS {
            for exe in GAME_EXES {
                let path = PathBuf::from(format!(r"{drive}:\{location}\{exe}"));
                if path.is_file() {
                    return Some(canonical(&path));
                }
            }
        }
    }
    None
}

/// Asks the user to point at the game's .exe.
fn pick_game(app: &AppHandle) -> Option<PathBuf> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Select GenshinImpact.exe (inside the \"Genshin Impact game\" folder)")
        .add_filter("Genshin Impact", &["exe"]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.blocking_pick_file()?.into_path().ok().map(|p| canonical(&p))
}

fn audit_enabled() -> Result<bool, String> {
    let output = run("auditpol", &["/get", &format!("/subcategory:{AUDIT_PROCESS_CREATION}"), "/r"])?;
    if !output.status.success() {
        return Err("Couldn't read the Windows audit policy".into());
    }
    // CSV: ...,Inclusion Setting,Exclusion Setting — "Success" or "Success and Failure" when on.
    let csv = String::from_utf8_lossy(&output.stdout);
    let row = csv.lines().find(|l| l.contains(AUDIT_PROCESS_CREATION)).unwrap_or_default();
    Ok(row.contains("Success"))
}

fn set_audit(enable: bool) -> Result<(), String> {
    let flag = if enable { "/success:enable" } else { "/success:disable" };
    check(
        run("auditpol", &["/set", &format!("/subcategory:{AUDIT_PROCESS_CREATION}"), flag])?,
        "Couldn't change the Windows audit policy",
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// XPath string literal (paths may contain apostrophes).
fn xpath_literal(s: &str) -> String {
    if !s.contains('\'') {
        format!("'{s}'")
    } else if !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("concat('{}')", s.split('\'').collect::<Vec<_>>().join("', \"'\", '"))
    }
}

/// Task definition: when the game's process starts, run this exe with --from-genshin.
fn task_xml(app_exe: &str, game_exe: &str, user: &str) -> String {
    let query = format!(
        "<QueryList><Query Id=\"0\" Path=\"Security\"><Select Path=\"Security\">\
         *[System[(EventID=4688)]] and *[EventData[Data[@Name='NewProcessName']={}]]\
         </Select></Query></QueryList>",
        xpath_literal(game_exe)
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Opens Genshin Impact Utility when Genshin Impact starts.</Description>
  </RegistrationInfo>
  <Triggers>
    <EventTrigger>
      <Enabled>true</Enabled>
      <Subscription>{subscription}</Subscription>
    </EventTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy>
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
      <Command>{app_exe}</Command>
      <Arguments>{FROM_GENSHIN_ARG}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        subscription = xml_escape(&query),
        app_exe = xml_escape(app_exe),
        user = xml_escape(user),
    )
}

fn marker_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(AUDIT_MARKER))
}

/// `allow_pick`: ask the user to locate the game if it can't be found (off for silent upgrades).
fn enable(app: &AppHandle, allow_pick: bool) -> Result<Option<PathBuf>, String> {
    let game = detect_game().or_else(|| if allow_pick { pick_game(app) } else { None });
    let Some(game) = game else {
        return Ok(None); // not found, or the user cancelled the picker
    };
    let file_name = game.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if !GAME_EXES.iter().any(|exe| exe.eq_ignore_ascii_case(&file_name)) {
        return Err(format!("Please pick GenshinImpact.exe, not {file_name}"));
    }

    if !audit_enabled()? {
        set_audit(true)?;
        if let Some(marker) = marker_path(app) {
            let _ = std::fs::create_dir_all(marker.parent().unwrap_or(Path::new(".")));
            let _ = std::fs::write(marker, "Process-creation auditing was enabled by Genshin Impact Utility.");
        }
    }

    let app_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").map_err(|_| "Couldn't read the Windows user name")?
    );
    // Task Scheduler expects the XML file as UTF-16 LE with a BOM.
    let mut bytes = vec![0xFF, 0xFE];
    let xml = task_xml(&app_exe.to_string_lossy(), &game.to_string_lossy(), &user);
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    let xml_path = std::env::temp_dir().join("genshin-utility-task.xml");
    std::fs::write(&xml_path, bytes).map_err(|e| e.to_string())?;
    let created = run("schtasks", &["/Create", "/TN", TASK_NAME, "/XML", &xml_path.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&xml_path);
    check(created?, "Couldn't create the launch task")?;
    Ok(Some(game))
}

fn disable(app: &AppHandle) -> Result<(), String> {
    if is_enabled() {
        check(run("schtasks", &["/Delete", "/TN", TASK_NAME, "/F"])?, "Couldn't remove the launch task")?;
    }
    // Only undo auditing if this app turned it on.
    if let Some(marker) = marker_path(app).filter(|m| m.exists()) {
        set_audit(false)?;
        let _ = std::fs::remove_file(marker);
    }
    Ok(())
}

/// Turns the feature on or off. Returns (enabled, game path when just enabled).
pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(bool, Option<String>), String> {
    if enabled {
        let game = enable(app, true)?;
        Ok((game.is_some(), game.map(|p| p.display().to_string())))
    } else {
        disable(app)?;
        Ok((false, None))
    }
}

fn task_definition() -> Option<String> {
    let output = run("schtasks", &["/Query", "/TN", TASK_NAME, "/XML"]).ok()?;
    if !output.status.success() {
        return None;
    }
    // schtasks may emit UTF-16; dropping NULs leaves the ASCII-compatible XML readable.
    Some(String::from_utf8_lossy(&output.stdout).replace('\0', ""))
}

/// Re-registers an existing task that's outdated: the old start-at-sign-in kind from 1.0.0, or one that
/// launches a different copy of the app (moved / reinstalled). Runs silently at startup.
pub fn upgrade_if_outdated(app: &AppHandle) {
    let Some(xml) = task_definition() else { return };
    let Ok(exe) = std::env::current_exe() else { return };
    let launches_this_exe = xml.to_lowercase().contains(&xml_escape(&exe.to_string_lossy()).to_lowercase());
    if xml.contains(FROM_GENSHIN_ARG) && launches_this_exe {
        return;
    }
    if let Err(e) = enable(app, false) {
        eprintln!("autostart: couldn't upgrade the launch task: {e}");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_triggers_on_the_game_exe() {
        let xml = task_xml(
            r"C:\Program Files\Genshin Impact Utility\Genshin Impact Utility.exe",
            r"C:\Games\Genshin Impact game\GenshinImpact.exe",
            r"PC\me",
        );
        assert!(xml.contains("<EventTrigger>"));
        // The XPath query is XML-escaped inside <Subscription>.
        assert!(xml.contains("EventID=4688"));
        assert!(xml.contains("@Name=&apos;NewProcessName&apos;]=&apos;C:\\Games\\Genshin Impact game\\GenshinImpact.exe&apos;"));
        assert!(xml.contains("<Arguments>--from-genshin</Arguments>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(!xml.contains("LogonTrigger"));
    }

    #[test]
    fn xpath_literal_handles_quotes() {
        assert_eq!(xpath_literal(r"C:\a\b.exe"), r"'C:\a\b.exe'");
        assert_eq!(xpath_literal(r"C:\Bob's\b.exe"), r#""C:\Bob's\b.exe""#);
    }
}
