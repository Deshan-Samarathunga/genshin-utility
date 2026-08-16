use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::ProcessStatus::GetProcessImageFileNameW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, SetWindowsHookExW,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
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

            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                // message loop to keep hook alive
            }

            let _ = UnhookWindowsHookEx(hook.unwrap());
        }
    });
}

fn is_genshin_active() -> bool {
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

unsafe extern "system" fn keyboard_hook_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 && (w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN) {
        let kb_struct = *(l_param.0 as *const KBDLLHOOKSTRUCT);
        let vk_code = kb_struct.vkCode;

        // F4 = 115, F7 = 118, F8 = 119
        // Left = 37, Right = 39

        if vk_code == 115 || vk_code == 118 || vk_code == 119 || vk_code == 37 || vk_code == 39 {
            let is_genshin = is_genshin_active();
            println!("Hotkey pressed: {}, Genshin active: {}", vk_code, is_genshin);
            
            if is_genshin {
                let guard = APP_STATE_REF.read().unwrap();
                if let Some((state, handle)) = guard.as_ref() {
                    let mut handled = false;

                    let mut macro_state = state.0.lock().unwrap();

                    if vk_code == 115 && macro_state.auto_dialogue {
                        println!("F4 triggered auto_dialogue");
                        macro_state.auto_dialogue_active = !macro_state.auto_dialogue_active;
                        handled = true;
                    } else if vk_code == 118 && macro_state.artifact_remover {
                        println!("F7 triggered artifact_remover");
                        handle.spawn(async {
                            crate::macros::artifact_remover_run();
                        });
                        handled = true;
                    } else if vk_code == 37 && macro_state.artifact_remover {
                        println!("Left triggered artifact_remover_prev");
                        handle.spawn(async {
                            crate::macros::artifact_remover_prev();
                        });
                        handled = true;
                    } else if vk_code == 39 && macro_state.artifact_remover {
                        println!("Right triggered artifact_remover_next");
                        handle.spawn(async {
                            crate::macros::artifact_remover_next();
                        });
                        handled = true;
                    } else if vk_code == 119 && macro_state.auto_message {
                        if let Some(last_time) = macro_state.last_toggle_time {
                            if last_time.elapsed().as_millis() < 300 {
                                return LRESULT(1); // Ignore auto-repeat
                            }
                        }
                        macro_state.last_toggle_time = Some(std::time::Instant::now());
                        
                        println!("F8 triggered auto_message");
                        
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

                    if handled {
                        return LRESULT(1); // Block the keypress
                    }
                }
            }
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}
