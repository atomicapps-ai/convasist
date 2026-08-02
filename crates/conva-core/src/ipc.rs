//! The typed IPC contract between the Rust core and the UI.
//!
//! Event names and payload shapes defined here are hand-mirrored in
//! `src/lib/ipc.ts` on the UI side. If you change anything in this file,
//! change the TypeScript mirror in the same commit (a ts-rs codegen step
//! replaces the hand mirror later in Phase 1).

use serde::{Deserialize, Serialize};

use crate::asr::TranscriptSegment;
use crate::audio::StreamSide;

/// Event channel names (Tauri `emit` topics).
pub mod events {
    /// Payload: [`super::TranscriptSegment`]
    pub const TRANSCRIPT_SEGMENT: &str = "conva://transcript-segment";
    /// Payload: [`super::AudioLevelEvent`]
    pub const AUDIO_LEVEL: &str = "conva://audio-level";
    /// Payload: [`super::SessionStateEvent`]
    pub const SESSION_STATE: &str = "conva://session-state";
    /// Payload: [`super::AllyChunkEvent`]
    pub const ALLY_CHUNK: &str = "conva://ally-chunk";
    /// Payload: [`super::ModelStatusEvent`]
    pub const MODEL_STATUS: &str = "conva://model-status";
    /// Payload: [`super::AllySourcesEvent`]
    pub const ALLY_SOURCES: &str = "conva://ally-sources";
    /// Payload: [`super::RadarEvent`]
    pub const RADAR: &str = "conva://radar";
    /// Payload: [`super::TrackerEvent`]
    pub const TRACKER: &str = "conva://tracker";
    /// Payload: [`super::RehearsalStateEvent`]
    pub const REHEARSAL_STATE: &str = "conva://rehearsal-state";
    /// Payload: `AuthChangedEvent` — defined shell-side in
    /// `src-tauri/src/auth.rs` (next to `AuthStatus`, which never crosses into
    /// core) and mirrored in `src/lib/ipc.ts`. Emitted when an OAuth sign-in
    /// finishes out-of-band via the `conva://auth/callback` deep link.
    pub const AUTH_CHANGED: &str = "conva://auth-changed";
}

/// Re-exported so the IPC module is a one-stop description of the wire.
pub type TranscriptEvent = TranscriptSegment;

/// VU meter + stream-health payload (A4), emitted ~10 Hz per side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioLevelEvent {
    pub side: StreamSide,
    /// RMS level in dBFS (<= 0.0; silence approaches -inf, clamp at -90).
    pub rms_dbfs: f32,
    /// True when the watchdog considers the stream healthy (frames flowing).
    pub healthy: bool,
}

/// Session lifecycle broadcast (U3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SessionStateEvent {
    Idle,
    /// Session start is underway but not yet capturing — model loading, GPU
    /// shader compilation (minutes on the first GPU run), engine connect.
    /// The UI shows a loading state with `message` instead of a dead screen.
    Preparing {
        message: String,
    },
    Listening {
        session_id: String,
        started_at_unix_ms: u64,
    },
    Paused {
        session_id: String,
    },
    Error {
        message: String,
    },
}

/// Which reference chunks grounded an Ally answer (R5 "peek" — emitted
/// once per request, before the first token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllySourcesEvent {
    pub request_id: String,
    pub sources: Vec<AllySource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllySource {
    pub file_name: String,
    pub location: String,
}

/// Question Radar hit (§6.2): the other party asked something the reference
/// library can answer — chunks shown verbatim, zero cost, instantly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadarEvent {
    /// The inbound utterance that triggered the radar.
    pub question: String,
    pub sources: Vec<crate::rag::ScoredChunk>,
}

/// Cumulative tracker state for the live session (§6.3) — the full deduped
/// list, re-emitted after each extraction pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerEvent {
    pub entities: Vec<crate::tracker::TrackedEntity>,
    pub commitments: Vec<crate::tracker::TrackedCommitment>,
}

/// Live Sim Con rehearsal phase (Phase E) — drives the "who's talking" UI
/// (speaking animation + active-speaker indicator).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum RehearsalStateEvent {
    /// Waiting for the user's turn (speak, or use a suggested answer).
    Listening,
    /// Generating the counterparty's reply.
    Thinking,
    /// The counterparty is speaking (TTS playing).
    Speaking,
    /// The rehearsal has ended.
    Ended,
}

/// ASR model provisioning progress (T6 first-run downloader).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ModelStatusEvent {
    Downloading { model: String, percent: u8 },
    Ready { model: String },
    Error { model: String, message: String },
}

/// One streamed piece of an Ally answer (U4/O2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllyChunkEvent {
    /// Correlates chunks to the request that produced them.
    pub request_id: String,
    pub token: String,
    pub done: bool,
    /// Set (with `done: true`) when the request failed mid-stream.
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_serializes_with_tag() {
        let e = SessionStateEvent::Listening {
            session_id: "s1".into(),
            started_at_unix_ms: 123,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["state"], "listening");
        assert_eq!(json["session_id"], "s1");
    }

    #[test]
    fn event_names_are_namespaced() {
        for name in [
            events::TRANSCRIPT_SEGMENT,
            events::AUDIO_LEVEL,
            events::SESSION_STATE,
            events::ALLY_CHUNK,
            events::AUTH_CHANGED,
        ] {
            assert!(name.starts_with("conva://"), "{name}");
        }
    }
}
