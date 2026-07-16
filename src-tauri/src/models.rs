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
/// The `-q5_1` / `-q8_0` variants are quantized (compressed) checkpoints —
/// smaller downloads and faster CPU decode at a small accuracy cost. All are
/// published as `ggml-<name>.bin` at `MODEL_BASE_URL`.
pub const KNOWN_MODELS: &[(&str, u64)] = &[
    ("tiny.en-q5_1", 33_000_000),
    ("tiny.en", 78_000_000),
    ("base.en-q5_1", 60_000_000),
    ("base.en", 148_000_000),
    ("small.en-q5_1", 190_000_000),
    ("small.en", 488_000_000),
    ("distil-small.en", 340_000_000),
];

/// One selectable ASR model for the Settings picker.
#[derive(serde::Serialize)]
pub struct WhisperModelInfo {
    pub id: String,
    pub label: String,
    pub note: String,
    pub approx_mb: u32,
}

fn approx_mb(id: &str) -> u32 {
    KNOWN_MODELS
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(_, bytes)| (*bytes / 1_000_000) as u32)
        .unwrap_or(0)
}

/// The models offered in the picker, ordered fastest → most accurate.
pub fn catalog() -> Vec<WhisperModelInfo> {
    let entries = [
        (
            "tiny.en-q5_1",
            "Tiny · quantized — fastest",
            "Lowest latency and smallest download. Great for clear speech.",
        ),
        (
            "tiny.en",
            "Tiny — fast",
            "Very low latency, a little more accurate than the quantized tiny.",
        ),
        (
            "base.en-q5_1",
            "Base · quantized — balanced",
            "Base-level accuracy, compressed so it stays quick.",
        ),
        (
            "base.en",
            "Base — accurate",
            "The old default. Noticeably slower on modest CPUs.",
        ),
        (
            "distil-small.en",
            "Distil-small — most accurate",
            "Best accuracy here; wants a stronger CPU.",
        ),
    ];
    entries
        .iter()
        .map(|(id, label, note)| WhisperModelInfo {
            id: (*id).to_string(),
            label: (*label).to_string(),
            note: (*note).to_string(),
            approx_mb: approx_mb(id),
        })
        .collect()
}

static DOWNLOAD_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static SILERO_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Silero VAD v5 model (official upstream). ~2.3 MB.
const SILERO_URL: &str =
    "https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx";

pub fn silero_path(app: &AppHandle) -> Result<PathBuf, CoreError> {
    Ok(models_dir(app)?.join("silero_vad.onnx"))
}

/// Returns the Silero model path when present; otherwise kicks off (at most
/// one) background download and returns `None`. Neural VAD stays off until it
/// lands — the segmenter falls back to the energy gate meanwhile.
pub fn ensure_silero(app: &AppHandle) -> Option<PathBuf> {
    let path = silero_path(app).ok()?;
    if path.exists() {
        return Some(path);
    }
    if SILERO_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let app = app.clone();
        let _ = std::thread::Builder::new()
            .name("silero-download".into())
            .spawn(move || {
                let _ = download_silero(&app);
                SILERO_IN_FLIGHT.store(false, Ordering::SeqCst);
            });
    }
    None
}

fn download_silero(app: &AppHandle) -> Result<(), CoreError> {
    let final_path = silero_path(app)?;
    let part_path = final_path.with_extension("onnx.part");

    let response = ureq::get(SILERO_URL)
        .call()
        .map_err(|e| CoreError::Asr(format!("download silero: {e}")))?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(&part_path).map_err(|e| CoreError::Asr(e.to_string()))?;
    let mut buf = vec![0u8; 1 << 16];
    let mut written: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| CoreError::Asr(format!("silero read: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| CoreError::Asr(e.to_string()))?;
        written += n as u64;
    }
    file.flush().map_err(|e| CoreError::Asr(e.to_string()))?;
    drop(file);

    // Sanity floor (model is ~2.3 MB): reject a truncated/HTML error body.
    if written < 500_000 {
        let _ = fs::remove_file(&part_path);
        return Err(CoreError::Asr(format!(
            "silero download too small ({written} bytes)"
        )));
    }
    fs::rename(&part_path, &final_path).map_err(|e| CoreError::Asr(e.to_string()))?;
    Ok(())
}

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
        if let Some(percent) = (written * 100).checked_div(total).map(|p| p.min(100) as u8) {
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
