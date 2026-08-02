//! LLM provider clients (design §4.6): one small SSE client per provider,
//! all normalized to a token callback. Blocking `ureq` driven from worker
//! threads — the UI receives tokens as ALLY_CHUNK events.
//!
//! Providers:
//! - Anthropic (native Messages API) — the default
//! - OpenAI-compatible adapter — OpenAI, xAI, DeepSeek, and local Ollama
//! - Google Gemini (generateContent SSE)

use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use conva_core::llm::{LlmRequest, ModelInfo, ProviderId, TokenUsage};
use conva_core::CoreError;

const HTTP_TIMEOUT: Duration = Duration::from_secs(120);

/// Streaming completion: `on_token` receives text deltas as they arrive.
/// Returns the provider-reported [`TokenUsage`] for metering (zeros when the
/// provider doesn't report usage, e.g. some local endpoints).
pub fn stream_completion(
    provider: ProviderId,
    api_key: &str,
    model: &str,
    request: &LlmRequest,
    on_token: &mut dyn FnMut(&str),
) -> Result<TokenUsage, CoreError> {
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
    // The Test button is a diagnostic ping, not feature usage, so its tokens are
    // intentionally not recorded in the usage ledger.
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
) -> Result<TokenUsage, CoreError> {
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

    // Anthropic reports input tokens in `message_start` and the (cumulative)
    // output count in each `message_delta` — keep the latest of each.
    let mut usage = TokenUsage::default();
    for_each_sse_data(response.into_reader(), |value| {
        match value["type"].as_str() {
            Some("content_block_delta") => {
                if let Some(text) = value["delta"]["text"].as_str() {
                    on_token(text);
                }
            }
            Some("message_start") => {
                let u = &value["message"]["usage"];
                if let Some(n) = u["input_tokens"].as_u64() {
                    usage.input_tokens = n;
                }
                if let Some(n) = u["output_tokens"].as_u64() {
                    usage.output_tokens = n;
                }
            }
            Some("message_delta") => {
                if let Some(n) = value["usage"]["output_tokens"].as_u64() {
                    usage.output_tokens = n;
                }
            }
            _ => {}
        }
    })?;
    Ok(usage)
}

/// Anthropic streaming **with tool use** — the Ally web-search loop. Streams
/// assistant text via `on_token`; when the model requests a tool, `run_tool(name,
/// input)` is called and its string output is fed back as a `tool_result`, up to
/// `max_rounds` tool rounds (tools are withheld on the final round so the loop
/// always terminates in a text answer). Returns the token usage **summed across
/// every round** — so metering captures the full cost of a tool-assisted answer.
///
/// The common case (model answers without searching) is a single request, no
/// slower than [`anthropic_stream`]; the extra round-trip is paid only when the
/// model actually calls a tool.
#[allow(clippy::too_many_arguments)]
pub fn anthropic_stream_with_tools(
    api_key: &str,
    model: &str,
    request: &LlmRequest,
    tools: &Value,
    on_token: &mut dyn FnMut(&str),
    run_tool: &mut dyn FnMut(&str, &Value) -> String,
    max_rounds: usize,
) -> Result<TokenUsage, CoreError> {
    use std::collections::HashMap;

    let mut messages: Vec<Value> = vec![json!({"role": "user", "content": request.user})];
    let mut total = TokenUsage::default();

    for round in 0..=max_rounds {
        // Offer tools only while another round remains; the final round forces a
        // plain text answer so the conversation can't loop forever.
        let offer_tools = round < max_rounds;
        let mut body = json!({
            "model": model,
            "max_tokens": request.max_tokens,
            "system": request.system,
            "messages": messages,
            "stream": true,
        });
        if offer_tools {
            body["tools"] = tools.clone();
        }

        let response = ureq::post("https://api.anthropic.com/v1/messages")
            .timeout(HTTP_TIMEOUT)
            .set("x-api-key", api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(body)
            .map_err(map_ureq)?;

        let mut round_usage = TokenUsage::default();
        let mut stop_reason = String::new();
        let mut text_acc = String::new();
        // Tool-use blocks captured this round: (id, name, accumulated input JSON).
        let mut tool_blocks: Vec<(String, String, String)> = Vec::new();
        // SSE content-block index -> position in `tool_blocks` (absent = text).
        let mut index_to_tool: HashMap<u64, usize> = HashMap::new();

        for_each_sse_data(response.into_reader(), |value| {
            match value["type"].as_str() {
                Some("message_start") => {
                    let u = &value["message"]["usage"];
                    if let Some(n) = u["input_tokens"].as_u64() {
                        round_usage.input_tokens = n;
                    }
                    if let Some(n) = u["output_tokens"].as_u64() {
                        round_usage.output_tokens = n;
                    }
                }
                Some("content_block_start") => {
                    let cb = &value["content_block"];
                    if cb["type"] == "tool_use" {
                        let idx = value["index"].as_u64().unwrap_or(0);
                        tool_blocks.push((
                            cb["id"].as_str().unwrap_or("").to_string(),
                            cb["name"].as_str().unwrap_or("").to_string(),
                            String::new(),
                        ));
                        index_to_tool.insert(idx, tool_blocks.len() - 1);
                    }
                }
                Some("content_block_delta") => {
                    let delta = &value["delta"];
                    match delta["type"].as_str() {
                        Some("text_delta") => {
                            if let Some(t) = delta["text"].as_str() {
                                text_acc.push_str(t);
                                on_token(t);
                            }
                        }
                        Some("input_json_delta") => {
                            let idx = value["index"].as_u64().unwrap_or(0);
                            if let (Some(pos), Some(pj)) = (
                                index_to_tool.get(&idx).copied(),
                                delta["partial_json"].as_str(),
                            ) {
                                tool_blocks[pos].2.push_str(pj);
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_delta") => {
                    if let Some(sr) = value["delta"]["stop_reason"].as_str() {
                        stop_reason = sr.to_string();
                    }
                    if let Some(n) = value["usage"]["output_tokens"].as_u64() {
                        round_usage.output_tokens = n;
                    }
                }
                _ => {}
            }
        })?;

        total.input_tokens = total.input_tokens.saturating_add(round_usage.input_tokens);
        total.output_tokens = total
            .output_tokens
            .saturating_add(round_usage.output_tokens);

        // No tool requested → this round's text is the final answer.
        if stop_reason != "tool_use" || tool_blocks.is_empty() {
            break;
        }

        // Echo the assistant turn (text + tool_use) back, then answer each
        // tool_use with a tool_result, and let the model continue next round.
        let mut assistant_content: Vec<Value> = Vec::new();
        if !text_acc.trim().is_empty() {
            assistant_content.push(json!({"type": "text", "text": text_acc}));
        }
        let mut tool_results: Vec<Value> = Vec::new();
        for (id, name, json_buf) in &tool_blocks {
            let input: Value = serde_json::from_str(json_buf).unwrap_or_else(|_| json!({}));
            assistant_content.push(json!({
                "type": "tool_use", "id": id, "name": name, "input": input,
            }));
            let result_text = run_tool(name, &input);
            tool_results.push(json!({
                "type": "tool_result", "tool_use_id": id, "content": result_text,
            }));
        }
        messages.push(json!({"role": "assistant", "content": assistant_content}));
        messages.push(json!({"role": "user", "content": tool_results}));
    }

    Ok(total)
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
) -> Result<TokenUsage, CoreError> {
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
            // Ask for a final usage chunk (ignored by providers that don't
            // support it — those simply report zeros).
            "stream_options": {"include_usage": true},
        }))
        .map_err(map_ureq)?;

    let mut usage = TokenUsage::default();
    for_each_sse_data(response.into_reader(), |value| {
        if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
            on_token(text);
        }
        // The final chunk (empty `choices`) carries cumulative usage.
        let u = &value["usage"];
        if let Some(n) = u["prompt_tokens"].as_u64() {
            usage.input_tokens = n;
        }
        if let Some(n) = u["completion_tokens"].as_u64() {
            usage.output_tokens = n;
        }
    })?;
    Ok(usage)
}

// ------------------------------------------------------------------ Gemini

fn gemini_stream(
    api_key: &str,
    model: &str,
    request: &LlmRequest,
    on_token: &mut dyn FnMut(&str),
) -> Result<TokenUsage, CoreError> {
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

    // Gemini reports cumulative `usageMetadata` on each chunk — keep the latest.
    let mut usage = TokenUsage::default();
    for_each_sse_data(response.into_reader(), |value| {
        if let Some(parts) = value["candidates"][0]["content"]["parts"].as_array() {
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    on_token(text);
                }
            }
        }
        let meta = &value["usageMetadata"];
        if let Some(n) = meta["promptTokenCount"].as_u64() {
            usage.input_tokens = n;
        }
        if let Some(n) = meta["candidatesTokenCount"].as_u64() {
            usage.output_tokens = n;
        }
    })?;
    Ok(usage)
}

// -------------------------------------------------------------- key vault

const KEYRING_SERVICE: &str = "conva";

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
    let requires_key = conva_core::llm::provider_registry()
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
