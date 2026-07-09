//! First-run ASR model provisioning (design §4.2 T6).
//!
//! Models live in `<app-data>/models/` (gitignored, never bundled). The
//! download runs on a dedicated thread, streams to a `.part` file, emits
//! MODEL_STATUS progress events, and renames atomically on completion so a
//! killed download never leaves a corrupt model behind.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};

use convasist_core::ipc::{events, ModelStatusEvent};
use convasist_core::CoreError;

/// ggml checkpoints published by the whisper.cpp project.
const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Names accepted in `AppConfig.whisper_model`, with approximate sizes.
pub const KNOWN_MODELS: &[(&str, u64)] = &[
    ("tiny.en", 78_000_000),
    ("base.en", 148_000_000),
    ("small.en", 488_000_000),
    ("distil-small.en", 340_000_000),
];

static DOWNLOAD_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

pub fn models_dir(app: &AppHandle) -> Result<PathBuf, CoreError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CoreError::Asr(format!("no app data dir: {e}")))?
        .join("models");
    fs::create_dir_all(&dir).map_err(|e| CoreError::Asr(e.to_string()))?;
    Ok(dir)
}

pub fn model_path(app: &AppHandle, model: &str) -> Result<PathBuf, CoreError> {
    Ok(models_dir(app)?.join(format!("ggml-{model}.bin")))
}

/// Returns the model path when present; otherwise kicks off (at most one)
/// background download and returns `Err(Asr("model_downloading"))` — the
/// UI shows MODEL_STATUS progress and the user retries start.
pub fn ensure_model(app: &AppHandle, model: &str) -> Result<PathBuf, CoreError> {
    if !KNOWN_MODELS.iter().any(|(name, _)| *name == model) {
        return Err(CoreError::Asr(format!("unknown whisper model '{model}'")));
    }
    let path = model_path(app, model)?;
    if path.exists() {
        return Ok(path);
    }

    if DOWNLOAD_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let app = app.clone();
        let model = model.to_string();
        std::thread::Builder::new()
            .name("model-download".into())
            .spawn(move || {
                let result = download(&app, &model);
                DOWNLOAD_IN_FLIGHT.store(false, Ordering::SeqCst);
                let event = match result {
                    Ok(()) => ModelStatusEvent::Ready {
                        model: model.clone(),
                    },
                    Err(e) => ModelStatusEvent::Error {
                        model: model.clone(),
                        message: e.to_string(),
                    },
                };
                let _ = app.emit(events::MODEL_STATUS, event);
            })
            .map_err(|e| CoreError::Asr(format!("spawn download: {e}")))?;
    }

    Err(CoreError::Asr("model_downloading".into()))
}

fn download(app: &AppHandle, model: &str) -> Result<(), CoreError> {
    let url = format!("{MODEL_BASE_URL}/ggml-{model}.bin");
    let final_path = model_path(app, model)?;
    let part_path = final_path.with_extension("bin.part");

    let response = ureq::get(&url)
        .call()
        .map_err(|e| CoreError::Asr(format!("download {model}: {e}")))?;

    let total: u64 = response
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            KNOWN_MODELS
                .iter()
                .find(|(name, _)| *name == model)
                .map(|(_, size)| *size)
        })
        .unwrap_or(0);

    let mut reader = response.into_reader();
    let mut file = fs::File::create(&part_path).map_err(|e| CoreError::Asr(e.to_string()))?;

    let mut buf = vec![0u8; 1 << 20];
    let mut written: u64 = 0;
    let mut last_percent: u8 = 255;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| CoreError::Asr(format!("download read: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| CoreError::Asr(e.to_string()))?;
        written += n as u64;
        if total > 0 {
            let percent = ((written * 100) / total).min(100) as u8;
            if percent != last_percent {
                last_percent = percent;
                let _ = app.emit(
                    events::MODEL_STATUS,
                    ModelStatusEvent::Downloading {
                        model: model.to_string(),
                        percent,
                    },
                );
            }
        }
    }
    file.flush().map_err(|e| CoreError::Asr(e.to_string()))?;
    drop(file);

    // Sanity floor: a truncated/HTML error body must not become a "model".
    if written < 10_000_000 {
        let _ = fs::remove_file(&part_path);
        return Err(CoreError::Asr(format!(
            "download too small ({written} bytes) — not a model file"
        )));
    }

    fs::rename(&part_path, &final_path).map_err(|e| CoreError::Asr(e.to_string()))?;
    Ok(())
}
