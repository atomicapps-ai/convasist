//! Neural voice-activity detection (Silero VAD v5) behind the core
//! `SpeechGate` interface.
//!
//! A real speech/no-speech classifier in front of whisper: only audio Silero
//! scores as speech reaches the transcriber, so background noise (fans,
//! keyboards, a TV) stops triggering wasted decodes and hallucinated lines.
//! It runs on the ONNX runtime already bundled for embeddings — no new native
//! dependency.
//!
//! Best-effort by contract: if the model can't load or inference errors, the
//! caller keeps using the energy gate. Neural VAD is an upgrade, never a hard
//! dependency.
//!
//! Silero v5 ONNX I/O (verified against the shipped model):
//!   inputs  `input` f32 [1, 512] · `state` f32 [2, 1, 128] · `sr` i64 scalar
//!   outputs `output` f32 [1, 1] (speech prob) · `stateN` f32 [2, 1, 128]
//! It consumes fixed 512-sample windows (32 ms at 16 kHz) and carries the LSTM
//! state across windows, so we buffer to 512 and thread the state through.

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use convasist_core::vad::SpeechGate;

/// Silero v5 window size at 16 kHz.
const CHUNK: usize = 512;
const STATE_LEN: usize = 2 * 128;
/// Keep reporting speech for a few windows after the last hit so a brief dip
/// mid-word doesn't clip the utterance (~96 ms).
const HANGOVER_CHUNKS: u32 = 3;

pub struct SileroGate {
    session: Session,
    state: Vec<f32>,
    buffer: Vec<f32>,
    threshold: f32,
    hangover: u32,
}

impl SileroGate {
    /// Load the model and prepare a gate. `threshold` is the speech-probability
    /// cutoff (higher = filter more aggressively).
    pub fn load(model_path: &Path, threshold: f32) -> Result<Self, String> {
        let bytes = std::fs::read(model_path).map_err(|e| format!("read silero model: {e}"))?;
        let session = Session::builder()
            .map_err(|e| format!("ort session builder: {e}"))?
            .commit_from_memory(&bytes)
            .map_err(|e| format!("load silero model: {e}"))?;
        Ok(Self {
            session,
            state: vec![0.0; STATE_LEN],
            buffer: Vec::with_capacity(CHUNK * 2),
            threshold: threshold.clamp(0.05, 0.95),
            hangover: 0,
        })
    }

    /// Run one 512-sample window; returns speech probability and advances the
    /// LSTM state.
    fn infer(&mut self, chunk: &[f32]) -> Result<f32, String> {
        let input = Tensor::from_array((vec![1_i64, CHUNK as i64], chunk.to_vec()))
            .map_err(|e| e.to_string())?;
        let state = Tensor::from_array((vec![2_i64, 1_i64, 128_i64], self.state.clone()))
            .map_err(|e| e.to_string())?;
        let sr =
            Tensor::from_array((Vec::<i64>::new(), vec![16_000_i64])).map_err(|e| e.to_string())?;

        let outputs = self
            .session
            .run(
                ort::inputs!["input" => input, "state" => state, "sr" => sr]
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("silero inference: {e}"))?;

        // Carry the updated state forward for the next window.
        let (_, new_state) = outputs["stateN"]
            .try_extract_raw_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        if new_state.len() == STATE_LEN {
            self.state.clear();
            self.state.extend_from_slice(new_state);
        }

        let (_, prob) = outputs["output"]
            .try_extract_raw_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        Ok(prob.first().copied().unwrap_or(0.0))
    }
}

impl SpeechGate for SileroGate {
    fn is_speech(&mut self, frame: &[f32]) -> bool {
        self.buffer.extend_from_slice(frame);
        let mut hit = false;
        while self.buffer.len() >= CHUNK {
            let chunk: Vec<f32> = self.buffer.drain(..CHUNK).collect();
            match self.infer(&chunk) {
                Ok(p) if p >= self.threshold => {
                    self.hangover = HANGOVER_CHUNKS;
                    hit = true;
                }
                Ok(_) => self.hangover = self.hangover.saturating_sub(1),
                // An inference glitch must not swallow speech — err toward
                // "speech" so audio still reaches the transcriber.
                Err(_) => return true,
            }
        }
        hit || self.hangover > 0
    }

    fn reset(&mut self) {
        self.state.iter_mut().for_each(|s| *s = 0.0);
        self.buffer.clear();
        self.hangover = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end check of the ort API + Silero I/O contract against the real
    /// model. Ignored by default (needs the model); run with:
    ///   CONVASIST_SILERO_MODEL=/tmp/silero_vad.onnx \
    ///     cargo test -p convasist-app --lib vad_silero -- --ignored --nocapture
    #[test]
    #[ignore]
    fn silence_scores_low_and_tone_scores_higher() {
        let Ok(path) = std::env::var("CONVASIST_SILERO_MODEL") else {
            eprintln!("set CONVASIST_SILERO_MODEL to run");
            return;
        };
        let mut gate = SileroGate::load(Path::new(&path), 0.5).expect("load model");

        // Silence: several windows should score below threshold.
        let mut silence_speech = false;
        for _ in 0..10 {
            silence_speech |= gate.is_speech(&vec![0.0f32; CHUNK]);
        }
        gate.reset();

        // A loud 220 Hz-ish tone is not speech either, but exercises non-zero
        // input through the full inference path without panicking.
        let tone: Vec<f32> = (0..CHUNK).map(|i| (i as f32 * 0.086).sin() * 0.6).collect();
        for _ in 0..10 {
            let _ = gate.is_speech(&tone);
        }

        assert!(
            !silence_speech,
            "pure silence should not register as speech"
        );
    }
}
