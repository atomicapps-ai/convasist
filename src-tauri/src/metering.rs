//! Usage metering — the shell half of the ledger (`conva_core::metering`).
//!
//! Records what the owner's BYO keys are spent on so Settings → Usage can show
//! it: LLM tokens per provider, and Tavily web searches. The live ledger is
//! held in `AppState.usage` (a `Mutex`) and mirrored to `<app-data>/usage.json`
//! after every change, so counts survive restarts. All writes are best-effort:
//! metering must never break a feature, so a failed persist is logged, not
//! propagated.

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use conva_core::llm::{ProviderId, TokenUsage};
use conva_core::metering::{UsageLedger, UsageSummary};

use crate::session::now_unix_ms;
use crate::AppState;

fn ledger_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("usage.json"))
}

/// Read the persisted ledger at startup. Missing/corrupt file → a fresh ledger.
pub fn load(app: &AppHandle) -> UsageLedger {
    ledger_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn persist(app: &AppHandle, ledger: &UsageLedger) {
    let Some(path) = ledger_path(app) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(ledger) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("[metering] could not write usage.json: {e}");
            }
        }
        Err(e) => eprintln!("[metering] could not serialize usage ledger: {e}"),
    }
}

/// Attribute one completion's tokens to `provider`, then persist. Best-effort.
pub fn record_llm(app: &AppHandle, provider: ProviderId, usage: TokenUsage) {
    let state = app.state::<AppState>();
    let mut ledger = state.usage.lock().expect("usage lock");
    ledger.record_llm(provider, usage, now_unix_ms());
    persist(app, &ledger);
}

/// Count `count` Tavily searches, then persist. Best-effort.
pub fn record_tavily_search(app: &AppHandle, count: u64) {
    let state = app.state::<AppState>();
    let mut ledger = state.usage.lock().expect("usage lock");
    ledger.record_tavily_search(count, now_unix_ms());
    persist(app, &ledger);
}

/// Count `chars` synthesized by TTS (Aura bills per character), then persist.
pub fn record_tts_characters(app: &AppHandle, chars: u64) {
    let state = app.state::<AppState>();
    let mut ledger = state.usage.lock().expect("usage lock");
    ledger.record_tts_characters(chars, now_unix_ms());
    persist(app, &ledger);
}

/// The Settings → Usage snapshot.
pub fn summary(app: &AppHandle) -> UsageSummary {
    let state = app.state::<AppState>();
    let ledger = state.usage.lock().expect("usage lock");
    ledger.summary()
}

/// Clear all counters (Settings → Usage "reset"). Returns the empty snapshot.
pub fn reset(app: &AppHandle) -> UsageSummary {
    let state = app.state::<AppState>();
    let mut ledger = state.usage.lock().expect("usage lock");
    ledger.reset(now_unix_ms());
    persist(app, &ledger);
    ledger.summary()
}
