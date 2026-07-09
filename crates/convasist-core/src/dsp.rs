//! Pure DSP helpers used by the audio pipeline (design §4.1).
//!
//! Everything here is allocation-conscious and stateful-streaming safe:
//! processing a signal in arbitrary chunk sizes yields the same output as
//! processing it in one call.

/// Downmix interleaved multi-channel f32 samples to mono by averaging.
/// `out` is cleared and refilled (reuse the buffer across calls).
pub fn downmix_interleaved_to_mono(input: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    if channels == 0 {
        return;
    }
    if channels == 1 {
        out.extend_from_slice(input);
        return;
    }
    let inv = 1.0 / channels as f32;
    out.extend(
        input
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * inv),
    );
}

/// Streaming linear-interpolation resampler (mono).
///
/// Chosen for M1 where the output only feeds VU meters; if ASR accuracy in
/// M2 shows artifacts, the internals swap to a windowed-sinc implementation
/// (rubato) behind this same interface.
pub struct LinearResampler {
    step: f64,
    /// Fractional read position into [prev, input...], where index 0.0 is
    /// `prev` (the last sample of the previous chunk) and 1.0 is input[0].
    pos: f64,
    prev: f32,
    primed: bool,
}

impl LinearResampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        assert!(src_rate > 0 && dst_rate > 0, "rates must be non-zero");
        Self {
            step: src_rate as f64 / dst_rate as f64,
            pos: 1.0, // start exactly on input[0] of the first chunk
            prev: 0.0,
            primed: false,
        }
    }

    /// Resample `input`, appending to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if !self.primed {
            self.prev = input[0];
            self.primed = true;
        }
        let n = input.len();
        // Sample index space: s(0) = prev, s(i) = input[i-1] for 1..=n.
        let sample = |idx: usize| -> f32 {
            if idx == 0 {
                self.prev
            } else {
                input[idx - 1]
            }
        };
        while self.pos <= n as f64 {
            let base = self.pos.floor();
            let frac = (self.pos - base) as f32;
            let i = base as usize;
            let a = sample(i);
            let b = if i < n { sample(i + 1) } else { a };
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        // Shift coordinates so next chunk's index 0.0 is input[n-1].
        self.pos -= n as f64;
        self.prev = input[n - 1];
    }
}

/// RMS level of a sample block in dBFS, clamped to [-90, 0].
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -90.0;
    }
    let mean_sq = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    let rms = mean_sq.sqrt().max(1e-9);
    (20.0 * rms.log10()).clamp(-90.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        let mut out = Vec::new();
        downmix_interleaved_to_mono(&[1.0, -1.0, 0.5, 0.5], 2, &mut out);
        assert_eq!(out, vec![0.0, 0.5]);
    }

    #[test]
    fn downmix_mono_is_passthrough() {
        let mut out = Vec::new();
        downmix_interleaved_to_mono(&[0.1, 0.2], 1, &mut out);
        assert_eq!(out, vec![0.1, 0.2]);
    }

    #[test]
    fn resampler_identity_rate_passes_through() {
        let mut rs = LinearResampler::new(16_000, 16_000);
        let mut out = Vec::new();
        rs.process(&[0.0, 1.0, 2.0, 3.0], &mut out);
        assert_eq!(out, vec![0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn resampler_3_to_1_ratio_yields_third_of_samples() {
        let mut rs = LinearResampler::new(48_000, 16_000);
        let input: Vec<f32> = (0..48_000).map(|i| i as f32).collect();
        let mut out = Vec::new();
        rs.process(&input, &mut out);
        let expected = 16_000;
        assert!(
            (out.len() as i64 - expected).abs() <= 1,
            "got {} samples",
            out.len()
        );
        // A linear ramp must resample to a linear ramp (lerp is exact).
        assert!((out[1] - out[0] - 3.0).abs() < 1e-3);
    }

    #[test]
    fn resampler_is_chunk_size_invariant() {
        let input: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.05).sin()).collect();

        let mut whole = Vec::new();
        LinearResampler::new(44_100, 16_000).process(&input, &mut whole);

        let mut chunked = Vec::new();
        let mut rs = LinearResampler::new(44_100, 16_000);
        for chunk in input.chunks(97) {
            rs.process(chunk, &mut chunked);
        }

        assert_eq!(whole.len(), chunked.len());
        for (a, b) in whole.iter().zip(&chunked) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn rms_of_silence_and_full_scale() {
        assert_eq!(rms_dbfs(&[]), -90.0);
        assert_eq!(rms_dbfs(&[0.0; 128]), -90.0);
        let full = rms_dbfs(&[1.0; 128]);
        assert!(full.abs() < 0.01, "full-scale ≈ 0 dBFS, got {full}");
        let half = rms_dbfs(&[0.5; 128]);
        assert!(
            (half + 6.02).abs() < 0.1,
            "half-scale ≈ -6 dBFS, got {half}"
        );
    }
}
