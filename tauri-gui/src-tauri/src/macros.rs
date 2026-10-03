use enigo::{Enigo, Keyboard, Mouse, Button, Key, Settings, Direction};
use arboard::Clipboard;
use std::time::Duration;
use tokio::time::sleep;

use windows::Win32::UI::WindowsAndMessaging::{SetCursorPos, GetCursorPos};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    mouse_event, keybd_event, 
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL,
    KEYEVENTF_KEYUP, VK_BACK, VK_CONTROL, VK_V, KEYBD_EVENT_FLAGS, VIRTUAL_KEY
};
use windows::Win32::Foundation::POINT;

pub fn click_at(x: i32, y: i32) {
    unsafe {
        SetCursorPos(x, y);
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        std::thread::sleep(Duration::from_millis(20));
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }
}

pub fn click_sequence(points: &[(i32, i32)], delay_ms: u64) {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        let ox = if GetCursorPos(&mut pt).is_ok() { pt.x } else { 0 };
        let oy = if pt.y != 0 { pt.y } else { 0 };

        for &(x, y) in points {
            click_at(x, y);
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        
        SetCursorPos(ox, oy);
    }
}

/// Pastes `text` into the focused input via the clipboard (Ctrl+V), restoring the previous clipboard text.
pub fn paste_text(text: &str) {
    if let Ok(mut clipboard) = Clipboard::new() {
        let saved_clip = clipboard.get_text().unwrap_or_default();
        if clipboard.set_text(text.to_string()).is_err() {
            return;
        }

        unsafe {
            keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0); // V
            std::thread::sleep(Duration::from_millis(50));
            keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        std::thread::sleep(Duration::from_millis(500));

        let _ = clipboard.set_text(saved_clip);
    }
}

pub fn press_key(vk: VIRTUAL_KEY) {
    unsafe {
        keybd_event(vk.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        std::thread::sleep(Duration::from_millis(25));
        keybd_event(vk.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

pub fn backspace(count: usize) {
    for _ in 0..count {
        press_key(VK_BACK);
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub async fn loot_loop_step() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    let _ = enigo.key(Key::Unicode('f'), Direction::Click);
    sleep(Duration::from_millis(50)).await;
    let _ = enigo.key(Key::Space, Direction::Click);
}

pub fn artifact_remover_run() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    
    let artifacts = [
        (1335, 735), (967, 787), (682, 780), (397, 638),
        (555, 558), (722, 771), (804, 784), (1222, 765),
    ];
    let panel_items = [
        (89, 43), (195, 41), (309, 40), (415, 37), (525, 44),
    ];
    let remove_btn = (1558, 1002);
    let back_btn = (1838, 35);
    let next_btn = (1840, 536);

    // Step 1: Click all artifact coordinates
    click_sequence(&artifacts, 80);
    std::thread::sleep(Duration::from_millis(200));

    // Step 2: Click each panel item then remove
    for &(px, py) in &panel_items {
        click_at(px, py);
        std::thread::sleep(Duration::from_millis(200));
        click_at(remove_btn.0, remove_btn.1);
        std::thread::sleep(Duration::from_millis(200));
    }

    // Step 3: Back button
    std::thread::sleep(Duration::from_millis(500));
    click_at(back_btn.0, back_btn.1);
    std::thread::sleep(Duration::from_millis(1000));

    // Step 4: Next button
    click_at(next_btn.0, next_btn.1);
}

pub fn artifact_remover_prev() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    click_at(68, 535);
}

pub fn artifact_remover_next() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    click_at(1840, 536);
}

pub async fn auto_message_run(state: std::sync::Arc<std::sync::Mutex<crate::state::MacroState>>, text: String, count: u32) {
    let _enigo = Enigo::new(&Settings::default()).unwrap();
    
    let loop_count = if count == 0 { 1 } else { count };
    let chat_y_coords = [167, 290, 416, 542, 668, 792, 917];
    
    // 1 page = 7 friends. 
    // Through testing, 49 notches is slightly too much (drifts up), 48 is too little.
    // The exact scroll distance is roughly 48.6 notches per page.
    // We will accumulate the exact fractional amount to prevent long-term drift.
    let mut accumulated_scroll: f32 = 0.0;
    let exact_notches_per_page = 48.61;
    
    for i in 0..loop_count {
        {
            let guard = state.lock().unwrap();
            if !guard.auto_message_active {
                break;
            }
        }

        let page_index = (i % 7) as usize;
        let y_coord = chat_y_coords[page_index];

        // 1. Click chat icon on the current row
        click_at(1721, y_coord);
        sleep(Duration::from_millis(500)).await;
        
        // 2. Select chat box
        click_at(441, 1001);
        sleep(Duration::from_millis(500)).await;
        
        // 3. Paste the text
        paste_text(&text);
        
        // 4. Send button
        click_at(1048, 1008);
        sleep(Duration::from_millis(500)).await;
        
        // 5. Chat close button (back to friend list)
        click_at(40, 42);
        sleep(Duration::from_millis(500)).await;
        
        // 6. Scroll down a full page (7 friends) ONLY after the 7th friend
        if page_index == 6 && i != loop_count - 1 {
            unsafe { SetCursorPos(200, 500); }
            sleep(Duration::from_millis(100)).await;
            
            accumulated_scroll += exact_notches_per_page;
            let notches_to_scroll = accumulated_scroll.round() as u32;
            accumulated_scroll -= notches_to_scroll as f32;
            
            unsafe {
                for _ in 0..notches_to_scroll {
                    mouse_event(MOUSEEVENTF_WHEEL, 0, 0, -120, 0);
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
    }
    
    // Auto turn off when done
    if let Ok(mut guard) = state.lock() {
        guard.auto_message_active = false;
    }
}
