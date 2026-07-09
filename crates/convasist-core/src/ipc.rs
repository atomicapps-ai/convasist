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
    pub const TRANSCRIPT_SEGMENT: &str = "convasist://transcript-segment";
    /// Payload: [`super::AudioLevelEvent`]
    pub const AUDIO_LEVEL: &str = "convasist://audio-level";
    /// Payload: [`super::SessionStateEvent`]
    pub const SESSION_STATE: &str = "convasist://session-state";
    /// Payload: [`super::AssistChunkEvent`]
    pub const ASSIST_CHUNK: &str = "convasist://assist-chunk";
    /// Payload: [`super::ModelStatusEvent`]
    pub const MODEL_STATUS: &str = "convasist://model-status";
    /// Payload: [`super::AssistSourcesEvent`]
    pub const ASSIST_SOURCES: &str = "convasist://assist-sources";
    /// Payload: [`super::RadarEvent`]
    pub const RADAR: &str = "convasist://radar";
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

/// Which reference chunks grounded an assist answer (R5 "peek" — emitted
/// once per request, before the first token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistSourcesEvent {
    pub request_id: String,
    pub sources: Vec<AssistSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistSource {
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

/// ASR model provisioning progress (T6 first-run downloader).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ModelStatusEvent {
    Downloading { model: String, percent: u8 },
    Ready { model: String },
    Error { model: String, message: String },
}

/// One streamed piece of an AI assist answer (U4/O2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistChunkEvent {
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
            events::ASSIST_CHUNK,
        ] {
            assert!(name.starts_with("convasist://"), "{name}");
        }
    }
}
