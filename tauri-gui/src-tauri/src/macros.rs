use enigo::{Enigo, Keyboard, Mouse, Button, Key, Settings, Direction};
use arboard::Clipboard;
use std::time::Duration;
use tokio::time::sleep;

use windows::Win32::UI::WindowsAndMessaging::{SetCursorPos, GetCursorPos};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    mouse_event, keybd_event, 
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL,
    KEYEVENTF_KEYUP, VK_CONTROL, VK_V, KEYBD_EVENT_FLAGS
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
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    
    let loop_count = if count == 0 { 1 } else { count };
    
    for _ in 0..loop_count {
        {
            let guard = state.lock().unwrap();
            if !guard.auto_message_active {
                break;
            }
        }

        // 1. Click chat icon upper right
        click_at(1726, 166);
        sleep(Duration::from_millis(500)).await;
        
        // 2. Select chat
        click_at(441, 1001);
        sleep(Duration::from_millis(500)).await;
        
        // 3. Paste the text
        if let Ok(mut clipboard) = Clipboard::new() {
            let saved_clip = clipboard.get_text().unwrap_or_default();
            let _ = clipboard.set_text(&text);
            sleep(Duration::from_millis(100)).await;
            // Send Ctrl+V using native Windows API
            unsafe {
                keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                std::thread::sleep(Duration::from_millis(50));
                keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
                keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            }
            
            sleep(Duration::from_millis(200)).await;
            let _ = clipboard.set_text(saved_clip);
        }
        
        // 4. Send button
        click_at(1048, 1008);
        sleep(Duration::from_millis(500)).await;
        
        // 5. Chat close button
        click_at(40, 42);
        sleep(Duration::from_millis(500)).await;
        
        // 6. Scroll to next chat
        unsafe { SetCursorPos(200, 500); }
        sleep(Duration::from_millis(100)).await;
        
        // WheelDown 7
        unsafe {
            for _ in 0..7 {
                mouse_event(MOUSEEVENTF_WHEEL, 0, 0, -120, 0);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        sleep(Duration::from_millis(500)).await;
    }
    
    // Auto turn off when done
    if let Ok(mut guard) = state.lock() {
        guard.auto_message_active = false;
    }
}
