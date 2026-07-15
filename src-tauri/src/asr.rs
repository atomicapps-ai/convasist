//! Local whisper.cpp transcription engine (design §4.2 T1).
//!
//! One shared `WhisperContext` (the model weights, loaded once) serves both
//! conversation sides through independent `WhisperState`s, so the two
//! streams decode concurrently without doubling memory.
//!
//! Streaming model: the core `UtteranceSegmenter` (energy VAD) bounds each
//! utterance. While an utterance runs, its accumulated window is re-decoded
//! every ~1.2 s and emitted as a partial (`is_final: false`, same `seq`);
//! when it closes, one last decode emits the final segment that replaces
//! the partials in the UI.

use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use convasist_core::asr::{AsrEngineId, TranscriptSegment, TranscriptionEngine};
use convasist_core::audio::{AudioFrame, StreamSide, TARGET_SAMPLE_RATE_HZ};
use convasist_core::vad::{SegmentEvent, SegmenterConfig, UtteranceSegmenter};
use convasist_core::CoreError;

/// Loaded model shared by both per-side engines.
pub struct SharedWhisper {
    context: WhisperContext,
}

impl SharedWhisper {
    pub fn load(model_path: &str) -> Result<Arc<Self>, CoreError> {
        let context =
            WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
                .map_err(|e| CoreError::Asr(format!("load model: {e}")))?;
        Ok(Arc::new(Self { context }))
    }
}

pub struct WhisperEngine {
    tx: Option<Sender<AudioFrame>>,
    worker: Option<JoinHandle<()>>,
    sink: Option<Box<dyn FnMut(TranscriptSegment) + Send>>,
    shared: Arc<SharedWhisper>,
    side: StreamSide,
}

impl WhisperEngine {
    pub fn new(shared: Arc<SharedWhisper>, side: StreamSide) -> Self {
        Self {
            tx: None,
            worker: None,
            sink: None,
            shared,
            side,
        }
    }

    /// Start the decode worker and return a cloneable frame sender for the
    /// audio sink to push into. Dropping every sender (via `finish`) flushes
    /// the open utterance and stops the worker.
    pub fn frame_sender(&mut self) -> Result<Sender<AudioFrame>, CoreError> {
        if let Some(tx) = &self.tx {
            return Ok(tx.clone());
        }
        let sink = self
            .sink
            .take()
            .ok_or_else(|| CoreError::Asr("sink not set before start".into()))?;
        let (tx, rx) = mpsc::channel::<AudioFrame>();
        let shared = self.shared.clone();
        let side = self.side;

        let worker = std::thread::Builder::new()
            .name(format!("asr-{side:?}"))
            .spawn(move || decode_loop(shared, side, rx, sink))
            .map_err(|e| CoreError::Asr(format!("spawn asr worker: {e}")))?;

        self.tx = Some(tx.clone());
        self.worker = Some(worker);
        Ok(tx)
    }
}

impl TranscriptionEngine for WhisperEngine {
    fn id(&self) -> AsrEngineId {
        AsrEngineId::WhisperLocal
    }

    fn set_sink(&mut self, sink: Box<dyn FnMut(TranscriptSegment) + Send>) {
        self.sink = Some(sink);
    }

    fn feed(&mut self, frame: AudioFrame) -> Result<(), CoreError> {
        let tx = self.frame_sender()?;
        tx.send(frame)
            .map_err(|_| CoreError::Asr("asr worker gone".into()))
    }

    fn finish(&mut self) -> Result<(), CoreError> {
        // Dropping the last sender disconnects the channel; the worker
        // flushes the open utterance and exits.
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Ok(())
    }
}

impl Drop for WhisperEngine {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn decode_loop(
    shared: Arc<SharedWhisper>,
    side: StreamSide,
    rx: mpsc::Receiver<AudioFrame>,
    mut sink: Box<dyn FnMut(TranscriptSegment) + Send>,
) {
    let mut state = match shared.context.create_state() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut segmenter = UtteranceSegmenter::new(low_latency_config());
    let mut events: Vec<SegmentEvent> = Vec::new();
    // Stream-position bookkeeping for start_ms/end_ms (16 kHz samples).
    let mut consumed_samples: u64 = 0;
    let mut seq: u64 = 0;
    let mut running = true;

    while running {
        events.clear();
        match rx.recv() {
            Ok(frame) => {
                consumed_samples += frame.samples.len() as u64;
                segmenter.feed(&frame.samples, &mut events);
                // Decoding can outrun capture; fold in anything queued so
                // partial windows always reflect the newest audio.
                while let Ok(more) = rx.try_recv() {
                    consumed_samples += more.samples.len() as u64;
                    segmenter.feed(&more.samples, &mut events);
                }
            }
            Err(_) => {
                // All senders dropped: session stopped — flush and exit.
                segmenter.finish(&mut events);
                running = false;
            }
        }

        // Collapse a burst to the newest partial window (decoding stale
        // windows wastes the latency budget), but keep every Final.
        let mut last_window: Option<Vec<f32>> = None;
        let mut work: Vec<(Vec<f32>, bool)> = Vec::new();
        for event in events.drain(..) {
            match event {
                SegmentEvent::Window(s) => last_window = Some(s),
                SegmentEvent::Final(s) => {
                    last_window = None;
                    work.push((s, true));
                }
            }
        }
        if let Some(s) = last_window {
            work.push((s, false));
        }

        for (samples, is_final) in work {
            // Partials decode only the most recent audio so their cost stays
            // bounded no matter how long the utterance runs; finals decode
            // the full window for accuracy.
            let input = decode_window(&samples, is_final);
            let decode_started = Instant::now();
            let Some(text) = decode(&mut state, input, is_final) else {
                continue;
            };
            if text.is_empty() {
                if is_final {
                    seq += 1;
                }
                continue;
            }
            let end_ms = consumed_samples * 1000 / TARGET_SAMPLE_RATE_HZ as u64;
            let dur_ms = input.len() as u64 * 1000 / TARGET_SAMPLE_RATE_HZ as u64;
            sink(TranscriptSegment {
                side,
                seq,
                text,
                is_final,
                start_ms: end_ms.saturating_sub(dur_ms),
                end_ms,
                confidence: None,
                latency_ms: decode_started.elapsed().as_millis() as u32,
            });
            if is_final {
                seq += 1;
            }
        }
    }
}

/// Cap a partial's decode window to the most recent audio (~6 s), so partial
/// decode time stays bounded however long the utterance runs. Finals decode
/// the full window.
const PARTIAL_MAX_SAMPLES: usize = 6 * TARGET_SAMPLE_RATE_HZ as usize;

fn decode_window(samples: &[f32], is_final: bool) -> &[f32] {
    if !is_final && samples.len() > PARTIAL_MAX_SAMPLES {
        &samples[samples.len() - PARTIAL_MAX_SAMPLES..]
    } else {
        samples
    }
}

/// Latency-tuned segmentation: partials ~2×/s, a line locks in ~400 ms after
/// you stop, and long run-ons finalize by 10 s (vs. the accuracy-first
/// defaults). Speeds up both conversation sides equally.
fn low_latency_config() -> SegmenterConfig {
    SegmenterConfig {
        partial_interval_ms: 500,
        silence_close_ms: 400,
        max_utterance_ms: 10_000,
        ..SegmenterConfig::default()
    }
}

/// Run one whisper decode over an utterance window. Partials use greedy
/// sampling; finals allow a slightly wider beam for quality (§2.5).
fn decode(state: &mut whisper_rs::WhisperState, samples: &[f32], is_final: bool) -> Option<String> {
    // whisper needs at least ~1000 ms of audio to behave; pad shorter
    // windows with trailing silence.
    const MIN_SAMPLES: usize = TARGET_SAMPLE_RATE_HZ as usize;
    let padded;
    let audio: &[f32] = if samples.len() < MIN_SAMPLES {
        padded = {
            let mut p = samples.to_vec();
            p.resize(MIN_SAMPLES, 0.0);
            p
        };
        &padded
    } else {
        samples
    };

    let strategy = if is_final {
        SamplingStrategy::BeamSearch {
            beam_size: 2,
            patience: -1.0,
        }
    } else {
        SamplingStrategy::Greedy { best_of: 1 }
    };
    let mut params = FullParams::new(strategy);
    params.set_language(Some("en"));
    params
        .set_n_threads((std::thread::available_parallelism().map_or(4, |n| n.get()) as i32).min(8));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_single_segment(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_no_timestamps(true);

    state.full(params, audio).ok()?;

    let n = state.full_n_segments().ok()?;
    let mut text = String::new();
    for i in 0..n {
        if let Ok(segment) = state.full_get_segment_text(i) {
            text.push_str(&segment);
        }
    }
    let text = text.trim().to_string();
    // Whisper hallucinates fillers on near-silence; drop the classics.
    const HALLUCINATIONS: &[&str] = &["[BLANK_AUDIO]", "(silence)", "[silence]", "."];
    if HALLUCINATIONS.contains(&text.as_str()) {
        return Some(String::new());
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_window_is_capped_but_finals_are_whole() {
        let long = vec![0.1f32; PARTIAL_MAX_SAMPLES + 5_000];
        // Partial: capped to the most recent PARTIAL_MAX_SAMPLES.
        assert_eq!(decode_window(&long, false).len(), PARTIAL_MAX_SAMPLES);
        // Final: the entire window is decoded.
        assert_eq!(decode_window(&long, true).len(), long.len());
        // Short partial: untouched.
        let short = vec![0.1f32; 1_000];
        assert_eq!(decode_window(&short, false).len(), 1_000);
    }

    #[test]
    fn low_latency_config_is_snappier_than_default() {
        let fast = low_latency_config();
        let base = SegmenterConfig::default();
        assert!(fast.partial_interval_ms < base.partial_interval_ms);
        assert!(fast.silence_close_ms < base.silence_close_ms);
        assert!(fast.max_utterance_ms <= base.max_utterance_ms);
    }
}
