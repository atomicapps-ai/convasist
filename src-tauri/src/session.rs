//! Session lifecycle (U3): owns capture sources + per-side ASR engines,
//! meters the streams, persists finalized segments to a per-session JSONL
//! file, fires the Question Radar (§6.2), and broadcasts typed IPC events.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager};

use convasist_core::asr::{TranscriptSegment, TranscriptionEngine};
use convasist_core::audio::{AudioFrame, AudioSource, StreamSide};
use convasist_core::config::AppConfig;
use convasist_core::dsp::rms_dbfs;
use convasist_core::ipc::{events, AudioLevelEvent, RadarEvent, SessionStateEvent};
use convasist_core::radar::looks_like_question;
use convasist_core::CoreError;

use convasist_core::asr::AsrEngineId;

use crate::asr::{SharedWhisper, VadSetup, WhisperEngine};
use crate::asr_deepgram::DeepgramEngine;
use crate::audio::CpalSource;
use crate::models;
use crate::rag::RagStore;
use crate::recorder::Recorder;

/// Either transcription engine behind one session-facing surface.
enum Engine {
    Whisper(WhisperEngine),
    Deepgram(DeepgramEngine),
}

impl Engine {
    // Sinks are set on the concrete engines before wrapping, so the enum
    // only needs the two session-lifecycle calls.
    fn frame_sender(&mut self) -> Result<Sender<AudioFrame>, CoreError> {
        match self {
            Engine::Whisper(e) => e.frame_sender(),
            Engine::Deepgram(e) => e.frame_sender(),
        }
    }

    fn finish(&mut self) -> Result<(), CoreError> {
        match self {
            Engine::Whisper(e) => e.finish(),
            Engine::Deepgram(e) => e.finish(),
        }
    }
}

/// A stream is unhealthy when no frames arrived for this long (A4 watchdog).
const STALL_AFTER: Duration = Duration::from_millis(1500);
/// Meter emit cadence: one AUDIO_LEVEL event per side per window.
const METER_WINDOW_SAMPLES: usize = 1600; // 100 ms at 16 kHz

pub struct SessionManager {
    active: Mutex<Option<ActiveSession>>,
    /// Loaded whisper weights, cached across sessions (keyed by model path).
    whisper_cache: Mutex<Option<(String, Arc<SharedWhisper>)>>,
    /// The in-progress call recording, if any. Shared with both frame sinks
    /// so they can tee audio to it while it's armed.
    recording: Arc<Mutex<Option<Recorder>>>,
}

struct ActiveSession {
    id: String,
    sources: Vec<CpalSource>,
    engines: Vec<Engine>,
    stop_flag: Arc<AtomicBool>,
    /// Held so the tracker worker lives with the session; dropping it (on
    /// stop) triggers the tracker's final pass and shutdown.
    _tracker_tx: Option<Sender<TranscriptSegment>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
            whisper_cache: Mutex::new(None),
            recording: Arc::new(Mutex::new(None)),
        }
    }

    fn load_whisper(&self, model_path: &str) -> Result<Arc<SharedWhisper>, CoreError> {
        let mut cache = self.whisper_cache.lock().expect("whisper cache lock");
        if let Some((cached_path, shared)) = cache.as_ref() {
            if cached_path == model_path {
                return Ok(shared.clone());
            }
        }
        let shared = SharedWhisper::load(model_path)?;
        *cache = Some((model_path.to_string(), shared.clone()));
        Ok(shared)
    }

    pub fn start(
        &self,
        app: &AppHandle,
        config: &AppConfig,
        rag: Arc<RagStore>,
    ) -> Result<String, CoreError> {
        {
            let active = self.active.lock().expect("session lock");
            if let Some(existing) = active.as_ref() {
                return Ok(existing.id.clone());
            }
        }

        // Engine choice: Deepgram cloud streaming when opted in and a key is
        // stored (conversation-speed interims, ~100–300 ms); local whisper
        // otherwise. Whisper stays the fallback if the cloud connect fails.
        let deepgram_key = if config.asr_engine == AsrEngineId::DeepgramCloud {
            crate::asr_deepgram::load_api_key()
        } else {
            None
        };
        // Fail fast (before touching audio devices) if the whisper model is
        // absent — ensure_model kicks off the background download (T6). With
        // Deepgram active, whisper (and its download) is skipped entirely.
        let mut whisper_shared: Option<Arc<SharedWhisper>> = None;
        if deepgram_key.is_none() {
            let model_path = models::ensure_model(app, &config.whisper_model)?;
            whisper_shared = Some(self.load_whisper(&model_path.to_string_lossy())?);
        }

        let session_id = format!("session-{}", now_unix_ms());
        let stop_flag = Arc::new(AtomicBool::new(false));
        // last-frame clocks (ms since epoch) per side, shared with watchdog.
        let last_frame = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);

        // Per-session transcript file (U3): meta line, then one JSON
        // segment per line. Shared by both sides' sinks.
        let session_file = Arc::new(Mutex::new(open_session_file(app, &session_id)?));

        // Commitment & entity tracker (§6.3): best-effort — only when
        // enabled and the fast-slot provider has a usable key.
        let tracker_tx = if config.tracker_enabled {
            let selection = config.fast_selection().clone();
            crate::llm::resolve_key(selection.provider)
                .ok()
                .map(|key| crate::tracker::spawn_tracker(app.clone(), selection, key))
        } else {
            None
        };

        // Neural VAD (Silero) when enabled and the model is present; the
        // segmenter falls back to the energy gate otherwise. Sensitivity maps
        // to a speech-probability cutoff (higher = filter more noise).
        let vad = VadSetup {
            silero_model: if config.vad_neural {
                models::ensure_silero(app)
            } else {
                None
            },
            threshold: 0.2 + config.vad_sensitivity.clamp(0.0, 1.0) * 0.5,
        };

        let mut engines: Vec<Engine> = Vec::new();
        let mut sources = Vec::new();
        for (side, device) in [
            (StreamSide::Outbound, config.input_device.clone()),
            (StreamSide::Inbound, config.loopback_device.clone()),
        ] {
            let make_sink = || {
                make_transcript_sink(
                    app.clone(),
                    rag.clone(),
                    session_file.clone(),
                    tracker_tx.clone(),
                )
            };

            let mut engine = match &deepgram_key {
                Some(key) => {
                    let mut dg = DeepgramEngine::new(side, key.clone());
                    dg.set_sink(make_sink());
                    // The connect happens here; a bad key / no network falls
                    // back to local whisper so the session still starts.
                    match dg.frame_sender() {
                        Ok(_) => Engine::Deepgram(dg),
                        Err(e) => {
                            eprintln!("deepgram unavailable ({e}); using local whisper");
                            let shared = match &whisper_shared {
                                Some(s) => s.clone(),
                                None => {
                                    let model_path =
                                        models::ensure_model(app, &config.whisper_model)?;
                                    let s = self.load_whisper(&model_path.to_string_lossy())?;
                                    whisper_shared = Some(s.clone());
                                    s
                                }
                            };
                            let mut w =
                                WhisperEngine::new(shared, side, stop_flag.clone(), vad.clone());
                            w.set_sink(make_sink());
                            Engine::Whisper(w)
                        }
                    }
                }
                None => {
                    let shared = whisper_shared
                        .clone()
                        .expect("whisper loaded when deepgram is off");
                    let mut w = WhisperEngine::new(shared, side, stop_flag.clone(), vad.clone());
                    w.set_sink(make_sink());
                    Engine::Whisper(w)
                }
            };
            // One line per side in the dev console so "why is there no
            // text" is answerable at a glance: engine + speech gate in use.
            match &engine {
                Engine::Whisper(_) => eprintln!(
                    "[convasist] {side:?}: local whisper '{}', gate={}",
                    config.whisper_model,
                    if vad.silero_model.is_some() {
                        format!("silero (threshold {:.2})", vad.threshold)
                    } else {
                        "energy".to_string()
                    }
                ),
                Engine::Deepgram(_) => {
                    eprintln!("[convasist] {side:?}: deepgram cloud streaming")
                }
            }
            let frames_tx = engine.frame_sender()?;

            let mut source = CpalSource::new(side, device);
            source.start(make_frame_sink(
                app.clone(),
                last_frame.clone(),
                frames_tx,
                self.recording.clone(),
            ))?;

            engines.push(engine);
            sources.push(source);
        }

        spawn_watchdog(app.clone(), stop_flag.clone(), last_frame);

        app.emit(
            events::SESSION_STATE,
            SessionStateEvent::Listening {
                session_id: session_id.clone(),
                started_at_unix_ms: now_unix_ms(),
            },
        )
        .map_err(|e| CoreError::Audio(e.to_string()))?;

        let mut active = self.active.lock().expect("session lock");
        *active = Some(ActiveSession {
            id: session_id.clone(),
            sources,
            engines,
            stop_flag,
            _tracker_tx: tracker_tx,
        });
        Ok(session_id)
    }

    pub fn stop(&self, app: &AppHandle) -> Result<(), CoreError> {
        let session = self.active.lock().expect("session lock").take();
        if let Some(mut session) = session {
            // Signal first so the ASR workers skip their final decode.
            session.stop_flag.store(true, Ordering::Relaxed);
            // Release the audio devices synchronously (fast — so a fresh
            // session can reopen them immediately) and finalize any recording.
            for source in &mut session.sources {
                source.stop()?;
            }
            if let Some(rec) = self.recording.lock().expect("recording lock").take() {
                let _ = rec.stop();
            }
            // The ASR worker joins can block on an in-flight whisper decode;
            // wind them down off the caller's thread so Stop returns now and
            // the UI flips to Idle immediately.
            std::thread::Builder::new()
                .name("session-teardown".into())
                .spawn(move || {
                    for engine in &mut session.engines {
                        let _ = engine.finish();
                    }
                })
                .map_err(|e| CoreError::Audio(format!("spawn teardown: {e}")))?;
        }
        app.emit(events::SESSION_STATE, SessionStateEvent::Idle)
            .map_err(|e| CoreError::Audio(e.to_string()))
    }

    /// Start recording the live conversation to a stereo WAV (you = left,
    /// them = right). Requires an active session; returns the file path.
    /// Idempotent — a second call while recording returns the same path.
    pub fn start_recording(&self, app: &AppHandle) -> Result<String, CoreError> {
        if self.active.lock().expect("session lock").is_none() {
            return Err(CoreError::Audio("start listening before recording".into()));
        }
        let mut guard = self.recording.lock().expect("recording lock");
        if let Some(rec) = guard.as_ref() {
            return Ok(rec.path().display().to_string());
        }
        let path = recordings_dir(app)?.join(format!("call-{}.wav", now_unix_ms()));
        let rec = Recorder::start(path)?;
        let out = rec.path().display().to_string();
        *guard = Some(rec);
        Ok(out)
    }

    /// Finalize the current recording, if any. Returns its path.
    pub fn stop_recording(&self) -> Result<Option<String>, CoreError> {
        let rec = self.recording.lock().expect("recording lock").take();
        Ok(rec.map(|r| r.stop().display().to_string()))
    }

    pub fn is_recording(&self) -> bool {
        self.recording.lock().expect("recording lock").is_some()
    }
}

fn side_index(side: StreamSide) -> usize {
    match side {
        StreamSide::Inbound => 0,
        StreamSide::Outbound => 1,
    }
}

/// Audio-frame sink: meters ~100 ms windows (VU events), feeds the watchdog
/// clock, and tees every frame into the side's ASR engine.
fn make_frame_sink(
    app: AppHandle,
    last_frame: Arc<[AtomicU64; 2]>,
    frames_tx: Sender<AudioFrame>,
    recording: Arc<Mutex<Option<Recorder>>>,
) -> Box<dyn FnMut(AudioFrame) + Send> {
    let mut window: Vec<f32> = Vec::with_capacity(METER_WINDOW_SAMPLES * 2);
    Box::new(move |frame: AudioFrame| {
        last_frame[side_index(frame.side)].store(now_unix_ms(), Ordering::Relaxed);
        // Tee to the call recording when armed (cheap copy + channel send;
        // the writer thread does the encoding). Never blocks capture.
        if let Ok(guard) = recording.lock() {
            if let Some(rec) = guard.as_ref() {
                rec.push(frame.side, &frame.samples);
            }
        }
        window.extend_from_slice(&frame.samples);
        if window.len() >= METER_WINDOW_SAMPLES {
            let _ = app.emit(
                events::AUDIO_LEVEL,
                AudioLevelEvent {
                    side: frame.side,
                    rms_dbfs: rms_dbfs(&window),
                    healthy: true,
                },
            );
            window.clear();
        }
        let _ = frames_tx.send(frame);
    })
}

/// Transcript sink: broadcast segments to the UI, persist finals to the
/// session file (U3), and fire the Question Radar on inbound questions
/// (§6.2 — verbatim reference chunks, zero LLM cost).
fn make_transcript_sink(
    app: AppHandle,
    rag: Arc<RagStore>,
    session_file: Arc<Mutex<fs::File>>,
    tracker_tx: Option<Sender<TranscriptSegment>>,
) -> Box<dyn FnMut(TranscriptSegment) + Send> {
    Box::new(move |segment| {
        if segment.is_final {
            if let Ok(json) = serde_json::to_string(&segment) {
                if let Ok(mut file) = session_file.lock() {
                    let _ = writeln!(file, "{json}");
                }
            }
            if let Some(tracker) = &tracker_tx {
                let _ = tracker.send(segment.clone());
            }
            if segment.side == StreamSide::Inbound && looks_like_question(&segment.text) {
                let sources = rag.retrieve(&segment.text, 3);
                if !sources.is_empty() {
                    let _ = app.emit(
                        events::RADAR,
                        RadarEvent {
                            question: segment.text.clone(),
                            sources,
                        },
                    );
                }
            }
        }
        let _ = app.emit(events::TRANSCRIPT_SEGMENT, segment);
    })
}

fn sessions_dir(app: &AppHandle) -> Result<PathBuf, CoreError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CoreError::Audio(format!("no app data dir: {e}")))?
        .join("sessions");
    fs::create_dir_all(&dir).map_err(|e| CoreError::Audio(e.to_string()))?;
    Ok(dir)
}

fn recordings_dir(app: &AppHandle) -> Result<PathBuf, CoreError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CoreError::Audio(format!("no app data dir: {e}")))?
        .join("recordings");
    fs::create_dir_all(&dir).map_err(|e| CoreError::Audio(e.to_string()))?;
    Ok(dir)
}

fn open_session_file(app: &AppHandle, session_id: &str) -> Result<fs::File, CoreError> {
    let path = sessions_dir(app)?.join(format!("{session_id}.jsonl"));
    let mut file = fs::File::create(path).map_err(|e| CoreError::Audio(e.to_string()))?;
    let meta = serde_json::json!({
        "id": session_id,
        "started_at_unix_ms": now_unix_ms(),
    });
    writeln!(file, "{meta}").map_err(|e| CoreError::Audio(e.to_string()))?;
    Ok(file)
}

/// Past-session catalog entry (U3 sessions list).
#[derive(serde::Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub started_at_unix_ms: u64,
    pub segment_count: u32,
    /// First few words of the conversation, for the list.
    pub preview: String,
}

pub fn list_sessions(app: &AppHandle) -> Result<Vec<SessionSummary>, CoreError> {
    let dir = sessions_dir(app)?;
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&dir)
        .map_err(|e| CoreError::Audio(e.to_string()))?
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let mut lines = content.lines();
        let Some(meta) = lines
            .next()
            .and_then(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        else {
            continue;
        };
        let segments: Vec<TranscriptSegment> =
            lines.filter_map(|l| serde_json::from_str(l).ok()).collect();
        let preview: String = segments
            .first()
            .map(|s| s.text.chars().take(60).collect())
            .unwrap_or_default();
        sessions.push(SessionSummary {
            id: meta["id"].as_str().unwrap_or_default().to_string(),
            started_at_unix_ms: meta["started_at_unix_ms"].as_u64().unwrap_or(0),
            segment_count: segments.len() as u32,
            preview,
        });
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.started_at_unix_ms));
    Ok(sessions)
}

pub fn load_session(app: &AppHandle, id: &str) -> Result<Vec<TranscriptSegment>, CoreError> {
    // ids are generated by us, but never trust them as path components.
    if id.contains(['/', '\\', '.']) {
        return Err(CoreError::Audio("invalid session id".into()));
    }
    let path = sessions_dir(app)?.join(format!("{id}.jsonl"));
    let content = fs::read_to_string(path).map_err(|e| CoreError::Audio(e.to_string()))?;
    Ok(content
        .lines()
        .skip(1) // meta line
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// Emits `healthy: false` meter events for any side whose frames stall
/// ("mic went dead" warning, A4).
fn spawn_watchdog(app: AppHandle, stop: Arc<AtomicBool>, last_frame: Arc<[AtomicU64; 2]>) {
    std::thread::Builder::new()
        .name("audio-watchdog".into())
        .spawn(move || {
            let sides = [StreamSide::Inbound, StreamSide::Outbound];
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                let now = now_unix_ms();
                for side in sides {
                    let last = last_frame[side_index(side)].load(Ordering::Relaxed);
                    if last != 0 && now.saturating_sub(last) > STALL_AFTER.as_millis() as u64 {
                        let _ = app.emit(
                            events::AUDIO_LEVEL,
                            AudioLevelEvent {
                                side,
                                rms_dbfs: -90.0,
                                healthy: false,
                            },
                        );
                    }
                }
            }
        })
        .expect("spawn watchdog");
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
