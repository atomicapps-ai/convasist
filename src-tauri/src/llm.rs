//! LLM provider clients (design §4.6): one small SSE client per provider,
//! all normalized to a token callback. Blocking `ureq` driven from worker
//! threads — the UI receives tokens as ASSIST_CHUNK events.
//!
//! Providers:
//! - Anthropic (native Messages API) — the default
//! - OpenAI-compatible adapter — OpenAI, xAI, DeepSeek, and local Ollama
//! - Google Gemini (generateContent SSE)

use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use convasist_core::llm::{LlmRequest, ModelInfo, ProviderId};
use convasist_core::CoreError;

const HTTP_TIMEOUT: Duration = Duration::from_secs(120);

/// Streaming completion: `on_token` receives text deltas as they arrive.
pub fn stream_completion(
    provider: ProviderId,
    api_key: &str,
    model: &str,
    request: &LlmRequest,
    on_token: &mut dyn FnMut(&str),
) -> Result<(), CoreError> {
    match provider {
        ProviderId::Anthropic => anthropic_stream(api_key, model, request, on_token),
        ProviderId::Openai | ProviderId::Xai | ProviderId::Deepseek | ProviderId::OllamaLocal => {
            openai_compatible_stream(provider, api_key, model, request, on_token)
        }
        ProviderId::Google => gemini_stream(api_key, model, request, on_token),
    }
}

/// The settings "Test" button (§4.6): one tiny completion; returns measured
/// first-token latency in ms.
pub fn validate_key(provider: ProviderId, api_key: &str, model: &str) -> Result<u32, CoreError> {
    let request = LlmRequest {
        system: "Reply with the single word: ok".into(),
        user: "ping".into(),
        max_tokens: 8,
    };
    let started = Instant::now();
    let mut first: Option<u32> = None;
    stream_completion(provider, api_key, model, &request, &mut |_| {
        first.get_or_insert_with(|| started.elapsed().as_millis() as u32);
    })?;
    first.ok_or_else(|| CoreError::Llm("no tokens returned".into()))
}

/// Live model list where the provider offers one (§4.6). Errors and
/// unsupported providers fall back to the curated defaults UI-side.
pub fn list_models(provider: ProviderId, api_key: &str) -> Result<Vec<ModelInfo>, CoreError> {
    let (url, auth_header) = match provider {
        ProviderId::Anthropic => (
            "https://api.anthropic.com/v1/models".to_string(),
            ("x-api-key", api_key.to_string()),
        ),
        ProviderId::Openai | ProviderId::Xai | ProviderId::Deepseek | ProviderId::OllamaLocal => (
            format!("{}/models", openai_base(provider)),
            ("Authorization", format!("Bearer {api_key}")),
        ),
        ProviderId::Google => {
            // Gemini's list API shape differs; curated defaults suffice.
            return Err(CoreError::Llm("model list unsupported".into()));
        }
    };

    let mut req = ureq::get(&url).timeout(HTTP_TIMEOUT);
    req = req.set(auth_header.0, &auth_header.1);
    if matches!(provider, ProviderId::Anthropic) {
        req = req.set("anthropic-version", "2023-06-01");
    }
    let body: Value = req
        .call()
        .map_err(map_ureq)?
        .into_json()
        .map_err(|e| CoreError::Llm(e.to_string()))?;

    let models = body["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|m| m["id"].as_str())
                .map(|id| ModelInfo {
                    id: id.to_string(),
                    display_name: id.to_string(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err(CoreError::Llm("empty model list".into()));
    }
    Ok(models)
}

fn map_ureq(e: ureq::Error) -> CoreError {
    match e {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let snippet: String = body.chars().take(300).collect();
            CoreError::Llm(format!("HTTP {code}: {snippet}"))
        }
        other => CoreError::Llm(other.to_string()),
    }
}

/// Iterate `data: {...}` SSE payloads, stopping on stream end.
fn for_each_sse_data(
    reader: impl std::io::Read,
    mut handle: impl FnMut(&Value),
) -> Result<(), CoreError> {
    let buffered = BufReader::new(reader);
    for line in buffered.lines() {
        let line = line.map_err(|e| CoreError::Llm(format!("stream read: {e}")))?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        if let Ok(value) = serde_json::from_str::<Value>(data) {
            handle(&value);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- Anthropic

fn anthropic_stream(
    api_key: &str,
    model: &str,
    request: &LlmRequest,
    on_token: &mut dyn FnMut(&str),
) -> Result<(), CoreError> {
    let response = ureq::post("https://api.anthropic.com/v1/messages")
        .timeout(HTTP_TIMEOUT)
        .set("x-api-key", api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(json!({
            "model": model,
            "max_tokens": request.max_tokens,
            "system": request.system,
            "messages": [{"role": "user", "content": request.user}],
            "stream": true,
        }))
        .map_err(map_ureq)?;

    for_each_sse_data(response.into_reader(), |value| {
        if value["type"] == "content_block_delta" {
            if let Some(text) = value["delta"]["text"].as_str() {
                on_token(text);
            }
        }
    })
}

// ------------------------------------------------------- OpenAI-compatible

fn openai_base(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Openai => "https://api.openai.com/v1",
        ProviderId::Xai => "https://api.x.ai/v1",
        ProviderId::Deepseek => "https://api.deepseek.com/v1",
        ProviderId::OllamaLocal => "http://127.0.0.1:11434/v1",
        _ => unreachable!("not an OpenAI-compatible provider"),
    }
}

fn openai_compatible_stream(
    provider: ProviderId,
    api_key: &str,
    model: &str,
    request: &LlmRequest,
    on_token: &mut dyn FnMut(&str),
) -> Result<(), CoreError> {
    let url = format!("{}/chat/completions", openai_base(provider));
    let mut req = ureq::post(&url)
        .timeout(HTTP_TIMEOUT)
        .set("content-type", "application/json");
    if !api_key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {api_key}"));
    }
    let response = req
        .send_json(json!({
            "model": model,
            "max_tokens": request.max_tokens,
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": request.user},
            ],
            "stream": true,
        }))
        .map_err(map_ureq)?;

    for_each_sse_data(response.into_reader(), |value| {
        if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
            on_token(text);
        }
    })
}

// ------------------------------------------------------------------ Gemini

fn gemini_stream(
    api_key: &str,
    model: &str,
    request: &LlmRequest,
    on_token: &mut dyn FnMut(&str),
) -> Result<(), CoreError> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse"
    );
    let response = ureq::post(&url)
        .timeout(HTTP_TIMEOUT)
        .set("content-type", "application/json")
        .set("x-goog-api-key", api_key)
        .send_json(json!({
            "systemInstruction": {"parts": [{"text": request.system}]},
            "contents": [{"role": "user", "parts": [{"text": request.user}]}],
            "generationConfig": {"maxOutputTokens": request.max_tokens},
        }))
        .map_err(map_ureq)?;

    for_each_sse_data(response.into_reader(), |value| {
        if let Some(parts) = value["candidates"][0]["content"]["parts"].as_array() {
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    on_token(text);
                }
            }
        }
    })
}

// -------------------------------------------------------------- key vault

const KEYRING_SERVICE: &str = "convasist";

fn keyring_entry(provider: ProviderId) -> Result<keyring::Entry, CoreError> {
    let user = format!(
        "api-key-{}",
        serde_json::to_string(&provider)
            .unwrap_or_default()
            .trim_matches('"')
    );
    keyring::Entry::new(KEYRING_SERVICE, &user).map_err(|e| CoreError::Llm(e.to_string()))
}

pub fn store_api_key(provider: ProviderId, key: &str) -> Result<(), CoreError> {
    let entry = keyring_entry(provider)?;
    if key.is_empty() {
        // Empty submission clears the stored key.
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CoreError::Llm(e.to_string())),
        }
    } else {
        entry
            .set_password(key)
            .map_err(|e| CoreError::Llm(e.to_string()))
    }
}

pub fn load_api_key(provider: ProviderId) -> Result<Option<String>, CoreError> {
    match keyring_entry(provider)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CoreError::Llm(e.to_string())),
    }
}

/// Resolve the key a request must use: the stored key, an empty string for
/// keyless local providers, or `api_key_missing`.
pub fn resolve_key(provider: ProviderId) -> Result<String, CoreError> {
    let requires_key = convasist_core::llm::provider_registry()
        .into_iter()
        .find(|p| p.id == provider)
        .map(|p| p.requires_api_key)
        .unwrap_or(true);
    match load_api_key(provider)? {
        Some(key) => Ok(key),
        None if !requires_key => Ok(String::new()),
        None => Err(CoreError::Llm("api_key_missing".into())),
    }
}
