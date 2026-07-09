//! Session lifecycle (U3): owns capture sources + per-side ASR engines,
//! meters the streams, and broadcasts typed IPC events.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use convasist_core::asr::TranscriptionEngine;
use convasist_core::audio::{AudioFrame, AudioSource, StreamSide};
use convasist_core::config::AppConfig;
use convasist_core::dsp::rms_dbfs;
use convasist_core::ipc::{events, AudioLevelEvent, SessionStateEvent};
use convasist_core::CoreError;

use crate::asr::{SharedWhisper, WhisperEngine};
use crate::audio::CpalSource;
use crate::models;

/// A stream is unhealthy when no frames arrived for this long (A4 watchdog).
const STALL_AFTER: Duration = Duration::from_millis(1500);
/// Meter emit cadence: one AUDIO_LEVEL event per side per window.
const METER_WINDOW_SAMPLES: usize = 1600; // 100 ms at 16 kHz

pub struct SessionManager {
    active: Mutex<Option<ActiveSession>>,
    /// Loaded whisper weights, cached across sessions (keyed by model path).
    whisper_cache: Mutex<Option<(String, Arc<SharedWhisper>)>>,
}

struct ActiveSession {
    id: String,
    sources: Vec<CpalSource>,
    engines: Vec<WhisperEngine>,
    stop_flag: Arc<AtomicBool>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
            whisper_cache: Mutex::new(None),
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

    pub fn start(&self, app: &AppHandle, config: &AppConfig) -> Result<String, CoreError> {
        {
            let active = self.active.lock().expect("session lock");
            if let Some(existing) = active.as_ref() {
                return Ok(existing.id.clone());
            }
        }

        // Fail fast (before touching audio devices) if the model is absent —
        // ensure_model kicks off the background download (T6).
        let model_path = models::ensure_model(app, &config.whisper_model)?;
        let shared = self.load_whisper(&model_path.to_string_lossy())?;

        let session_id = format!("session-{}", now_unix_ms());
        let stop_flag = Arc::new(AtomicBool::new(false));
        // last-frame clocks (ms since epoch) per side, shared with watchdog.
        let last_frame = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);

        let mut engines = Vec::new();
        let mut sources = Vec::new();
        for (side, device) in [
            (StreamSide::Outbound, config.input_device.clone()),
            (StreamSide::Inbound, config.loopback_device.clone()),
        ] {
            let mut engine = WhisperEngine::new(shared.clone(), side);
            engine.set_sink(make_transcript_sink(app.clone()));
            let frames_tx = engine.frame_sender()?;

            let mut source = CpalSource::new(side, device);
            source.start(make_frame_sink(app.clone(), last_frame.clone(), frames_tx))?;

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
        });
        Ok(session_id)
    }

    pub fn stop(&self, app: &AppHandle) -> Result<(), CoreError> {
        let session = self.active.lock().expect("session lock").take();
        if let Some(mut session) = session {
            session.stop_flag.store(true, Ordering::Relaxed);
            // Stop capture first (drops the frame senders), then let the
            // engines flush their final utterances.
            for source in &mut session.sources {
                source.stop()?;
            }
            for engine in &mut session.engines {
                engine.finish()?;
            }
        }
        app.emit(events::SESSION_STATE, SessionStateEvent::Idle)
            .map_err(|e| CoreError::Audio(e.to_string()))
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
) -> Box<dyn FnMut(AudioFrame) + Send> {
    let mut window: Vec<f32> = Vec::with_capacity(METER_WINDOW_SAMPLES * 2);
    Box::new(move |frame: AudioFrame| {
        last_frame[side_index(frame.side)].store(now_unix_ms(), Ordering::Relaxed);
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

/// Transcript sink: broadcast segments to the UI. Session persistence (U3
/// reopen) attaches here in M2b.
fn make_transcript_sink(
    app: AppHandle,
) -> Box<dyn FnMut(convasist_core::asr::TranscriptSegment) + Send> {
    Box::new(move |segment| {
        let _ = app.emit(events::TRANSCRIPT_SEGMENT, segment);
    })
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
