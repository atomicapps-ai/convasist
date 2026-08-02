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

use conva_core::asr::{TranscriptSegment, TranscriptionEngine};
use conva_core::audio::{AudioFrame, AudioSource, StreamSide};
use conva_core::config::AppConfig;
use conva_core::dsp::rms_dbfs;
use conva_core::ipc::{events, AudioLevelEvent, RadarEvent, SessionStateEvent};
use conva_core::radar::looks_like_question;
use conva_core::CoreError;

use conva_core::asr::AsrEngineId;

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
    /// For a live Sim Con rehearsal: the flag the "your turn" command sets to
    /// end the user's current turn immediately (the worker also auto-ends on a
    /// pause). Present only while a rehearsal is active.
    rehearsal_force: Mutex<Option<Arc<AtomicBool>>>,
    /// For a rehearsal: a clone of the user-turn sender, so "use a suggested
    /// answer" can inject a typed turn as if the user spoke it.
    rehearsal_inject: Mutex<Option<Sender<TranscriptSegment>>>,
    /// Epoch-ms the current session started — the shared clock for rehearsal
    /// timestamps so persona/injected turns interleave with spoken turns.
    session_started_ms: AtomicU64,
    /// Handle to the current session's transcript log, so rehearsal turns that
    /// bypass the capture sink (persona replies, injected answers) still get
    /// written to the per-session file — the auto-log stays complete.
    session_log: Mutex<Option<Arc<Mutex<fs::File>>>>,
}

/// Which capture topology a session runs.
enum Mode {
    /// Normal live assist: mic (you) + WASAPI loopback (them).
    Live,
    /// Sim Con rehearsal: mic only (the AI is the other party — capturing
    /// system audio would feed its own TTS back in). Finalized user turns are
    /// forwarded to the rehearsal worker via this sender; the Sim Con title
    /// tags the session log so it's identifiable as a rehearsal.
    Rehearsal {
        reh_tx: Sender<TranscriptSegment>,
        simcon_title: String,
    },
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
            rehearsal_force: Mutex::new(None),
            rehearsal_inject: Mutex::new(None),
            session_started_ms: AtomicU64::new(0),
            session_log: Mutex::new(None),
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

    /// Start normal live assist (mic + system-audio loopback).
    pub fn start(
        &self,
        app: &AppHandle,
        config: &AppConfig,
        rag: Arc<RagStore>,
    ) -> Result<String, CoreError> {
        self.start_inner(app, config, rag, Mode::Live)
            .map(|(id, _stop)| id)
    }

    /// Start a Sim Con rehearsal: mic-only capture whose finalized user turns
    /// flow to `reh_tx`. Returns `(session_id, stop_flag, force_end)` — the
    /// caller spawns the rehearsal worker with the first two and keeps the last
    /// for the "your turn" control (also stored here for `rehearsal_your_turn`).
    pub fn start_rehearsal(
        &self,
        app: &AppHandle,
        config: &AppConfig,
        rag: Arc<RagStore>,
        reh_tx: Sender<TranscriptSegment>,
        simcon_title: String,
    ) -> Result<(String, Arc<AtomicBool>, Arc<AtomicBool>), CoreError> {
        // Keep a clone so "use a suggested answer" can inject a typed turn.
        *self.rehearsal_inject.lock().expect("rehearsal lock") = Some(reh_tx.clone());
        let (id, stop_flag) = self.start_inner(
            app,
            config,
            rag,
            Mode::Rehearsal {
                reh_tx,
                simcon_title,
            },
        )?;
        let force_end = Arc::new(AtomicBool::new(false));
        *self.rehearsal_force.lock().expect("rehearsal lock") = Some(force_end.clone());
        Ok((id, stop_flag, force_end))
    }

    /// End the user's current rehearsal turn now (manual "your turn").
    pub fn rehearsal_your_turn(&self) {
        if let Some(f) = self
            .rehearsal_force
            .lock()
            .expect("rehearsal lock")
            .as_ref()
        {
            f.store(true, Ordering::Relaxed);
        }
    }

    /// Epoch-ms the current session started (rehearsal timeline base).
    pub fn session_started_ms(&self) -> u64 {
        self.session_started_ms.load(Ordering::Relaxed)
    }

    /// Append a finalized segment to the current session's transcript log.
    /// Used for rehearsal turns that bypass the capture sink (persona replies,
    /// injected answers) so the per-session auto-log is complete. No-op if no
    /// session is active or the segment isn't a non-empty final.
    pub fn log_segment(&self, segment: &TranscriptSegment) {
        if !segment.is_final || segment.text.trim().is_empty() {
            return;
        }
        if let Some(file) = self.session_log.lock().expect("log lock").as_ref() {
            if let Ok(json) = serde_json::to_string(segment) {
                if let Ok(mut f) = file.lock() {
                    let _ = writeln!(f, "{json}");
                }
            }
        }
    }

    /// Inject a typed user turn into the active rehearsal (from "use a suggested
    /// answer"). Returns false if no rehearsal is running.
    pub fn rehearsal_inject_turn(&self, segment: TranscriptSegment) -> bool {
        let guard = self.rehearsal_inject.lock().expect("rehearsal lock");
        match guard.as_ref() {
            Some(tx) => {
                let _ = tx.send(segment);
                // Answer promptly rather than waiting out the silence timer.
                if let Some(f) = self
                    .rehearsal_force
                    .lock()
                    .expect("rehearsal lock")
                    .as_ref()
                {
                    f.store(true, Ordering::Relaxed);
                }
                true
            }
            None => false,
        }
    }

    fn start_inner(
        &self,
        app: &AppHandle,
        config: &AppConfig,
        rag: Arc<RagStore>,
        mode: Mode,
    ) -> Result<(String, Arc<AtomicBool>), CoreError> {
        {
            let active = self.active.lock().expect("session lock");
            if let Some(existing) = active.as_ref() {
                return Ok((existing.id.clone(), existing.stop_flag.clone()));
            }
        }

        // Session start can take real time (model load; on the first GPU run,
        // minutes of shader compilation). Tell the UI so it can show a
        // loading state instead of a dead screen; Listening replaces it.
        let emit_preparing = |message: String| {
            let _ = app.emit(
                events::SESSION_STATE,
                SessionStateEvent::Preparing { message },
            );
        };
        emit_preparing("Preparing the speech engine…".into());

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
            let cached = {
                let cache = self.whisper_cache.lock().expect("whisper cache lock");
                matches!(cache.as_ref(), Some((p, _)) if *p == model_path.to_string_lossy())
            };
            if !cached {
                emit_preparing(if crate::asr::WHISPER_BACKEND == "cpu" {
                    format!("Loading speech model {}…", config.whisper_model)
                } else {
                    format!(
                        "Loading speech model {} on the GPU — the first run compiles shaders and can take a few minutes…",
                        config.whisper_model
                    )
                });
            }
            whisper_shared = Some(self.load_whisper(&model_path.to_string_lossy())?);
            emit_preparing("Starting audio capture…".into());
        }

        let started_ms = now_unix_ms();
        self.session_started_ms.store(started_ms, Ordering::Relaxed);
        let session_id = format!("session-{started_ms}");
        let stop_flag = Arc::new(AtomicBool::new(false));
        // last-frame clocks (ms since epoch) per side, shared with watchdog.
        let last_frame = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);

        // Per-session transcript file (U3): meta line, then one JSON
        // segment per line. Shared by both sides' sinks. A rehearsal tags the
        // meta with its Sim Con title so the session is identifiable as one.
        let rehearsal_title = match &mode {
            Mode::Rehearsal { simcon_title, .. } => Some(simcon_title.as_str()),
            Mode::Live => None,
        };
        let session_file = Arc::new(Mutex::new(open_session_file(
            app,
            &session_id,
            rehearsal_title,
        )?));
        // Expose the log so rehearsal turns bypassing the sink still get written.
        *self.session_log.lock().expect("log lock") = Some(session_file.clone());

        // Commitment & entity tracker (§6.3): best-effort — only when enabled
        // and the fast-slot provider has a usable key. Runs in rehearsals too so
        // Ally captures key entities/commitments as the practice plays out.
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

        // Rehearsal forwards finalized user turns to the worker; live doesn't.
        let reh_tx = match &mode {
            Mode::Rehearsal { reh_tx, .. } => Some(reh_tx.clone()),
            Mode::Live => None,
        };
        // Rehearsal is mic-only (the AI is the other party); live captures both.
        let sides: Vec<(StreamSide, Option<String>)> = match &mode {
            Mode::Live => vec![
                (StreamSide::Outbound, config.input_device.clone()),
                (StreamSide::Inbound, config.loopback_device.clone()),
            ],
            Mode::Rehearsal { .. } => {
                vec![(StreamSide::Outbound, config.input_device.clone())]
            }
        };

        let mut engines: Vec<Engine> = Vec::new();
        let mut sources = Vec::new();
        for (side, device) in sides {
            let make_sink = || {
                make_transcript_sink(
                    app.clone(),
                    rag.clone(),
                    session_file.clone(),
                    tracker_tx.clone(),
                    if side == StreamSide::Outbound {
                        reh_tx.clone()
                    } else {
                        None
                    },
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
                    "[conva] {side:?}: local whisper '{}', gate={}",
                    config.whisper_model,
                    if vad.silero_model.is_some() {
                        format!("silero (threshold {:.2})", vad.threshold)
                    } else {
                        "energy".to_string()
                    }
                ),
                Engine::Deepgram(_) => {
                    eprintln!("[conva] {side:?}: deepgram cloud streaming")
                }
            }
            let frames_tx = engine.frame_sender()?;

            let mut source = CpalSource::new(side, device);
            if let Err(e) = source.start(make_frame_sink(
                app.clone(),
                last_frame.clone(),
                frames_tx,
                self.recording.clone(),
            )) {
                // The inbound (other-party) side is WASAPI loopback — capturing
                // an *output* device as an input stream. That trick is
                // Windows-only; on macOS/Linux cpal can't do it, so this call
                // fails. Rather than sink the whole session (which read as
                // "Start does nothing" on Mac), degrade to mic-only so the user
                // still gets live transcription of their own side. A native
                // macOS system-audio path (ScreenCaptureKit) is future work.
                match side {
                    StreamSide::Inbound => {
                        eprintln!(
                            "[conva] system-audio (other-party) capture unavailable on this platform ({e}); continuing with your microphone only"
                        );
                        continue;
                    }
                    StreamSide::Outbound => return Err(e),
                }
            }

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

        let stop_ret = stop_flag.clone();
        let mut active = self.active.lock().expect("session lock");
        *active = Some(ActiveSession {
            id: session_id.clone(),
            sources,
            engines,
            stop_flag,
            _tracker_tx: tracker_tx,
        });
        Ok((session_id, stop_ret))
    }

    pub fn stop(&self, app: &AppHandle) -> Result<(), CoreError> {
        // Drop the rehearsal controls (if any) — the worker exits when the
        // capture stops and its channel disconnects.
        *self.rehearsal_force.lock().expect("rehearsal lock") = None;
        *self.rehearsal_inject.lock().expect("rehearsal lock") = None;
        *self.session_log.lock().expect("log lock") = None;
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
    rehearsal_tx: Option<Sender<TranscriptSegment>>,
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
            // Rehearsal: hand finalized user (outbound) turns to the worker.
            if segment.side == StreamSide::Outbound {
                if let Some(reh) = &rehearsal_tx {
                    let _ = reh.send(segment.clone());
                }
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

fn open_session_file(
    app: &AppHandle,
    session_id: &str,
    rehearsal_title: Option<&str>,
) -> Result<fs::File, CoreError> {
    let path = sessions_dir(app)?.join(format!("{session_id}.jsonl"));
    let mut file = fs::File::create(path).map_err(|e| CoreError::Audio(e.to_string()))?;
    let mut meta = serde_json::json!({
        "id": session_id,
        "started_at_unix_ms": now_unix_ms(),
    });
    // Tag rehearsals so the Sessions list can mark them as Sim Cons.
    if let Some(title) = rehearsal_title {
        meta["kind"] = serde_json::Value::String("rehearsal".into());
        meta["simcon_title"] = serde_json::Value::String(title.to_string());
    }
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
    /// True when this session was a Sim Con rehearsal (tagged in its meta).
    pub is_rehearsal: bool,
    /// The Sim Con title, when this was a rehearsal.
    pub simcon_title: Option<String>,
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
            is_rehearsal: meta["kind"].as_str() == Some("rehearsal"),
            simcon_title: meta["simcon_title"].as_str().map(|s| s.to_string()),
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
