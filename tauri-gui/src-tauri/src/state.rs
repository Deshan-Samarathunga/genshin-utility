use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct MacroState {
    pub auto_dialogue: bool,
    /// Auto Dialogue with AI choices and a summary. Shares F4 and `auto_dialogue_active`; only one
    /// of the two modes is on at a time.
    pub story_mode: bool,
    pub artifact_remover: bool,
    pub auto_message: bool,
    pub voice_chat: bool,
    pub auto_message_text: String,
    pub auto_message_count: u32,
    
    // Internal runtime state
    pub auto_dialogue_active: bool,
    pub auto_message_active: bool,
    pub last_toggle_time: Option<std::time::Instant>,
    pub auto_dialogue_speed: u64,
    pub story_speed: u64,
}

impl MacroState {
    pub fn new() -> Self {
        Self {
            auto_dialogue: false,
            story_mode: false,
            artifact_remover: false,
            auto_message: false,
            voice_chat: false,
            auto_message_text: String::new(),
            auto_message_count: 0,
            auto_dialogue_active: false,
            auto_message_active: false,
            last_toggle_time: None,
            auto_dialogue_speed: 1000,
            story_speed: 1000,
        }
    }
}

#[derive(Clone)]
pub struct AppState(pub Arc<Mutex<MacroState>>);

impl AppState {
    pub fn new() -> Self {
        AppState(Arc::new(Mutex::new(MacroState::new())))
    }
}
