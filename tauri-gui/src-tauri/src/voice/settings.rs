use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const DEFAULT_VOCABULARY: &str = "Genshin, Teyvat, Paimon, Mondstadt, Liyue, Inazuma, Sumeru, Fontaine, Natlan, Snezhnaya, Nod-Krai, \
Wriothesley, Neuvillette, Furina, Navia, Chasca, Mavuika, Xilonen, Kinich, Mualani, Citlali, Ororon, Varesa, Iansan, \
Yae Miko, Raiden, Nahida, Zhongli, Venti, Kazuha, Xiao, Hu Tao, Ganyu, Ayaka, Yelan, Alhaitham, Cyno, Tighnari, \
Kokomi, Bennett, Xiangling, Xingqiu, Fischl, Diluc, Kaeya, Lyney, Arlecchino, Clorinde, Emilie, Escoffier, Skirk, \
artifacts, resin, primogems, co-op, domain, ley line, weekly boss, Spiral Abyss, Imaginarium Theater";

/// Rough character budget for the vocabulary part of the prompt (~3 chars per token for names).
const MAX_PROMPT_CHARS: usize = 520;

/// Cloud providers: (id, default base URL, default model).
pub const PROVIDERS: &[(&str, &str, &str)] = &[
    ("groq", "https://api.groq.com/openai/v1", "whisper-large-v3"),
    ("gemini", "https://generativelanguage.googleapis.com/v1beta", "gemini-3.5-flash-lite"),
    ("deepgram", "https://api.deepgram.com/v1", "nova-3"),
    ("elevenlabs", "https://api.elevenlabs.io/v1", "scribe_v2"),
    ("mistral", "https://api.mistral.ai/v1", "voxtral-mini-latest"),
    ("openai", "https://api.openai.com/v1", "gpt-4o-transcribe"),
    ("custom", "", "whisper-1"),
];

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CloudConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct VoiceSettings {
    /// "local" (whisper-server) or a cloud provider id from `PROVIDERS`.
    pub engine: String,
    /// Substring of the input device name; empty = Windows default microphone.
    pub mic_name: String,
    /// Whisper language code ("en", "auto", ...).
    pub language: String,
    /// ggml model id, e.g. "small.en-q5_1" -> ggml-small.en-q5_1.bin
    pub local_model: String,
    pub local_gpu: bool,
    /// Per-provider key/model/URL, so switching providers keeps each one's key.
    pub cloud: BTreeMap<String, CloudConfig>,
    /// Comma/newline separated words used to bias spelling of names.
    pub vocabulary: String,
    /// Lines of `wrong => right` applied to every transcript.
    pub replacements: String,

    // Pre-provider single cloud config; read once to migrate into `cloud["groq"]`.
    #[serde(skip_serializing)]
    cloud_base_url: String,
    #[serde(skip_serializing)]
    cloud_model: String,
    #[serde(skip_serializing)]
    cloud_api_key: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        let mut s = Self {
            engine: "local".into(),
            mic_name: "Wireless Controller".into(),
            language: "en".into(),
            local_model: "small.en-q5_1".into(),
            local_gpu: false,
            cloud: BTreeMap::new(),
            vocabulary: DEFAULT_VOCABULARY.into(),
            replacements: String::new(),
            cloud_base_url: String::new(),
            cloud_model: String::new(),
            cloud_api_key: String::new(),
        };
        s.normalize();
        s
    }
}

impl VoiceSettings {
    pub fn load(path: &Path) -> Self {
        let mut s: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.normalize();
        s
    }

    /// Migrates old fields and makes sure every provider has a model and base URL.
    pub fn normalize(&mut self) {
        if self.engine == "cloud" {
            self.engine = "groq".into();
        }
        if !self.cloud_api_key.is_empty() {
            let groq = self.cloud.entry("groq".into()).or_default();
            if groq.api_key.is_empty() {
                groq.api_key = std::mem::take(&mut self.cloud_api_key);
                groq.model = std::mem::take(&mut self.cloud_model);
                groq.base_url = std::mem::take(&mut self.cloud_base_url);
            }
        }
        for (id, base_url, model) in PROVIDERS {
            let config = self.cloud.entry((*id).into()).or_default();
            if config.model.trim().is_empty() {
                config.model = (*model).into();
            }
            if config.base_url.trim().is_empty() {
                config.base_url = (*base_url).into();
            }
        }
        if self.engine != "local" && !PROVIDERS.iter().any(|(id, ..)| *id == self.engine) {
            self.engine = "local".into();
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn is_local(&self) -> bool {
        self.engine == "local"
    }

    /// Config for the selected cloud provider.
    pub fn provider(&self) -> CloudConfig {
        self.cloud.get(&self.engine).cloned().unwrap_or_default()
    }

    /// Vocabulary words, trimmed so the total stays within a Whisper-sized prompt. Words are dropped
    /// from the front so the names the user appended last (friends) always make it in.
    pub fn vocab_words(&self) -> Vec<&str> {
        let mut words: Vec<&str> = self
            .vocabulary
            .split([',', '\n'])
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .collect();
        while words.iter().map(|w| w.len() + 2).sum::<usize>() > MAX_PROMPT_CHARS && words.len() > 1 {
            words.remove(0);
        }
        words
    }

    /// Whisper prompt built from the vocabulary list (Groq rejects prompts over ~224 tokens).
    pub fn prompt(&self) -> String {
        let words = self.vocab_words();
        if words.is_empty() {
            String::new()
        } else {
            format!("Game chat about Genshin Impact. Names: {}.", words.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_capped_and_keeps_last_words() {
        let s = VoiceSettings {
            vocabulary: format!("{}, duqs, Kazuto", DEFAULT_VOCABULARY),
            ..Default::default()
        };
        let prompt = s.prompt();
        assert!(prompt.len() < MAX_PROMPT_CHARS + 60);
        assert!(prompt.ends_with("duqs, Kazuto."));
    }

    #[test]
    fn migrates_old_cloud_settings_to_groq() {
        let old = r#"{"engine":"cloud","cloud_api_key":"gsk_x","cloud_model":"whisper-large-v3-turbo",
            "cloud_base_url":"https://api.groq.com/openai/v1"}"#;
        let mut s: VoiceSettings = serde_json::from_str(old).unwrap();
        s.normalize();
        assert_eq!(s.engine, "groq");
        assert_eq!(s.provider().api_key, "gsk_x");
        assert_eq!(s.provider().model, "whisper-large-v3-turbo");
        assert_eq!(s.cloud["deepgram"].model, "nova-3");
        assert!(!serde_json::to_string(&s).unwrap().contains("cloud_api_key"));
    }
}
