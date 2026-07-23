//! convasist Tauri shell — wires the UI to the core layers.
//!
//! M3 state: dual capture (mic + WASAPI loopback) → per-side whisper.cpp
//! transcription → manual AI assist streaming through the provider
//! registry (Claude default), with API keys in the OS credential vault.

mod asr;
mod asr_deepgram;
mod audio;
mod embed;
mod llm;
mod models;
mod rag;
mod recorder;
mod secrets;
mod session;
mod tracker;
mod vad_silero;

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

/// Candidate locations for the repo-committed defaults file. `tauri dev`
/// runs the app with cwd = `src-tauri/`, so the repo root is one level up.
fn repo_config_candidates() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("CONVASIST_CONFIG_FILE") {
        if !p.trim().is_empty() {
            out.push(std::path::PathBuf::from(p));
        }
    }
    out.push(std::path::PathBuf::from("convasist.config.json"));
    out.push(std::path::PathBuf::from("../convasist.config.json"));
    out
}

fn load_config(app: &AppHandle) -> AppConfig {
    if let Some(mut existing) = config_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
    {
        // One-time migration: "base.en" was the pre-quantization seeded
        // default and decodes several times slower than the current default —
        // configs still carrying it are stale defaults, not a user choice.
        // (Explicitly picking base.en-q5_1 or any other model is respected.)
        if existing.whisper_model == "base.en" {
            let new_default = AppConfig::default().whisper_model;
            eprintln!(
                "[convasist] migrating whisper model 'base.en' (stale default) -> '{new_default}'"
            );
            existing.whisper_model = new_default;
            let _ = persist_config(app, &existing);
        }
        return existing;
    }
    // Fresh machine: seed from the repo-committed defaults so tuned settings
    // travel via git (owner request). Falls back to compiled defaults.
    for candidate in repo_config_candidates() {
        if let Some(config) = fs::read_to_string(&candidate)
            .ok()
            .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
        {
            eprintln!("[convasist] seeded config from {}", candidate.display());
            let _ = persist_config(app, &config);
            return config;
        }
    }
    AppConfig::default()
}

/// Write the current config as pretty JSON to `path` — meant for committing
/// `convasist.config.json` to the repo as the cross-machine defaults.
#[tauri::command]
fn export_config(state: State<AppState>, path: String) -> Result<(), String> {
    let config = state.config.lock().expect("config lock").clone();
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Load a config file, apply it as the live config, and persist it.
#[tauri::command]
fn import_config(
    app: AppHandle,
    state: State<AppState>,
    path: String,
) -> Result<AppConfig, String> {
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let config: AppConfig = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    persist_config(&app, &config)?;
    *state.config.lock().expect("config lock") = config.clone();
    Ok(config)
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

/// The selectable speech-to-text models (Settings picker) — fastest first.
#[tauri::command]
fn list_whisper_models() -> Vec<models::WhisperModelInfo> {
    models::catalog()
}

/// Store (or clear, with an empty string) the Deepgram API key in the OS
/// vault. Enables the cloud streaming engine (Settings → engine).
#[tauri::command]
fn set_deepgram_key(key: String) -> Result<(), String> {
    asr_deepgram::store_api_key(&key).map_err(|e| e.to_string())
}

#[tauri::command]
fn deepgram_key_status() -> bool {
    asr_deepgram::load_api_key().is_some()
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

/// Start recording the live call to a stereo WAV; returns the file path.
#[tauri::command]
fn start_recording(app: AppHandle, state: State<AppState>) -> Result<String, String> {
    state
        .session
        .start_recording(&app)
        .map_err(|e| e.to_string())
}

/// Stop the current recording; returns the saved file path (if any).
#[tauri::command]
fn stop_recording(state: State<AppState>) -> Result<Option<String>, String> {
    state.session.stop_recording().map_err(|e| e.to_string())
}

#[tauri::command]
fn recording_status(state: State<AppState>) -> bool {
    state.session.is_recording()
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

/// Ingest text pasted from the clipboard as a `.txt` document (U5). The name
/// is a display label; the store persists it like any ingested file.
#[tauri::command]
async fn rag_ingest_text(
    state: State<'_, AppState>,
    name: String,
    text: String,
) -> Result<IngestReport, String> {
    let store = state.rag.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.ingest_text(&name, &text).map_err(|e| e.to_string())
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

/// Download a library document back to `dest` (chosen via the save dialog):
/// the original uploaded file when retained, else its reconstructed text.
#[tauri::command]
async fn rag_download(state: State<'_, AppState>, id: String, dest: String) -> Result<(), String> {
    let store = state.rag.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.export_original(&id, &dest).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// -------------------------------------------------- Portable encrypted secrets

#[derive(Serialize)]
struct SecretsStatus {
    passphrase_set: bool,
    file_present: bool,
    file_path: String,
    passphrase_env: String,
}

#[tauri::command]
fn secrets_status() -> SecretsStatus {
    let path = secrets::default_path();
    SecretsStatus {
        passphrase_set: secrets::passphrase_set(),
        file_present: path.exists(),
        file_path: path.display().to_string(),
        passphrase_env: secrets::PASSPHRASE_ENV.to_string(),
    }
}

/// Encrypt the stored API keys to a file safe to commit to git. `dest` comes
/// from the save dialog; falls back to the default path when omitted.
#[tauri::command]
async fn secrets_export(dest: Option<String>) -> Result<String, String> {
    let path = dest
        .map(std::path::PathBuf::from)
        .unwrap_or_else(secrets::default_path);
    tauri::async_runtime::spawn_blocking(move || {
        secrets::export_to(&path).map(|n| format!("Encrypted {n} key(s) → {}", path.display()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Decrypt a secrets file and load its keys into the OS vault.
#[tauri::command]
async fn secrets_import(src: Option<String>, overwrite: bool) -> Result<String, String> {
    let path = src
        .map(std::path::PathBuf::from)
        .unwrap_or_else(secrets::default_path);
    tauri::async_runtime::spawn_blocking(move || {
        secrets::import_from(&path, overwrite).map(|n| format!("Loaded {n} key(s) from the file"))
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Filter for whisper.cpp/ggml native logs (routed through the `log` crate by
/// `whisper_rs::install_logging_hooks`). Dev builds of whisper.cpp are
/// compiled with -DWHISPER_DEBUG and emit hundreds of per-token debug lines
/// per utterance — slow enough on the Windows console to tax every decode.
/// Keep info+ from whisper/ggml (model load, GPU device pick), warn+ from
/// anything else, and drop debug/trace entirely.
struct AsrLogFilter;

impl log::Log for AsrLogFilter {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        if meta.target().starts_with("whisper_rs") {
            meta.level() <= log::Level::Info
        } else {
            meta.level() <= log::Level::Warn
        }
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}

static ASR_LOG_FILTER: AsrLogFilter = AsrLogFilter;

pub fn run() {
    // Install before anything touches whisper/ggml so their C-side stderr
    // chatter is level-filtered from the first model load.
    if log::set_logger(&ASR_LOG_FILTER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    whisper_rs::install_logging_hooks();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config = load_config(app.handle());
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir must resolve");
            let rag = Arc::new(RagStore::open(&data_dir).expect("open rag store"));

            // Seed API keys from a committed encrypted secrets file when the
            // passphrase env var is set (fills only missing keys). Lets keys
            // travel to another machine via git without re-entering them.
            secrets::seed_on_startup();

            // Warm the embedding model off the critical path (first run
            // downloads ~130 MB), then embed any chunks ingested before it
            // was ready. Retrieval degrades to BM25-only until this lands.
            {
                let rag = rag.clone();
                let cache_dir = data_dir.join("models");
                let _ = std::thread::Builder::new()
                    .name("embed-warm".into())
                    .spawn(move || {
                        embed::warm(cache_dir);
                        rag.backfill_embeddings();
                    });
            }

            // Fetch the neural-VAD model in the background so it's ready for
            // the first session (falls back to the energy gate until it lands).
            let _ = models::ensure_silero(app.handle());

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
            export_config,
            import_config,
            get_provider_registry,
            list_audio_devices,
            list_whisper_models,
            set_deepgram_key,
            deepgram_key_status,
            start_session,
            stop_session,
            start_recording,
            stop_recording,
            recording_status,
            set_api_key,
            provider_key_status,
            test_provider,
            list_provider_models,
            assist,
            rag_ingest,
            rag_ingest_text,
            rag_list,
            rag_set_enabled,
            rag_delete,
            rag_download,
            secrets_status,
            secrets_export,
            secrets_import,
            session_list,
            session_load,
            export_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running convasist");
}
