//! convasist Tauri shell — wires the UI to the core layers.
//!
//! M3 state: dual capture (mic + WASAPI loopback) → per-side whisper.cpp
//! transcription → manual AI assist streaming through the provider
//! registry (Claude default), with API keys in the OS credential vault.

mod asr;
mod audio;
mod llm;
mod models;
mod rag;
mod session;
mod tracker;

use std::fs;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use std::sync::Arc;

use convasist_core::asr::TranscriptSegment;
use convasist_core::audio::AudioDevice;
use convasist_core::config::AppConfig;
use convasist_core::ipc::{events, AssistChunkEvent, AssistSource, AssistSourcesEvent};
use convasist_core::llm::{provider_registry, ModelInfo, ProviderId, ProviderInfo};
use convasist_core::prompt::{build_assist_request, AssistKind};
use convasist_core::rag::{IngestReport, RagDocument};

use rag::RagStore;
use session::SessionManager;

/// In-memory app state; the config mirrors the JSON file on disk.
struct AppState {
    config: Mutex<AppConfig>,
    session: SessionManager,
    rag: Arc<RagStore>,
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

// Async commands run off the main thread — model load (~1 s) and session
// teardown must never freeze the UI.
#[tauri::command]
async fn start_session(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let config = state.config.lock().expect("config lock").clone();
    if !config.consent_acknowledged {
        return Err("consent_required".into());
    }
    let rag = state.rag.clone();
    state
        .session
        .start(&app, &config, rag)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn session_list(app: AppHandle) -> Result<Vec<session::SessionSummary>, String> {
    session::list_sessions(&app).map_err(|e| e.to_string())
}

#[tauri::command]
fn session_load(app: AppHandle, id: String) -> Result<Vec<TranscriptSegment>, String> {
    session::load_session(&app, &id).map_err(|e| e.to_string())
}

/// Export a transcript as Markdown to a caller-chosen path (U8). The UI
/// obtains `path` from the native save dialog.
#[tauri::command]
fn export_transcript(path: String, segments: Vec<TranscriptSegment>) -> Result<(), String> {
    use convasist_core::audio::StreamSide;
    let mut out = String::from("# convasist transcript\n\n");
    for s in segments.iter().filter(|s| s.is_final) {
        let speaker = match s.side {
            StreamSide::Inbound => "Them",
            StreamSide::Outbound => "You",
        };
        let total_seconds = s.start_ms / 1000;
        out.push_str(&format!(
            "**{speaker}** ({:02}:{:02}:{:02}): {}\n\n",
            total_seconds / 3600,
            (total_seconds % 3600) / 60,
            total_seconds % 60,
            s.text.trim()
        ));
    }
    fs::write(&path, out).map_err(|e| e.to_string())
}

#[tauri::command]
async fn stop_session(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.session.stop(&app).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct ProviderKeyStatus {
    id: ProviderId,
    has_key: bool,
}

#[tauri::command]
fn set_api_key(provider: ProviderId, key: String) -> Result<(), String> {
    llm::store_api_key(provider, &key).map_err(|e| e.to_string())
}

#[tauri::command]
fn provider_key_status() -> Vec<ProviderKeyStatus> {
    provider_registry()
        .into_iter()
        .map(|p| ProviderKeyStatus {
            id: p.id,
            has_key: !p.requires_api_key || matches!(llm::load_api_key(p.id), Ok(Some(_))),
        })
        .collect()
}

fn resolve_key(provider: ProviderId) -> Result<String, String> {
    llm::resolve_key(provider).map_err(|e| match e {
        convasist_core::CoreError::Llm(msg) if msg == "api_key_missing" => msg,
        other => other.to_string(),
    })
}

/// Settings "Test" button: validates the stored key, returns first-token
/// latency in ms (§4.6).
#[tauri::command]
async fn test_provider(provider: ProviderId, model: String) -> Result<u32, String> {
    let key = resolve_key(provider)?;
    tauri::async_runtime::spawn_blocking(move || {
        llm::validate_key(provider, &key, &model).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_provider_models(provider: ProviderId) -> Result<Vec<ModelInfo>, String> {
    let key = resolve_key(provider)?;
    tauri::async_runtime::spawn_blocking(move || {
        llm::list_models(provider, &key).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ------------------------------------------------------------ RAG library

#[tauri::command]
async fn rag_ingest(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<IngestReport>, String> {
    let store = state.rag.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut reports = Vec::new();
        for path in paths {
            match store.ingest(&path) {
                Ok(report) => reports.push(report),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(reports)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn rag_list(state: State<AppState>) -> Vec<RagDocument> {
    state.rag.list()
}

#[tauri::command]
fn rag_set_enabled(state: State<AppState>, id: String, enabled: bool) -> Result<(), String> {
    state
        .rag
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rag_delete(state: State<AppState>, id: String) -> Result<(), String> {
    state.rag.delete(&id).map_err(|e| e.to_string())
}

/// Build the retrieval query for an assist: the explicit question, or the
/// text of the last few finalized turns (what's being discussed right now).
fn retrieval_query(question: Option<&str>, segments: &[TranscriptSegment]) -> String {
    if let Some(q) = question {
        return q.to_string();
    }
    segments
        .iter()
        .rev()
        .filter(|s| s.is_final)
        .take(4)
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Manual assist (U4/O2): retrieves grounding chunks (R4), builds the
/// context, and streams the answer back as ASSIST_CHUNK events. Returns
/// immediately.
#[tauri::command]
fn assist(
    app: AppHandle,
    state: State<AppState>,
    request_id: String,
    kind: AssistKind,
    question: Option<String>,
    segments: Vec<TranscriptSegment>,
) -> Result<(), String> {
    let selection = state
        .config
        .lock()
        .expect("config lock")
        .llm_quality
        .clone();
    let key = resolve_key(selection.provider)?;

    let query = retrieval_query(question.as_deref(), &segments);
    let chunks = if query.trim().is_empty() {
        Vec::new()
    } else {
        state.rag.retrieve(&query, 8)
    };
    // R5 "peek": tell the UI which sources ground this answer, up front.
    let _ = app.emit(
        events::ASSIST_SOURCES,
        AssistSourcesEvent {
            request_id: request_id.clone(),
            sources: chunks
                .iter()
                .map(|c| AssistSource {
                    file_name: c.file_name.clone(),
                    location: c.location.clone(),
                })
                .collect(),
        },
    );

    let request = build_assist_request(kind, &segments, &chunks, question.as_deref(), 1024);

    std::thread::Builder::new()
        .name("assist".into())
        .spawn(move || {
            let emit = |token: &str, done: bool, error: Option<String>| {
                let _ = app.emit(
                    events::ASSIST_CHUNK,
                    AssistChunkEvent {
                        request_id: request_id.clone(),
                        token: token.to_string(),
                        done,
                        error,
                    },
                );
            };
            let result = llm::stream_completion(
                selection.provider,
                &key,
                &selection.model,
                &request,
                &mut |token| emit(token, false, None),
            );
            match result {
                Ok(()) => emit("", true, None),
                Err(e) => emit("", true, Some(e.to_string())),
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config = load_config(app.handle());
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir must resolve");
            let rag = Arc::new(RagStore::open(&data_dir).expect("open rag store"));
            app.manage(AppState {
                config: Mutex::new(config),
                session: SessionManager::new(),
                rag,
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
            set_api_key,
            provider_key_status,
            test_provider,
            list_provider_models,
            assist,
            rag_ingest,
            rag_list,
            rag_set_enabled,
            rag_delete,
            session_list,
            session_load,
            export_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running convasist");
}
