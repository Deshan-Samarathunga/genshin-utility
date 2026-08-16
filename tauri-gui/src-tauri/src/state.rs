use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct MacroState {
    pub auto_dialogue: bool,
    pub artifact_remover: bool,
    pub auto_message: bool,
    pub auto_message_text: String,
    pub auto_message_count: u32,
    
    // Internal runtime state
    pub auto_dialogue_active: bool,
    pub auto_message_active: bool,
    pub last_toggle_time: Option<std::time::Instant>,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Mutex<MacroState>>);

impl AppState {
    pub fn new() -> Self {
        AppState(Arc::new(Mutex::new(MacroState::default())))
    }
}
