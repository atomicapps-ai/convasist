//! Transcription Layer contract (design §4.2).
//!
//! `TranscriptionEngine` is the pluggable seam: local whisper.cpp is the
//! default implementation, cloud engines (Deepgram streaming) are opt-in.
//! Selecting an engine is a config choice, never an architecture change.

use serde::{Deserialize, Serialize};

use crate::audio::{AudioFrame, StreamSide};
use crate::CoreError;

/// A partial or final transcription segment (IPC-visible; §4.2 T4 metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub side: StreamSide,
    /// Monotonic per-side sequence number. A final segment replaces all
    /// partials that carried the same `seq`.
    pub seq: u64,
    pub text: String,
    /// Partial (still mutating) vs final (settled) — drives the UI "settle"
    /// treatment (§5.1 principle 3).
    pub is_final: bool,
    /// Audio-timeline bounds of this segment, in milliseconds from session
    /// start.
    pub start_ms: u64,
    pub end_ms: u64,
    /// Engine confidence in [0.0, 1.0] where the engine reports one.
    pub confidence: Option<f32>,
    /// Measured capture→emit latency in milliseconds (per-stage HUD, §2.4
    /// rule 3).
    pub latency_ms: u32,
}

/// Identifies a transcription engine implementation in config and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrEngineId {
    /// Local whisper.cpp — the default.
    WhisperLocal,
    /// Deepgram streaming WebSocket — cloud opt-in.
    DeepgramCloud,
}

/// A streaming speech-to-text engine bound to one side of the conversation.
/// The two sides run independent engine instances so neither stream can
/// head-of-line block the other (§4.2 T3).
pub trait TranscriptionEngine: Send {
    fn id(&self) -> AsrEngineId;

    /// Feed captured audio. Implementations buffer internally (VAD-gated
    /// chunking) and emit segments through `sink` as they become available.
    fn feed(&mut self, frame: AudioFrame) -> Result<(), CoreError>;

    /// Register the segment sink. Called once before the first `feed`.
    fn set_sink(&mut self, sink: Box<dyn FnMut(TranscriptSegment) + Send>);

    /// Flush any buffered audio as final segments (session stop).
    fn finish(&mut self) -> Result<(), CoreError>;
}
