//! convasist Tauri shell — wires the UI to the core layers.
//!
//! M0 scope: config persistence, provider registry exposure, and session
//! lifecycle stubs that exercise the full typed IPC path end-to-end. Real
//! capture (M1) and ASR (M2) replace the stub internals without changing
//! any command or event signature.

use std::fs;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use convasist_core::config::AppConfig;
use convasist_core::ipc::{events, SessionStateEvent};
use convasist_core::llm::{provider_registry, ProviderInfo};

/// In-memory app state; the config mirrors the JSON file on disk.
struct AppState {
    config: Mutex<AppConfig>,
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("no app config dir: {e}"))?;
    Ok(dir.join("config.json"))
}

fn load_config(app: &AppHandle) -> AppConfig {
    config_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn persist_config(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().expect("config lock").clone()
}

#[tauri::command]
fn save_config(app: AppHandle, state: State<AppState>, config: AppConfig) -> Result<(), String> {
    persist_config(&app, &config)?;
    *state.config.lock().expect("config lock") = config;
    Ok(())
}

#[tauri::command]
fn get_provider_registry() -> Vec<ProviderInfo> {
    provider_registry()
}

/// M1 replaces the stub internals with real WASAPI capture start.
#[tauri::command]
fn start_session(app: AppHandle, state: State<AppState>) -> Result<String, String> {
    let config = state.config.lock().expect("config lock");
    if !config.consent_acknowledged {
        return Err("consent_required".into());
    }
    let session_id = format!("session-{}", std::process::id());
    app.emit(
        events::SESSION_STATE,
        SessionStateEvent::Listening {
            session_id: session_id.clone(),
            started_at_unix_ms: now_unix_ms(),
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(session_id)
}

#[tauri::command]
fn stop_session(app: AppHandle) -> Result<(), String> {
    app.emit(events::SESSION_STATE, SessionStateEvent::Idle)
        .map_err(|e| e.to_string())
}

fn now_unix_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config = load_config(app.handle());
            app.manage(AppState {
                config: Mutex::new(config),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_provider_registry,
            start_session,
            stop_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running convasist");
}
