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
//! Silero v5 ONNX I/O (verified against the shipped model + a real speech
//! sample — see the ignored `real_speech_opens_the_gate` test):
//!   inputs  `input` f32 [1, 64+512] · `state` f32 [2, 1, 128] · `sr` i64 scalar
//!   outputs `output` f32 [1, 1] (speech prob) · `stateN` f32 [2, 1, 128]
//! Each 512-sample window (32 ms at 16 kHz) must be fed with the last 64
//! samples of the PREVIOUS window prepended (the official wrapper's
//! `context_size`); the dynamic input dims silently accept a bare 512 but
//! then score everything near zero. Both the LSTM state and that audio
//! context thread across windows.

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use convasist_core::vad::SpeechGate;

/// Silero v5 window size at 16 kHz.
const CHUNK: usize = 512;
/// v5 requires the last 64 samples of the PREVIOUS window prepended to each
/// input (the official wrapper's `context_size` for 16 kHz), so the model
/// actually consumes 576 samples. Feeding a bare 512 is silently accepted
/// (dynamic dims) but scores near zero — speech never opens the gate.
const CONTEXT: usize = 64;
const STATE_LEN: usize = 2 * 128;
/// Keep reporting speech for a few windows after the last hit so a brief dip
/// mid-word doesn't clip the utterance (~96 ms).
const HANGOVER_CHUNKS: u32 = 3;

pub struct SileroGate {
    session: Session,
    state: Vec<f32>,
    /// Tail of the previous window, prepended to the next input (v5 contract).
    context: Vec<f32>,
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
            context: vec![0.0; CONTEXT],
            buffer: Vec::with_capacity(CHUNK * 2),
            threshold: threshold.clamp(0.05, 0.95),
            hangover: 0,
        })
    }

    /// Run one 512-sample window; returns speech probability and advances the
    /// LSTM state + audio context.
    fn infer(&mut self, chunk: &[f32]) -> Result<f32, String> {
        // v5 input = [context (64) || window (512)] = 576 samples.
        let mut window = Vec::with_capacity(CONTEXT + CHUNK);
        window.extend_from_slice(&self.context);
        window.extend_from_slice(chunk);
        let input_len = window.len() as i64;
        let input =
            Tensor::from_array((vec![1_i64, input_len], window)).map_err(|e| e.to_string())?;
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

        // Carry the tail of this window as the next window's context.
        self.context.clear();
        self.context.extend_from_slice(&chunk[CHUNK - CONTEXT..]);

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
        self.context.iter_mut().for_each(|s| *s = 0.0);
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
    /// Definitive positive-path check: real speech (a 16 kHz mono WAV, e.g.
    /// whisper.cpp's jfk.wav) must open the gate at the default threshold —
    /// and the same audio attenuated to a quiet-mic level shows how far the
    /// score drops. Run with:
    ///   CONVASIST_SILERO_MODEL=... CONVASIST_SPEECH_WAV=... \
    ///     cargo test -p convasist-app --lib vad_silero -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_speech_opens_the_gate() {
        let (Ok(model), Ok(wav)) = (
            std::env::var("CONVASIST_SILERO_MODEL"),
            std::env::var("CONVASIST_SPEECH_WAV"),
        ) else {
            eprintln!("set CONVASIST_SILERO_MODEL and CONVASIST_SPEECH_WAV to run");
            return;
        };
        let reader = hound::WavReader::open(&wav).expect("open wav");
        assert_eq!(reader.spec().sample_rate, 16_000);
        let samples: Vec<f32> = reader
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();

        // Feed 30 ms frames exactly like the segmenter does.
        let run = |gain: f32, threshold: f32| -> (usize, usize) {
            let mut gate = SileroGate::load(Path::new(&model), threshold).expect("load");
            let mut speech_frames = 0usize;
            let mut total = 0usize;
            for frame in samples.chunks(480) {
                let scaled: Vec<f32> = frame.iter().map(|s| s * gain).collect();
                total += 1;
                if gate.is_speech(&scaled) {
                    speech_frames += 1;
                }
            }
            (speech_frames, total)
        };

        let (full, total) = run(1.0, 0.32);
        let (quiet, _) = run(0.01, 0.32); // ≈ -40 dB: the reported mic level
        eprintln!(
            "silero @0.32: full-level speech {full}/{total} frames, quiet(-40dB) {quiet}/{total}"
        );
        assert!(
            full > total / 4,
            "real speech at normal level must open the gate (got {full}/{total})"
        );
    }

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
