use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::ProcessStatus::GetProcessImageFileNameW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, SetWindowsHookExW,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::state::AppState;

lazy_static::lazy_static! {
    static ref APP_STATE_REF: std::sync::RwLock<Option<(AppState, tokio::runtime::Handle)>> = std::sync::RwLock::new(None);
}

pub fn init_hook(state: AppState, handle: tokio::runtime::Handle) {
    *APP_STATE_REF.write().unwrap() = Some((state, handle));
    
    thread::spawn(|| {
        unsafe {
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook_proc),
                None,
                0,
            );

            if hook.is_err() {
                eprintln!("Failed to install keyboard hook");
                return;
            }
            // Mouse side buttons for voice push-to-talk.
            let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0);
            if mouse_hook.is_err() {
                eprintln!("Failed to install mouse hook");
            }

            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                // message loop to keep hook alive
            }

            let _ = UnhookWindowsHookEx(hook.unwrap());
            if let Ok(mouse_hook) = mouse_hook {
                let _ = UnhookWindowsHookEx(mouse_hook);
            }
        }
    });
}

pub(crate) fn is_genshin_active() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == std::ptr::null_mut() {
            return false;
        }

        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buffer = [0u16; 512];
            let len = GetProcessImageFileNameW(process, &mut buffer);
            if len > 0 {
                let path = String::from_utf16_lossy(&buffer[..len as usize]);
                return path.ends_with("GenshinImpact.exe") || path.ends_with("YuanShen.exe");
            }
        }
    }
    false
}

unsafe extern "system" fn mouse_hook_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 {
        let msg = w_param.0 as u32;
        if msg == WM_XBUTTONDOWN || msg == WM_XBUTTONUP {
            let info = *(l_param.0 as *const MSLLHOOKSTRUCT);
            // High word of mouseData: 1 = XBUTTON1 (Mouse 4, "back"), 2 = XBUTTON2 (Mouse 5).
            let code = if (info.mouseData >> 16) & 0xFFFF == 1 {
                crate::voice::ptt::MOUSE4
            } else {
                crate::voice::ptt::MOUSE5
            };
            if crate::voice::ptt::handle(code, msg == WM_XBUTTONDOWN) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}

unsafe extern "system" fn keyboard_hook_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    // Voice push-to-talk key (needs both press and release).
    if n_code >= 0 {
        let msg = w_param.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        if down || msg == WM_KEYUP || msg == WM_SYSKEYUP {
            let vk = (*(l_param.0 as *const KBDLLHOOKSTRUCT)).vkCode;
            if crate::voice::ptt::handle(vk, down) {
                return LRESULT(1);
            }
        }
    }

    if n_code >= 0 && (w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN) {
        let vk_code = (*(l_param.0 as *const KBDLLHOOKSTRUCT)).vkCode;
        // Hotkeys need their exact modifiers, so Alt+F4, Win+Left and friends still reach Windows.
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
        let held = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| GetAsyncKeyState(vk.0 as i32) < 0;
        let pressed = crate::hotkeys::Hotkey {
            vk: vk_code,
            ctrl: held(VK_CONTROL),
            alt: held(VK_MENU),
            shift: held(VK_SHIFT),
        };
        let action = if held(VK_LWIN) || held(VK_RWIN) { None } else { crate::hotkeys::action_for(pressed) };

        if let Some(action) = action.filter(|_| is_genshin_active()) {
            use crate::hotkeys::Action;
            let guard = APP_STATE_REF.read().unwrap();
            if let Some((state, handle)) = guard.as_ref() {
                let mut handled = false;
                let mut macro_state = state.0.lock().unwrap();

                match action {
                    Action::Dialogue if macro_state.auto_dialogue || macro_state.story_mode => {
                        macro_state.auto_dialogue_active = !macro_state.auto_dialogue_active;
                        handled = true;
                    }
                    Action::ArtifactsOne | Action::ArtifactsAll if macro_state.artifact_remover => {
                        if crate::artifacts::running() {
                            // Either artifact key while every character is being done: stop.
                            crate::artifacts::cancel();
                        } else if action == Action::ArtifactsAll {
                            handle.spawn(crate::artifacts::remove_all());
                        } else {
                            handle.spawn(async {
                                crate::macros::artifact_remover_run();
                            });
                        }
                        handled = true;
                    }
                    Action::PrevCharacter if macro_state.artifact_remover => {
                        handle.spawn(async {
                            crate::macros::artifact_remover_prev();
                        });
                        handled = true;
                    }
                    Action::NextCharacter if macro_state.artifact_remover => {
                        handle.spawn(async {
                            crate::macros::artifact_remover_next();
                        });
                        handled = true;
                    }
                    Action::AutoMessage if macro_state.auto_message => {
                        if let Some(last_time) = macro_state.last_toggle_time {
                            if last_time.elapsed().as_millis() < 300 {
                                return LRESULT(1); // Ignore auto-repeat
                            }
                        }
                        macro_state.last_toggle_time = Some(std::time::Instant::now());
                        macro_state.auto_message_active = !macro_state.auto_message_active;

                        if macro_state.auto_message_active {
                            let text = macro_state.auto_message_text.clone();
                            let count = macro_state.auto_message_count;
                            let state_clone = state.0.clone();
                            handle.spawn(async move {
                                crate::macros::auto_message_run(state_clone, text, count).await;
                            });
                        }
                        handled = true;
                    }
                    _ => {}
                }

                if handled {
                    return LRESULT(1); // Block the keypress
                }
            }
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}
