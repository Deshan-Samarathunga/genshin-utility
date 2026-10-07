//! Speech-to-text. Both engines speak multipart HTTP: a local whisper.cpp `whisper-server` child
//! process, or any OpenAI-compatible `/audio/transcriptions` endpoint (Groq, OpenAI).

use super::settings::{CloudConfig, VoiceSettings};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Phrases Whisper tends to invent on silence/noise.
const HALLUCINATIONS: &[&str] = &[
    "thank you.",
    "thank you",
    "thanks for watching!",
    "thanks for watching.",
    "you",
    "bye.",
    "[blank_audio]",
    "[music]",
    "(music)",
    "[silence]",
    "...",
];

pub struct LocalServer {
    child: Child,
    pub port: u16,
    key: String,
}

impl LocalServer {
    pub fn is_for(&mut self, key: &str) -> bool {
        self.key == key && matches!(self.child.try_wait(), Ok(None))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.kill();
    }
}

pub fn log_path(exe: &Path) -> PathBuf {
    exe.parent().unwrap_or(Path::new(".")).join("server.log")
}

/// Whether whisper-server reported running on CUDA, from its startup log (None = unknown).
pub fn gpu_in_use(exe: &Path) -> Option<bool> {
    let log = std::fs::read_to_string(log_path(exe)).ok()?;
    if log.contains("using CUDA") {
        Some(true)
    } else if log.contains("no GPU found") || log.contains("use gpu    = 0") {
        Some(false)
    } else {
        None
    }
}

pub fn server_key(exe: &Path, model: &Path, s: &VoiceSettings) -> String {
    format!("{}|{}|{}", exe.display(), model.display(), s.local_gpu)
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(listener.local_addr().map_err(|e| e.to_string())?.port())
}

/// Starts whisper-server with the model preloaded and waits until it answers HTTP.
pub async fn spawn_local_server(
    client: &reqwest::Client,
    exe: &Path,
    model: &Path,
    s: &VoiceSettings,
) -> Result<LocalServer, String> {
    let port = free_port()?;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let mut cmd = Command::new(exe);
    cmd.arg("-m")
        .arg(model)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["-t", &threads.to_string()])
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(log_path(exe)).map(Stdio::from).unwrap_or_else(|_| Stdio::null()))
        .creation_flags(CREATE_NO_WINDOW);
    if !s.local_gpu {
        cmd.arg("-ng");
    }
    let child = cmd.spawn().map_err(|e| format!("Couldn't start whisper-server: {e}"))?;
    let mut server = LocalServer { child, port, key: server_key(exe, model, s) };

    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Ok(Some(status)) = server.child.try_wait() {
            return Err(format!(
                "whisper-server exited ({status}). {}",
                if s.local_gpu { "Try turning GPU off." } else { "Is the model file valid?" }
            ));
        }
        if client.get(format!("http://127.0.0.1:{port}/")).send().await.is_ok() {
            return Ok(server);
        }
        if Instant::now() > deadline {
            return Err("whisper-server didn't start in time".into());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

fn text_part(form: reqwest::multipart::Form, name: &'static str, value: &str) -> reqwest::multipart::Form {
    if value.is_empty() {
        form
    } else {
        form.text(name, value.to_string())
    }
}

fn wav_part(wav: Vec<u8>) -> Result<reqwest::multipart::Part, String> {
    reqwest::multipart::Part::bytes(wav)
        .file_name("speech.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())
}

/// Sends a request and returns the JSON body, turning HTTP errors into readable messages.
pub(crate) async fn send_json(request: reqwest::RequestBuilder, who: &str) -> Result<serde_json::Value, String> {
    let response = request
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("{who}: request failed: {e}"))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let hint = match status.as_u16() {
            401 | 403 => " (check the API key)",
            429 => " (rate limit — wait a bit or switch provider)",
            _ => "",
        };
        return Err(format!("{who} returned {status}{hint}: {}", body.chars().take(200).collect::<String>()));
    }
    serde_json::from_str(&body).map_err(|_| format!("{who}: unexpected response: {body}"))
}

fn base(config: &CloudConfig) -> String {
    config.base_url.trim().trim_end_matches('/').to_string()
}

/// Sends a WAV to the selected engine and returns the raw transcript.
pub async fn transcribe(
    client: &reqwest::Client,
    s: &VoiceSettings,
    local_port: Option<u16>,
    wav: Vec<u8>,
) -> Result<String, String> {
    if s.is_local() {
        let port = local_port.ok_or("Local engine isn't running")?;
        let mut form = reqwest::multipart::Form::new()
            .part("file", wav_part(wav)?)
            .text("response_format", "json")
            .text("temperature", "0")
            // Greedy decoding at temperature 0: several times faster than beam search, same text in practice.
            .text("no_timestamps", "true")
            .text("suppress_nst", "true");
        form = text_part(form, "language", &s.language);
        form = text_part(form, "prompt", &s.prompt());
        let request = client.post(format!("http://127.0.0.1:{port}/inference")).multipart(form);
        let json = send_json(request, "Local engine").await?;
        return Ok(json["text"].as_str().unwrap_or_default().to_string());
    }

    let config = s.provider();
    let key = config.api_key.trim();
    if key.is_empty() {
        return Err(format!("Add your {} API key in Voice Chat settings", provider_name(&s.engine)));
    }
    // Language codes are ISO-639-1; "auto" means let the provider detect it.
    let language = if s.language == "auto" { "" } else { s.language.as_str() };
    let words = s.vocab_words();

    match s.engine.as_str() {
        "deepgram" => {
            let mut query: Vec<(&str, &str)> = vec![
                ("model", config.model.trim()),
                ("smart_format", "true"),
                ("punctuate", "true"),
            ];
            if language.is_empty() {
                query.push(("detect_language", "true"));
            } else {
                query.push(("language", language));
            }
            // Keyterm prompting is Nova-3 only.
            if config.model.starts_with("nova-3") {
                query.extend(words.iter().rev().take(100).map(|w| ("keyterm", *w)));
            }
            let request = client
                .post(format!("{}/listen", base(&config)))
                .query(&query)
                .header("Authorization", format!("Token {key}"))
                .header("Content-Type", "audio/wav")
                .body(wav);
            let json = send_json(request, "Deepgram").await?;
            Ok(json["results"]["channels"][0]["alternatives"][0]["transcript"]
                .as_str()
                .unwrap_or_default()
                .to_string())
        }
        "elevenlabs" => {
            let mut form = reqwest::multipart::Form::new()
                .part("file", wav_part(wav)?)
                .text("model_id", config.model.trim().to_string())
                .text("tag_audio_events", "false");
            form = text_part(form, "language_code", language);
            let request = client
                .post(format!("{}/speech-to-text", base(&config)))
                .header("xi-api-key", key)
                .multipart(form);
            let json = send_json(request, "ElevenLabs").await?;
            Ok(json["text"].as_str().unwrap_or_default().to_string())
        }
        "mistral" => {
            let mut form = reqwest::multipart::Form::new()
                .part("file", wav_part(wav)?)
                .text("model", config.model.trim().to_string());
            form = text_part(form, "language", language);
            // context_bias is an array field: repeat it, up to 100 terms.
            for word in words.iter().rev().take(100) {
                form = form.text("context_bias", word.to_string());
            }
            let request = client
                .post(format!("{}/audio/transcriptions", base(&config)))
                .bearer_auth(key)
                .multipart(form);
            let json = send_json(request, "Mistral").await?;
            Ok(json["text"].as_str().unwrap_or_default().to_string())
        }
        "gemini" => {
            use base64::Engine as _;
            let language_hint = if language.is_empty() {
                String::new()
            } else {
                format!(" The language is '{language}'.")
            };
            let instruction = format!(
                "Transcribe the speech in this audio exactly, word for word. It is a short message for a \
                 Genshin Impact in-game chat.{language_hint} Output only the transcript with normal punctuation: \
                 no quotes, labels or commentary. If there is no speech, output nothing. When these names or \
                 terms are spoken, spell them like this: {}.",
                words.join(", ")
            );
            let body = serde_json::json!({
                "contents": [{
                    "parts": [
                        { "text": instruction },
                        { "inline_data": {
                            "mime_type": "audio/wav",
                            "data": base64::engine::general_purpose::STANDARD.encode(&wav),
                        } },
                    ]
                }],
                "generationConfig": { "temperature": 0 },
            });
            let request = client
                .post(format!("{}/models/{}:generateContent", base(&config), config.model.trim()))
                .header("x-goog-api-key", key)
                .json(&body);
            let json = send_json(request, "Gemini").await?;
            let text = json["candidates"][0]["content"]["parts"]
                .as_array()
                .map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<String>())
                .unwrap_or_default();
            Ok(text)
        }
        // OpenAI-compatible: groq, openai, custom.
        _ => {
            let mut form = reqwest::multipart::Form::new()
                .part("file", wav_part(wav)?)
                .text("model", config.model.trim().to_string())
                .text("response_format", "json")
                .text("temperature", "0");
            form = text_part(form, "language", language);
            form = text_part(form, "prompt", &s.prompt());
            let request = client
                .post(format!("{}/audio/transcriptions", base(&config)))
                .bearer_auth(key)
                .multipart(form);
            let json = send_json(request, provider_name(&s.engine)).await?;
            Ok(json["text"].as_str().unwrap_or_default().to_string())
        }
    }
}

pub fn provider_name(id: &str) -> &'static str {
    match id {
        "local" => "Local Whisper",
        "groq" => "Groq",
        "gemini" => "Gemini",
        "deepgram" => "Deepgram",
        "elevenlabs" => "ElevenLabs",
        "mistral" => "Mistral",
        "openai" => "OpenAI",
        _ => "Custom API",
    }
}

/// Trims, drops known silence hallucinations and applies `wrong => right` replacements.
pub fn clean_transcript(raw: &str, replacements: &str) -> String {
    let mut text = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if HALLUCINATIONS.contains(&text.to_lowercase().as_str()) {
        return String::new();
    }
    for line in replacements.lines() {
        if let Some((from, to)) = line.split_once("=>") {
            let (from, to) = (from.trim(), to.trim());
            if !from.is_empty() {
                text = replace_case_insensitive(&text, from, to);
            }
        }
    }
    text.trim().to_string()
}

fn replace_case_insensitive(text: &str, from: &str, to: &str) -> String {
    let lower = text.to_lowercase();
    let needle = from.to_lowercase();
    // Lowercasing can change byte lengths for some scripts; fall back to exact matching then.
    if lower.len() != text.len() {
        return text.replace(from, to);
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (idx, _) in lower.match_indices(&needle) {
        out.push_str(&text[last..idx]);
        out.push_str(to);
        last = idx + needle.len();
    }
    out.push_str(&text[last..]);
    out
}

pub fn model_path(models_dir: &Path, model: &str) -> PathBuf {
    models_dir.join(format!("ggml-{model}.bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_whitespace_and_hallucinations() {
        assert_eq!(clean_transcript("  hey   there \n", ""), "hey there");
        assert_eq!(clean_transcript(" Thank you. ", ""), "");
        assert_eq!(clean_transcript("[BLANK_AUDIO]", ""), "");
    }

    #[test]
    fn applies_replacements_case_insensitively() {
        let r = "jen shin => Genshin\nriot fishy => Wriothesley";
        assert_eq!(clean_transcript("Jen Shin with Riot Fishy?", r), "Genshin with Wriothesley?");
    }
}
