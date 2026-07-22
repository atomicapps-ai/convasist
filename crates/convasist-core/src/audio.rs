//! Audio Layer contract (design §4.1).
//!
//! Rules from §2.4 that every implementation must obey:
//! 1. Device callbacks do nothing but copy into a ring buffer — no
//!    allocation, locks, logging, or syscalls on the callback thread.
//! 2. Every frame carries a monotonic capture timestamp.
//! 3. All audio is normalized to 16 kHz mono f32 before leaving this layer.

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// Which side of the conversation a stream belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamSide {
    /// What the user hears — system output / loopback capture.
    Inbound,
    /// What the user says — microphone capture.
    Outbound,
}

/// Sample rate every capture source is resampled to before ASR.
pub const TARGET_SAMPLE_RATE_HZ: u32 = 16_000;

/// A normalized chunk of captured audio.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub side: StreamSide,
    /// Monotonic capture timestamp in nanoseconds (stage-latency accounting).
    pub captured_at_ns: u64,
    /// 16 kHz mono samples in [-1.0, 1.0].
    pub samples: Vec<f32>,
}

/// An enumerable capture device as shown in the device picker (U-layer A3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub side: StreamSide,
    pub is_default: bool,
}

/// A capture source: microphone (outbound) or system loopback (inbound).
///
/// Phase 1 implementation: WASAPI (Windows). The macOS backend arrives in
/// Phase 1.5 behind this same trait.
pub trait AudioSource: Send {
    /// Which side this source captures.
    fn side(&self) -> StreamSide;

    /// Begin capture. `sink` is invoked from a worker thread (never the
    /// device callback thread) with normalized frames.
    fn start(&mut self, sink: Box<dyn FnMut(AudioFrame) + Send>) -> Result<(), CoreError>;

    /// Stop capture and release the device.
    fn stop(&mut self) -> Result<(), CoreError>;
}
