//! Text-to-speech — Deepgram Aura (`/v1/speak`) synthesized to PCM and played
//! on the default output device via cpal. Used by the live Sim Con rehearsal
//! so the AI counterparty speaks its turns. Reuses the saved Deepgram key
//! (same vault entry as cloud STT). Blocking — always driven from the rehearsal
//! worker thread, never the UI or audio-capture path.
//!
//! Aura bills per character; the caller records `text.chars().count()` in the
//! usage meter after a successful synth.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

use conva_core::CoreError;

/// A natural default Aura voice. (Voice selection can become a setting later.)
const AURA_MODEL: &str = "aura-asteria-en";
/// Rate we request from Aura (widely supported); resampled to the device rate.
const AURA_RATE: u32 = 24_000;

/// Synthesize `text` with Aura and play it on the default output device,
/// blocking until playback finishes. Returns `Ok(())` once spoken.
pub fn speak(api_key: &str, text: &str) -> Result<(), CoreError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }

    // 1. Fetch raw PCM (mono s16le @ AURA_RATE).
    let url = format!(
        "https://api.deepgram.com/v1/speak?model={AURA_MODEL}&encoding=linear16&sample_rate={AURA_RATE}&container=none"
    );
    let resp = ureq::post(&url)
        .timeout(Duration::from_secs(30))
        .set("Authorization", &format!("Token {api_key}"))
        .set("Content-Type", "application/json")
        .send_json(serde_json::json!({ "text": text }))
        .map_err(|e| CoreError::Audio(format!("aura request: {e}")))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| CoreError::Audio(format!("aura read: {e}")))?;
    if bytes.len() < 2 {
        return Err(CoreError::Audio("aura returned no audio".into()));
    }
    let mono: Vec<f32> = bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect();

    // 2. Resolve the output device + its native format.
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| CoreError::Audio("no audio output device".into()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| CoreError::Audio(e.to_string()))?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();
    let out_rate = config.sample_rate.0;
    let channels = config.channels as usize;

    // 3. Match the device sample rate.
    let samples = if out_rate == AURA_RATE {
        mono
    } else {
        resample_linear(&mono, AURA_RATE, out_rate)
    };
    let total_frames = samples.len();
    let samples = Arc::new(samples);
    let pos = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));

    // 4. Stream it to the device.
    let err_fn = |e| eprintln!("[tts] output stream error: {e}");
    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config,
            samples,
            pos,
            done.clone(),
            channels,
            err_fn,
        )?,
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config,
            samples,
            pos,
            done.clone(),
            channels,
            err_fn,
        )?,
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config,
            samples,
            pos,
            done.clone(),
            channels,
            err_fn,
        )?,
        other => {
            return Err(CoreError::Audio(format!(
                "unsupported output format: {other:?}"
            )))
        }
    };
    stream.play().map_err(|e| CoreError::Audio(e.to_string()))?;

    // 5. Block until the buffer drains (with a safety cap), then let the last
    //    device buffer flush before dropping the stream.
    let max = Duration::from_secs_f32(total_frames as f32 / out_rate.max(1) as f32 + 2.0);
    let started = Instant::now();
    while !done.load(Ordering::Relaxed) && started.elapsed() < max {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(80));
    drop(stream);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Vec<f32>>,
    pos: Arc<AtomicUsize>,
    done: Arc<AtomicBool>,
    channels: usize,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, CoreError>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let i = pos.fetch_add(1, Ordering::Relaxed);
                    let sample = if i < samples.len() {
                        samples[i]
                    } else {
                        done.store(true, Ordering::Relaxed);
                        0.0
                    };
                    let value = T::from_sample(sample);
                    for slot in frame.iter_mut() {
                        *slot = value;
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| CoreError::Audio(e.to_string()))
}

/// Minimal linear resampler (mono). Aura's fixed output rate → the device rate.
fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 || to == 0 {
        return Vec::new();
    }
    if from == to {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = (input.len() as f64 * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}
