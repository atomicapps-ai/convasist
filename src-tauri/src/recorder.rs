//! Call recording (owner request): capture the conversation as stereo audio
//! without slowing the app down.
//!
//! The app already produces 16 kHz mono frames per side for ASR. Recording
//! *tees* those frames (a cheap `Vec` copy + channel send on the audio worker
//! thread) to a dedicated writer thread — no extra capture, and all WAV
//! encoding + disk I/O happen off the audio and UI paths. The output is a
//! 16-bit stereo WAV: **left = you (microphone), right = them (system audio)**.
//!
//! The two sides arrive as independent frame streams, so the writer is driven
//! by a wall clock: every ~100 ms it advances both channels to where real
//! time says they should be, filling any side that hasn't delivered with
//! silence. That keeps the channels time-aligned and tolerates loopback going
//! quiet during silence, at the cost of sub-100 ms channel skew (inaudible
//! for a call). Sample-rate vs. wall-clock drift is a few samples per minute.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use convasist_core::audio::{StreamSide, TARGET_SAMPLE_RATE_HZ};
use convasist_core::CoreError;

const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

enum RecMsg {
    Frame(StreamSide, Vec<i16>),
    Stop,
}

/// Handle to a running recording. Dropping it (or calling `stop`) finalizes
/// the WAV file.
pub struct Recorder {
    tx: Sender<RecMsg>,
    join: Option<JoinHandle<()>>,
    path: PathBuf,
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

impl Recorder {
    /// Create the WAV file and start the writer thread. Surfaces file-create
    /// errors synchronously.
    pub fn start(path: PathBuf) -> Result<Self, CoreError> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: TARGET_SAMPLE_RATE_HZ,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(&path, spec)
            .map_err(|e| CoreError::Audio(format!("create recording file: {e}")))?;

        let (tx, rx) = mpsc::channel::<RecMsg>();
        let join = std::thread::Builder::new()
            .name("recorder".into())
            .spawn(move || writer_loop(rx, writer))
            .map_err(|e| CoreError::Audio(format!("spawn recorder: {e}")))?;

        Ok(Self {
            tx,
            join: Some(join),
            path,
        })
    }

    /// Tee one side's frame into the recording. Called on the audio worker
    /// thread — converts to i16 and hands off; the writer thread does the
    /// rest. A closed channel (writer gone) is ignored.
    pub fn push(&self, side: StreamSide, samples: &[f32]) {
        let pcm: Vec<i16> = samples.iter().map(|s| to_i16(*s)).collect();
        let _ = self.tx.send(RecMsg::Frame(side, pcm));
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Finalize the WAV (writes the correct RIFF length) and return its path.
    pub fn stop(mut self) -> PathBuf {
        let _ = self.tx.send(RecMsg::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        self.path.clone()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // If dropped without an explicit stop, still finalize cleanly.
        let _ = self.tx.send(RecMsg::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn writer_loop(
    rx: Receiver<RecMsg>,
    mut writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
) {
    let start = Instant::now();
    let mut left: VecDeque<i16> = VecDeque::new();
    let mut right: VecDeque<i16> = VecDeque::new();
    let mut written: u64 = 0;
    let mut last_flush = Instant::now();

    loop {
        match rx.recv_timeout(FLUSH_INTERVAL) {
            Ok(RecMsg::Frame(StreamSide::Outbound, pcm)) => left.extend(pcm),
            Ok(RecMsg::Frame(StreamSide::Inbound, pcm)) => right.extend(pcm),
            Ok(RecMsg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if last_flush.elapsed() >= FLUSH_INTERVAL {
            let target = wall_target(start);
            flush_to(&mut writer, &mut left, &mut right, &mut written, target);
            last_flush = Instant::now();
        }
    }

    // Final flush to the wall clock, then drain whatever is still buffered so
    // the tail isn't lost.
    let target = wall_target(start);
    flush_to(&mut writer, &mut left, &mut right, &mut written, target);
    while !left.is_empty() || !right.is_empty() {
        let l = left.pop_front().unwrap_or(0);
        let r = right.pop_front().unwrap_or(0);
        let _ = writer.write_sample(l);
        let _ = writer.write_sample(r);
    }
    let _ = writer.finalize();
}

/// Number of stereo frames that should exist by now (real time × sample rate).
fn wall_target(start: Instant) -> u64 {
    (start.elapsed().as_secs_f64() * TARGET_SAMPLE_RATE_HZ as f64) as u64
}

/// Advance both channels up to `target` frames, popping buffered samples and
/// writing silence for any side that has underrun.
fn flush_to(
    writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    left: &mut VecDeque<i16>,
    right: &mut VecDeque<i16>,
    written: &mut u64,
    target: u64,
) {
    while *written < target {
        let l = left.pop_front().unwrap_or(0);
        let r = right.pop_front().unwrap_or(0);
        let _ = writer.write_sample(l);
        let _ = writer.write_sample(r);
        *written += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_stereo_wav_with_both_channels() {
        let dir = std::env::temp_dir().join(format!("convasist-rec-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("call.wav");

        let rec = Recorder::start(path.clone()).unwrap();
        // Feed distinct non-zero content to each side over a short window.
        for _ in 0..8 {
            rec.push(StreamSide::Outbound, &[0.5f32; 320]); // you → left
            rec.push(StreamSide::Inbound, &[-0.4f32; 320]); // them → right
            std::thread::sleep(Duration::from_millis(20));
        }
        let out = rec.stop();
        assert_eq!(out, path);

        // Read it back: stereo, 16 kHz, with real content on both channels.
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, TARGET_SAMPLE_RATE_HZ);
        let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
        assert!(!samples.is_empty(), "no audio written");
        assert_eq!(samples.len() % 2, 0, "stereo frames must be paired");

        let left_has_signal = samples.iter().step_by(2).any(|&s| s > 1000);
        let right_has_signal = samples.iter().skip(1).step_by(2).any(|&s| s < -1000);
        assert!(left_has_signal, "left (you) channel is silent");
        assert!(right_has_signal, "right (them) channel is silent");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
