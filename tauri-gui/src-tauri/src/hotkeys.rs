//! In-game hotkeys, assignable in Settings. A binding is a key plus the exact modifiers it needs, so
//! F4 alone starts Story Mode while Alt+F4 still reaches Windows and closes the game.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    /// Windows virtual-key code.
    pub vk: u32,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Start/stop Auto Dialogue or Story Mode (whichever is on).
    Dialogue,
    ArtifactsOne,
    ArtifactsAll,
    PrevCharacter,
    NextCharacter,
    AutoMessage,
}

const fn key(vk: u32) -> Hotkey {
    Hotkey { vk, ctrl: false, alt: false, shift: false }
}

/// Setting name, action, default binding.
pub const ACTIONS: &[(&str, Action, Hotkey)] = &[
    ("dialogue", Action::Dialogue, key(0x73)),          // F4
    ("artifacts_one", Action::ArtifactsOne, key(0x76)), // F7
    ("artifacts_all", Action::ArtifactsAll, Hotkey { vk: 0x76, ctrl: false, alt: false, shift: true }),
    ("prev_character", Action::PrevCharacter, key(0x25)), // Left
    ("next_character", Action::NextCharacter, key(0x27)), // Right
    ("auto_message", Action::AutoMessage, key(0x77)),     // F8
];

lazy_static::lazy_static! {
    static ref BINDINGS: RwLock<Vec<(Action, Hotkey)>> =
        RwLock::new(ACTIONS.iter().map(|(_, action, hotkey)| (*action, *hotkey)).collect());
}

/// The action bound to exactly this key and modifier combination.
pub fn action_for(pressed: Hotkey) -> Option<Action> {
    BINDINGS.read().unwrap().iter().find(|(_, hotkey)| *hotkey == pressed).map(|(action, _)| *action)
}

/// Checks a set of bindings: every key set and no two actions on the same combination.
fn resolve(bindings: &HashMap<String, Hotkey>) -> Result<Vec<(Action, Hotkey)>, String> {
    let mut resolved: Vec<(&str, Action, Hotkey)> = Vec::new();
    for (name, action, default) in ACTIONS {
        let hotkey = bindings.get(*name).copied().unwrap_or(*default);
        if hotkey.vk == 0 || hotkey.vk > 0xFE {
            return Err(format!("No key set for {name}"));
        }
        if let Some((other, ..)) = resolved.iter().find(|(_, _, h)| *h == hotkey) {
            return Err(format!("{name} and {other} use the same keys"));
        }
        resolved.push((name, *action, hotkey));
    }
    Ok(resolved.into_iter().map(|(_, action, hotkey)| (action, hotkey)).collect())
}

#[tauri::command]
pub fn set_hotkeys(bindings: HashMap<String, Hotkey>) -> Result<(), String> {
    let resolved = resolve(&bindings)?;
    *BINDINGS.write().unwrap() = resolved;
    Ok(())
}

#[tauri::command]
pub fn default_hotkeys() -> HashMap<String, Hotkey> {
    ACTIONS.iter().map(|(name, _, hotkey)| (name.to_string(), *hotkey)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_must_match_exactly() {
        let f4 = key(0x73);
        let alt_f4 = Hotkey { alt: true, ..f4 };
        let resolved = resolve(&HashMap::new()).unwrap();
        let find = |pressed: Hotkey| resolved.iter().find(|(_, h)| *h == pressed).map(|(a, _)| *a);
        assert_eq!(find(f4), Some(Action::Dialogue));
        assert_eq!(find(alt_f4), None, "Alt+F4 is left to Windows");
        assert_eq!(find(Hotkey { shift: true, ..key(0x76) }), Some(Action::ArtifactsAll));
    }

    #[test]
    fn rejects_clashes_and_fills_defaults() {
        let mut bindings = HashMap::new();
        bindings.insert("dialogue".to_string(), key(0x76)); // F7, already artifacts_one
        assert!(resolve(&bindings).unwrap_err().contains("same keys"));
        bindings.insert("artifacts_one".to_string(), key(0x75)); // F6
        let resolved = resolve(&bindings).unwrap();
        assert_eq!(resolved.len(), ACTIONS.len());
        assert!(resolved.contains(&(Action::Dialogue, key(0x76))));
    }
}
