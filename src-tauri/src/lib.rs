//! convasist Tauri shell — wires the UI to the core layers.
//!
//! M1 state: real dual capture (mic + WASAPI loopback) with VU meters,
//! stall watchdog, and reopen-on-error hot-swap. ASR attaches to the frame
//! path in M2 without changing any command or event signature.

mod audio;
mod session;

use std::fs;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};

use convasist_core::audio::AudioDevice;
use convasist_core::config::AppConfig;
use convasist_core::llm::{provider_registry, ProviderInfo};

use session::SessionManager;

/// In-memory app state; the config mirrors the JSON file on disk.
struct AppState {
    config: Mutex<AppConfig>,
    session: SessionManager,
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

#[tauri::command]
fn list_audio_devices() -> Vec<AudioDevice> {
    audio::list_devices()
}

#[tauri::command]
fn start_session(app: AppHandle, state: State<AppState>) -> Result<String, String> {
    let config = state.config.lock().expect("config lock").clone();
    if !config.consent_acknowledged {
        return Err("consent_required".into());
    }
    state
        .session
        .start(&app, &config)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn stop_session(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.session.stop(&app).map_err(|e| e.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config = load_config(app.handle());
            app.manage(AppState {
                config: Mutex::new(config),
                session: SessionManager::new(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_provider_registry,
            list_audio_devices,
            start_session,
            stop_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running convasist");
}
