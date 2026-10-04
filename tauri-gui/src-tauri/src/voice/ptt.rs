//! Keyboard / mouse push-to-talk. The global hooks in `hook.rs` report every press of the configured
//! key here; while Voice Chat is on and Genshin is in front, the press is consumed (the game never sees
//! it) and forwarded to the voice orchestrator as a down/up pair.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEvent {
    Down,
    Up,
}

/// Mouse side buttons share the code space with virtual-key codes, offset past the VK range.
pub const MOUSE4: u32 = 0x1_0001;
pub const MOUSE5: u32 = 0x1_0002;
/// Setting value -> trigger code (0 = off).
pub const KEYS: &[(&str, u32)] = &[
    ("off", 0),
    ("mouse4", MOUSE4),
    ("mouse5", MOUSE5),
    ("f9", 0x78),
    ("f10", 0x79),
    ("f11", 0x7A),
    ("f12", 0x7B),
];

static KEY: AtomicU32 = AtomicU32::new(0);
static HELD: AtomicBool = AtomicBool::new(false);
static SENDER: OnceLock<UnboundedSender<PttEvent>> = OnceLock::new();

pub fn set_sender(tx: UnboundedSender<PttEvent>) {
    let _ = SENDER.set(tx);
}

pub fn code_for(key: &str) -> u32 {
    KEYS.iter().find(|(name, _)| name.eq_ignore_ascii_case(key)).map_or(0, |(_, code)| *code)
}

/// Sets the trigger; `active` is false while Voice Chat is off (the key then works normally).
pub fn configure(key: &str, active: bool) {
    let code = if active { code_for(key) } else { 0 };
    if KEY.swap(code, Ordering::Relaxed) != code && HELD.swap(false, Ordering::Relaxed) {
        send(PttEvent::Up);
    }
}

fn send(event: PttEvent) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(event);
    }
}

/// Called from the low-level hooks. Returns true when the event was consumed.
pub fn handle(code: u32, down: bool) -> bool {
    let key = KEY.load(Ordering::Relaxed);
    if key == 0 || code != key {
        return false;
    }
    if down {
        if HELD.load(Ordering::Relaxed) {
            return true; // keyboard auto-repeat while held
        }
        if !crate::hook::is_genshin_active() {
            return false;
        }
        HELD.store(true, Ordering::Relaxed);
        send(PttEvent::Down);
        true
    } else if HELD.swap(false, Ordering::Relaxed) {
        // Swallow the release too, even if focus moved, so the game never sees half a press.
        send(PttEvent::Up);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_setting_names() {
        assert_eq!(code_for("mouse4"), MOUSE4);
        assert_eq!(code_for("F9"), 0x78);
        assert_eq!(code_for("off"), 0);
        assert_eq!(code_for("unknown"), 0);
    }
}
