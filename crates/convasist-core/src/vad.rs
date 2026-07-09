//! Voice-activity gating + utterance segmentation (design §4.1 A5).
//!
//! M2 ships an energy-based VAD: simple, deterministic, unit-testable, and
//! zero-dependency. The Silero ONNX upgrade slots in behind the same
//! `UtteranceSegmenter` interface if energy gating proves too coarse (its
//! job is only to bound ASR windows — whisper itself is robust to a little
//! leading/trailing silence).
//!
//! Contract: feed 16 kHz mono samples in arbitrary chunk sizes; receive
//! `SegmentEvent`s. `Window` events carry the utterance-so-far for partial
//! decodes; `Final` carries the complete utterance when it closes (silence
//! or max length).

use crate::dsp::rms_dbfs;

#[derive(Debug, Clone)]
pub struct SegmenterConfig {
    pub sample_rate: u32,
    /// Analysis frame length.
    pub frame_ms: u32,
    /// Frames at or above this RMS level count as speech.
    pub speech_threshold_dbfs: f32,
    /// Silence run that closes an utterance.
    pub silence_close_ms: u32,
    /// Hard cap: an utterance longer than this is finalized mid-speech so
    /// partial latency and decode cost stay bounded (§2.5).
    pub max_utterance_ms: u32,
    /// Speech shorter than this is discarded as noise.
    pub min_speech_ms: u32,
    /// Audio kept from before the first speech frame (soft onsets).
    pub pre_roll_ms: u32,
    /// Emit a `Window` partial no more often than this while speech runs.
    pub partial_interval_ms: u32,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            frame_ms: 30,
            speech_threshold_dbfs: -45.0,
            silence_close_ms: 600,
            max_utterance_ms: 12_000,
            min_speech_ms: 200,
            pre_roll_ms: 300,
            partial_interval_ms: 1_200,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SegmentEvent {
    /// Utterance-in-progress snapshot (for a partial decode). Carries all
    /// samples of the open utterance including pre-roll.
    Window(Vec<f32>),
    /// The utterance closed; decode + emit as final.
    Final(Vec<f32>),
}

#[derive(Clone, Copy)]
enum Phase {
    Silence,
    Speech {
        silence_run_frames: u32,
        frames_since_partial: u32,
    },
}

pub struct UtteranceSegmenter {
    config: SegmenterConfig,
    frame_len: usize,
    /// Buffered input not yet forming a whole analysis frame.
    pending: Vec<f32>,
    /// Rolling pre-roll kept during silence.
    pre_roll: Vec<f32>,
    /// Samples of the open utterance.
    utterance: Vec<f32>,
    /// Speech frames observed in the open utterance (min-speech gate).
    speech_frames: u32,
    phase: Phase,
}

impl UtteranceSegmenter {
    pub fn new(config: SegmenterConfig) -> Self {
        let frame_len = (config.sample_rate as usize * config.frame_ms as usize) / 1000;
        Self {
            config,
            frame_len: frame_len.max(1),
            pending: Vec::new(),
            pre_roll: Vec::new(),
            utterance: Vec::new(),
            speech_frames: 0,
            phase: Phase::Silence,
        }
    }

    fn ms_to_frames(&self, ms: u32) -> u32 {
        (ms / self.config.frame_ms).max(1)
    }

    fn pre_roll_len(&self) -> usize {
        (self.config.sample_rate as usize * self.config.pre_roll_ms as usize) / 1000
    }

    fn max_utterance_len(&self) -> usize {
        (self.config.sample_rate as usize * self.config.max_utterance_ms as usize) / 1000
    }

    fn min_speech_frames(&self) -> u32 {
        self.ms_to_frames(self.config.min_speech_ms)
    }

    /// Feed samples; append resulting events to `events`.
    pub fn feed(&mut self, samples: &[f32], events: &mut Vec<SegmentEvent>) {
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= self.frame_len {
            let frame: Vec<f32> = self.pending.drain(..self.frame_len).collect();
            self.process_frame(&frame, events);
        }
    }

    /// Session stop: close any open utterance.
    pub fn finish(&mut self, events: &mut Vec<SegmentEvent>) {
        if matches!(self.phase, Phase::Speech { .. })
            && self.speech_frames >= self.min_speech_frames()
        {
            events.push(SegmentEvent::Final(std::mem::take(&mut self.utterance)));
        }
        self.utterance.clear();
        self.pre_roll.clear();
        self.pending.clear();
        self.speech_frames = 0;
        self.phase = Phase::Silence;
    }

    fn process_frame(&mut self, frame: &[f32], events: &mut Vec<SegmentEvent>) {
        let is_speech = rms_dbfs(frame) >= self.config.speech_threshold_dbfs;
        let silence_close = self.ms_to_frames(self.config.silence_close_ms);
        let partial_every = self.ms_to_frames(self.config.partial_interval_ms);
        let min_speech = self.min_speech_frames();
        let max_len = self.max_utterance_len();
        let pre_roll_keep = self.pre_roll_len();

        match self.phase {
            Phase::Silence => {
                if is_speech {
                    // Open an utterance seeded with the pre-roll.
                    self.utterance.clear();
                    self.utterance.extend_from_slice(&self.pre_roll);
                    self.utterance.extend_from_slice(frame);
                    self.pre_roll.clear();
                    self.speech_frames = 1;
                    self.phase = Phase::Speech {
                        silence_run_frames: 0,
                        frames_since_partial: 0,
                    };
                } else {
                    self.pre_roll.extend_from_slice(frame);
                    if self.pre_roll.len() > pre_roll_keep {
                        let cut = self.pre_roll.len() - pre_roll_keep;
                        self.pre_roll.drain(..cut);
                    }
                }
            }
            Phase::Speech {
                mut silence_run_frames,
                mut frames_since_partial,
            } => {
                self.utterance.extend_from_slice(frame);
                frames_since_partial += 1;
                if is_speech {
                    self.speech_frames += 1;
                    silence_run_frames = 0;
                } else {
                    silence_run_frames += 1;
                }

                let closed_by_silence = silence_run_frames >= silence_close;
                let closed_by_length = self.utterance.len() >= max_len;

                if closed_by_silence || closed_by_length {
                    let enough_speech = self.speech_frames >= min_speech;
                    let samples = std::mem::take(&mut self.utterance);
                    if enough_speech {
                        events.push(SegmentEvent::Final(samples));
                    }
                    self.speech_frames = 0;
                    self.phase = Phase::Silence;
                } else {
                    if frames_since_partial >= partial_every && self.speech_frames >= min_speech {
                        frames_since_partial = 0;
                        events.push(SegmentEvent::Window(self.utterance.clone()));
                    }
                    self.phase = Phase::Speech {
                        silence_run_frames,
                        frames_since_partial,
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SegmenterConfig {
        SegmenterConfig::default()
    }

    fn silence(ms: u32) -> Vec<f32> {
        vec![0.0; (16_000 * ms as usize) / 1000]
    }

    fn tone(ms: u32) -> Vec<f32> {
        (0..(16_000 * ms as usize) / 1000)
            .map(|i| (i as f32 * 0.2).sin() * 0.5)
            .collect()
    }

    fn run(chunks: &[Vec<f32>]) -> Vec<SegmentEvent> {
        let mut seg = UtteranceSegmenter::new(config());
        let mut events = Vec::new();
        for chunk in chunks {
            seg.feed(chunk, &mut events);
        }
        seg.finish(&mut events);
        events
    }

    #[test]
    fn pure_silence_emits_nothing() {
        assert!(run(&[silence(3_000)]).is_empty());
    }

    #[test]
    fn short_blip_is_discarded_as_noise() {
        // 60 ms of tone < min_speech_ms (200) → nothing.
        assert!(run(&[silence(500), tone(60), silence(1_000)]).is_empty());
    }

    #[test]
    fn utterance_closes_on_silence_and_is_final() {
        let events = run(&[silence(500), tone(1_000), silence(1_000)]);
        let finals: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SegmentEvent::Final(_)))
            .collect();
        assert_eq!(finals.len(), 1, "events: {}", events.len());
        if let SegmentEvent::Final(samples) = finals[0] {
            // ~1 s of speech + pre-roll + closing silence ≤ total fed.
            assert!(samples.len() >= 16_000, "got {}", samples.len());
        }
    }

    #[test]
    fn long_speech_emits_partial_windows_before_final() {
        let events = run(&[silence(300), tone(3_000), silence(1_000)]);
        let windows = events
            .iter()
            .filter(|e| matches!(e, SegmentEvent::Window(_)))
            .count();
        let finals = events
            .iter()
            .filter(|e| matches!(e, SegmentEvent::Final(_)))
            .count();
        assert!(windows >= 1, "expected partial windows, got {windows}");
        assert_eq!(finals, 1);
    }

    #[test]
    fn very_long_speech_is_split_by_max_length() {
        let events = run(&[tone(30_000), silence(1_000)]);
        let finals = events
            .iter()
            .filter(|e| matches!(e, SegmentEvent::Final(_)))
            .count();
        assert!(
            finals >= 2,
            "30 s of speech must split, got {finals} finals"
        );
    }

    #[test]
    fn chunk_size_does_not_change_results() {
        let signal: Vec<f32> = [silence(400), tone(1_500), silence(900)].concat();

        let whole = run(std::slice::from_ref(&signal));
        let tiny: Vec<Vec<f32>> = signal.chunks(160).map(|c| c.to_vec()).collect();
        let chunked = run(&tiny);

        assert_eq!(whole, chunked);
    }

    #[test]
    fn finish_flushes_open_utterance() {
        let mut seg = UtteranceSegmenter::new(config());
        let mut events = Vec::new();
        seg.feed(&tone(1_000), &mut events);
        seg.finish(&mut events);
        assert!(events.iter().any(|e| matches!(e, SegmentEvent::Final(_))));
    }
}
