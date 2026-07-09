//! Session lifecycle (U3): owns the two capture sources, meters them, and
//! broadcasts typed IPC events. ASR consumers attach to the same frame path
//! in M2.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use convasist_core::audio::{AudioFrame, AudioSource, StreamSide};
use convasist_core::config::AppConfig;
use convasist_core::dsp::rms_dbfs;
use convasist_core::ipc::{events, AudioLevelEvent, SessionStateEvent};
use convasist_core::CoreError;

use crate::audio::CpalSource;

/// A stream is unhealthy when no frames arrived for this long (A4 watchdog).
const STALL_AFTER: Duration = Duration::from_millis(1500);
/// Meter emit cadence: one AUDIO_LEVEL event per side per window.
const METER_WINDOW_SAMPLES: usize = 1600; // 100 ms at 16 kHz

pub struct SessionManager {
    active: Mutex<Option<ActiveSession>>,
}

struct ActiveSession {
    id: String,
    sources: Vec<CpalSource>,
    stop_flag: Arc<AtomicBool>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    pub fn start(&self, app: &AppHandle, config: &AppConfig) -> Result<String, CoreError> {
        let mut active = self.active.lock().expect("session lock");
        if let Some(existing) = active.as_ref() {
            return Ok(existing.id.clone());
        }

        let session_id = format!("session-{}", now_unix_ms());
        let stop_flag = Arc::new(AtomicBool::new(false));
        // last-frame clocks (ms since epoch) per side, shared with watchdog.
        let last_frame = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);

        let mut sources = Vec::new();
        for (side, device) in [
            (StreamSide::Outbound, config.input_device.clone()),
            (StreamSide::Inbound, config.loopback_device.clone()),
        ] {
            let mut source = CpalSource::new(side, device);
            source.start(make_meter_sink(app.clone(), last_frame.clone()))?;
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

        *active = Some(ActiveSession {
            id: session_id.clone(),
            sources,
            stop_flag,
        });
        Ok(session_id)
    }

    pub fn stop(&self, app: &AppHandle) -> Result<(), CoreError> {
        let mut active = self.active.lock().expect("session lock");
        if let Some(mut session) = active.take() {
            session.stop_flag.store(true, Ordering::Relaxed);
            for source in &mut session.sources {
                source.stop()?;
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

/// Frame sink: accumulates ~100 ms windows, emits one VU meter event per
/// window, and feeds the watchdog clock. M2 tees frames to the ASR worker
/// from this same closure.
fn make_meter_sink(
    app: AppHandle,
    last_frame: Arc<[AtomicU64; 2]>,
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
