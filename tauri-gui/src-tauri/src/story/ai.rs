//! Text chat with the cloud providers the user already has keys for (shared with Voice Chat).

use crate::voice::settings::CloudConfig;
use crate::voice::stt::send_json;

/// Providers with a chat API: (id, default model).
pub const PROVIDERS: &[(&str, &str)] = &[
    ("groq", "llama-3.3-70b-versatile"),
    ("gemini", "gemini-3.5-flash-lite"),
    ("openai", "gpt-4o-mini"),
    ("mistral", "mistral-small-latest"),
    ("custom", "gpt-4o-mini"),
];

pub fn default_model(provider: &str) -> &'static str {
    PROVIDERS.iter().find(|(id, _)| *id == provider).map_or("", |(_, model)| model)
}

pub async fn chat(
    client: &reqwest::Client,
    provider: &str,
    config: &CloudConfig,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let key = config.api_key.trim();
    if key.is_empty() {
        return Err(format!("No API key for {provider} — add it in the API Keys tab"));
    }
    let base = config.base_url.trim().trim_end_matches('/');
    let model = if model.trim().is_empty() { default_model(provider) } else { model.trim() };

    if provider == "gemini" {
        let body = serde_json::json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": [{ "role": "user", "parts": [{ "text": user }] }],
            "generationConfig": { "temperature": 0.3 },
        });
        let request = client
            .post(format!("{base}/models/{model}:generateContent"))
            .header("x-goog-api-key", key)
            .json(&body);
        let json = send_json(request, "Gemini").await?;
        let text = json["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<String>())
            .unwrap_or_default();
        return Ok(text.trim().to_string());
    }

    // OpenAI-compatible chat completions: groq, openai, mistral, custom.
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.3,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    let request = client.post(format!("{base}/chat/completions")).bearer_auth(key).json(&body);
    let json = send_json(request, provider).await?;
    Ok(json["choices"][0]["message"]["content"].as_str().unwrap_or_default().trim().to_string())
}
