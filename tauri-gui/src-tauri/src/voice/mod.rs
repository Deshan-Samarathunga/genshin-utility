//! Voice Chat (PS5): hold the DualSense mic button, speak, and the transcript is typed into
//! Genshin's open chat box. Tap to send, double-tap to discard, hold to redo.

pub mod audio;
pub mod controller;
pub mod gesture;
pub mod settings;
pub mod setup;
pub mod stt;

use controller::PadEvent;
use gesture::{Action, Gesture, Input, State};
use serde::Serialize;
use settings::VoiceSettings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::unbounded_channel;

use crate::state::AppState;

// Chat panel coordinates at 1080p (same layout Auto Message uses).
const CHAT_INPUT: (i32, i32) = (441, 1001);
const CHAT_SEND: (i32, i32) = (1048, 1008);
/// Recordings quieter than this are treated as silence and never sent to the engine.
const SILENCE_RMS: f32 = 0.003;

pub struct VoiceState {
    settings: Mutex<VoiceSettings>,
    settings_path: PathBuf,
    /// Root for downloaded engines and models.
    pub data_root: PathBuf,
    pub client: reqwest::Client,
    server: tokio::sync::Mutex<Option<stt::LocalServer>>,
    /// Set when settings change so the orchestrator reopens the mic.
    reload: AtomicBool,
    pub downloading: AtomicBool,
}

pub type VoiceHandle = Arc<VoiceState>;

impl VoiceState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        let settings_path = config_dir.join("voice.json");
        Self {
            settings: Mutex::new(VoiceSettings::load(&settings_path)),
            settings_path,
            data_root: data_dir.join("whisper"),
            client: reqwest::Client::new(),
            server: tokio::sync::Mutex::new(None),
            reload: AtomicBool::new(false),
            downloading: AtomicBool::new(false),
        }
    }

    pub fn settings(&self) -> VoiceSettings {
        self.settings.lock().unwrap().clone()
    }

    pub fn save_settings(&self, mut new: VoiceSettings) -> Result<(), String> {
        new.normalize();
        new.save(&self.settings_path)?;
        *self.settings.lock().unwrap() = new;
        self.reload.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Starts (or reuses) whisper-server for the current settings and returns its port.
    async fn ensure_server(&self, s: &VoiceSettings) -> Result<u16, String> {
        let exe = setup::find_server_exe(&setup::engine_dir(&self.data_root, s.local_gpu))
            .ok_or("Local engine not downloaded yet (Voice Chat card → Download engine)")?;
        let model = stt::model_path(&setup::models_dir(&self.data_root), &s.local_model);
        if !model.exists() {
            return Err(format!("Model {} not downloaded yet", s.local_model));
        }
        let key = stt::server_key(&exe, &model, s);
        let mut guard = self.server.lock().await;
        if let Some(server) = guard.as_mut() {
            if server.is_for(&key) {
                return Ok(server.port);
            }
        }
        *guard = None; // dropping kills the old process
        let server = stt::spawn_local_server(&self.client, &exe, &model, s).await?;
        let port = server.port;
        *guard = Some(server);
        Ok(port)
    }

    /// After warm-up: warn if the GPU was requested but whisper-server fell back to CPU.
    fn gpu_warning(&self, s: &VoiceSettings) -> Option<String> {
        if !s.local_gpu {
            return None;
        }
        let exe = setup::find_server_exe(&setup::engine_dir(&self.data_root, true))?;
        match stt::gpu_in_use(&exe) {
            Some(false) => Some(format!(
                "GPU not in use — engine fell back to CPU (slow). Log: {}",
                stt::log_path(&exe).display()
            )),
            _ => None,
        }
    }

    pub async fn stop_server(&self) {
        *self.server.lock().await = None;
    }

    /// Synchronous variant for app shutdown.
    pub fn kill_server_now(&self) {
        if let Ok(mut guard) = self.server.try_lock() {
            guard.take();
        }
    }

    async fn transcribe(&self, samples: Vec<f32>, rate: u32) -> Result<String, String> {
        let s = self.settings();
        let mut samples = audio::resample_to_16k(&samples, rate);
        if samples.len() < (audio::WHISPER_RATE as usize) / 4 || audio::rms(&samples) < SILENCE_RMS {
            return Ok(String::new());
        }
        audio::normalize(&mut samples);
        let wav = audio::wav_bytes(&samples)?;
        let port = if s.is_local() { Some(self.ensure_server(&s).await?) } else { None };
        let raw = stt::transcribe(&self.client, &s, port, wav).await?;
        Ok(stt::clean_transcript(&raw, &s.replacements))
    }
}

#[derive(Serialize, Clone)]
struct Status {
    /// disabled | idle | listening | transcribing | draft | sent | discarded | info | error | loading
    state: &'static str,
    text: String,
    message: String,
}

fn emit(app: &AppHandle, state: &'static str, text: &str, message: &str) {
    let _ = app.emit(
        "voice-status",
        Status { state, text: text.to_string(), message: message.to_string() },
    );
}

enum InjectOp {
    Paste(String),
    Send,
    Backspace(usize),
}

/// Keystrokes/clicks run on one thread so paste, send and discard can never interleave.
fn spawn_injector() -> mpsc::Sender<InjectOp> {
    let (tx, rx) = mpsc::channel::<InjectOp>();
    std::thread::spawn(move || {
        for op in rx {
            if !crate::hook::is_genshin_active() {
                continue;
            }
            match op {
                InjectOp::Paste(text) => {
                    crate::macros::click_sequence(&[CHAT_INPUT], 150);
                    crate::macros::paste_text(&text);
                }
                InjectOp::Send => crate::macros::click_sequence(&[CHAT_SEND], 100),
                InjectOp::Backspace(n) => {
                    // Re-focus the input first so the deletes can't land anywhere else.
                    crate::macros::click_sequence(&[CHAT_INPUT], 100);
                    crate::macros::backspace(n);
                }
            }
        }
    });
    tx
}

fn open_recorder(app: &AppHandle, s: &VoiceSettings) -> Result<audio::Recorder, String> {
    let r = audio::Recorder::start(&s.mic_name).map_err(|e| format!("Microphone: {e}"))?;
    let wanted = s.mic_name.trim().to_lowercase();
    let message = if !wanted.is_empty() && !r.device_name.to_lowercase().contains(&wanted) {
        format!("“{}” not found — using {}", s.mic_name, r.device_name)
    } else {
        format!("Mic: {}", r.device_name)
    };
    emit(app, "idle", "", &message);
    Ok(r)
}

/// Starts the controller reader and the orchestrator loop.
pub fn start(app: AppHandle, macro_state: AppState, voice: VoiceHandle) {
    let enabled = Arc::new(AtomicBool::new(false));
    let (pad_tx, mut pad_rx) = unbounded_channel::<PadEvent>();
    controller::spawn(enabled.clone(), pad_tx);
    let injector = spawn_injector();
    // (transcript, seconds it took)
    let (done_tx, mut done_rx) = unbounded_channel::<Result<(String, f32), String>>();

    tauri::async_runtime::spawn(async move {
        let epoch = Instant::now();
        let mut gesture = Gesture::default();
        let mut recorder: Option<audio::Recorder> = None;
        let mut last_mic_attempt: Option<Instant> = None;
        let mut last_mic_error = String::new();
        let mut opened_mic = String::new();
        let mut draft = String::new();
        let mut tick = tokio::time::interval(Duration::from_millis(25));

        loop {
            let input = tokio::select! {
                _ = tick.tick() => Some(Input::Tick),
                Some(event) = pad_rx.recv() => match event {
                    PadEvent::MicDown => Some(Input::Down),
                    PadEvent::MicUp => Some(Input::Up),
                    PadEvent::Connected(connected) => {
                        if connected {
                            // The controller's mic shows up as an audio device a moment after the HID
                            // device; reopen the mic ~2 s from now to pick it up.
                            recorder = None;
                            last_mic_attempt = Instant::now().checked_sub(Duration::from_secs(3));
                            last_mic_error.clear();
                            emit(&app, "idle", "", "Controller connected");
                        } else {
                            emit(&app, "error", "", "Controller disconnected");
                        }
                        None
                    }
                },
                Some(result) = done_rx.recv() => {
                    if gesture.state == State::Transcribing {
                        match result {
                            Ok((text, _)) if text.is_empty() => {
                                gesture.transcribe_done(false);
                                emit(&app, "info", "", "Didn't catch that — hold the mic button and try again");
                            }
                            Ok((text, secs)) if crate::hook::is_genshin_active() => {
                                let _ = injector.send(InjectOp::Paste(text.clone()));
                                draft = text;
                                gesture.transcribe_done(true);
                                let message = format!("{secs:.1}s · Tap mic to send · double-tap to discard · hold to redo");
                                emit(&app, "draft", &draft, &message);
                            }
                            Ok((text, secs)) => {
                                gesture.transcribe_done(false);
                                emit(&app, "info", &text, &format!("{secs:.1}s · Genshin isn't focused — not typed"));
                            }
                            Err(e) => {
                                gesture.transcribe_done(false);
                                emit(&app, "error", "", &e);
                            }
                        }
                    }
                    None
                }
            };

            let want = macro_state.0.lock().unwrap().voice_chat;
            if want != enabled.load(Ordering::Relaxed) {
                enabled.store(want, Ordering::Relaxed);
                gesture.reset();
                draft.clear();
                set_overlay_visible(&app, want);
                if want {
                    last_mic_attempt = None;
                    let s = voice.settings();
                    if s.is_local() {
                        // Warm up whisper-server so the first message isn't slow.
                        let voice = voice.clone();
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            emit(&app, "loading", "", "Starting local speech engine…");
                            match voice.ensure_server(&s).await {
                                Ok(_) => match voice.gpu_warning(&s) {
                                    Some(warning) => emit(&app, "error", "", &warning),
                                    None => emit(&app, "idle", "", "Speech engine ready — hold the mic button to talk"),
                                },
                                Err(e) => emit(&app, "error", "", &e),
                            }
                        });
                    }
                } else {
                    recorder = None;
                    // May wait for an in-progress warm-up, so don't block the loop on it.
                    let voice = voice.clone();
                    tauri::async_runtime::spawn(async move { voice.stop_server().await });
                    emit(&app, "disabled", "", "");
                }
            }
            if !want {
                continue;
            }

            if voice.reload.swap(false, Ordering::Relaxed) && voice.settings().mic_name != opened_mic {
                recorder = None;
                last_mic_attempt = None;
                last_mic_error.clear();
            }
            if recorder.is_none()
                && gesture.state == State::Idle
                && last_mic_attempt.is_none_or(|t| t.elapsed() > Duration::from_secs(5))
            {
                last_mic_attempt = Some(Instant::now());
                let s = voice.settings();
                match open_recorder(&app, &s) {
                    Ok(r) => {
                        recorder = Some(r);
                        opened_mic = s.mic_name;
                        last_mic_error.clear();
                    }
                    // Retries every few seconds; only report a new problem once.
                    Err(e) if e != last_mic_error => {
                        emit(&app, "error", "", &e);
                        last_mic_error = e;
                    }
                    Err(_) => {}
                }
            }

            let Some(input) = input else { continue };
            let now = epoch.elapsed().as_millis() as u64;
            for action in gesture.step(input, now) {
                match action {
                    Action::StartRecording => {
                        if let Some(r) = &recorder {
                            r.begin();
                        }
                        // From a draft this might still turn out to be a tap, so stay quiet until it's a hold.
                        if matches!(gesture.state, State::Recording { .. }) {
                            emit(&app, "listening", "", "Listening… release to finish");
                        }
                    }
                    Action::CancelRecording => {
                        if let Some(r) = &recorder {
                            r.end();
                        }
                        if gesture.state == State::Idle {
                            emit(&app, "idle", "", "");
                        }
                    }
                    Action::StopAndTranscribe => {
                        emit(&app, "transcribing", "", "Transcribing…");
                        let Some(r) = &recorder else {
                            let _ = done_tx.send(Err("Microphone not available".into()));
                            continue;
                        };
                        let (samples, rate) = r.end();
                        let voice = voice.clone();
                        let done_tx = done_tx.clone();
                        tauri::async_runtime::spawn(async move {
                            let started = Instant::now();
                            let result = voice.transcribe(samples, rate).await;
                            let _ = done_tx.send(result.map(|text| (text, started.elapsed().as_secs_f32())));
                        });
                    }
                    Action::Send => {
                        let _ = injector.send(InjectOp::Send);
                        emit(&app, "sent", &draft, "Sent");
                        draft.clear();
                    }
                    Action::Discard => {
                        let _ = injector.send(InjectOp::Backspace(draft.chars().count() + 2));
                        if matches!(gesture.state, State::Recording { .. }) {
                            emit(&app, "listening", "", "Discarded — listening for a new take…");
                        } else {
                            emit(&app, "discarded", "", "Discarded");
                        }
                        draft.clear();
                    }
                }
            }
        }
    });
}

/// Shows/hides the overlay without ever activating it, so the game keeps keyboard focus.
fn set_overlay_visible(app: &AppHandle, visible: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};
    let Some(hwnd) = app.get_webview_window("overlay").and_then(|w| w.hwnd().ok()) else {
        return;
    };
    unsafe {
        let _ = ShowWindow(HWND(hwnd.0), if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

/// Creates the click-through, non-focusable overlay shown at the top of the screen (hidden until enabled).
pub fn create_overlay(app: &AppHandle) -> tauri::Result<()> {
    let (width, height) = (720.0, 160.0);
    let window = tauri::WebviewWindowBuilder::new(app, "overlay", tauri::WebviewUrl::App("overlay.html".into()))
        .title("Voice Chat Overlay")
        .inner_size(width, height)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
    if let Some(monitor) = window.primary_monitor()? {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let x = (size.width as f64 - width * scale) / 2.0;
        let _ = window.set_position(tauri::PhysicalPosition::new(x.max(0.0) as i32, (24.0 * scale) as i32));
    }
    window.set_ignore_cursor_events(true)?;
    Ok(())
}
