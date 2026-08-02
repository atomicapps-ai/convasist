//! Shared web-search helper (Tavily). Used by Ally's `web_search` tool (the
//! model calls it only when it decides fresh/external info is needed). The
//! caller owns the key lookup and usage metering; this is just the HTTP call.
//!
//! NOTE: `simcon::research` still has its own bounded Tavily loop for Sim Con
//! knowledge-profile building; unifying the two onto this helper is a safe
//! follow-up once the Sim Con research path is confirmed on-device.

use std::time::Duration;

use conva_core::simcon::ResearchSource;
use conva_core::CoreError;

use crate::session::now_unix_ms;

/// One Tavily search. Returns the result rows (title/url/snippet). Errors on a
/// network/HTTP failure; an empty result set is `Ok(vec![])`.
pub fn tavily_search(
    api_key: &str,
    query: &str,
    max_results: usize,
) -> Result<Vec<ResearchSource>, CoreError> {
    let body = serde_json::json!({
        "api_key": api_key,
        "query": query,
        "max_results": max_results,
        "search_depth": "basic",
    });
    let resp = ureq::post("https://api.tavily.com/search")
        .timeout(Duration::from_secs(15))
        .send_json(body)
        .map_err(|e| CoreError::Llm(e.to_string()))?;
    let val: serde_json::Value = resp
        .into_json()
        .map_err(|e| CoreError::Llm(e.to_string()))?;

    let mut out = Vec::new();
    if let Some(results) = val.get("results").and_then(|r| r.as_array()) {
        for r in results {
            let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.is_empty() {
                continue;
            }
            out.push(ResearchSource {
                title: r
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                url: url.to_string(),
                snippet: r
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(500)
                    .collect(),
                fetched_at_unix_ms: now_unix_ms(),
            });
        }
    }
    Ok(out)
}
