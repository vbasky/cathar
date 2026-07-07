//! Constant-Q transform — a log-frequency spectral analysis where every bin
//! spans the same number of octaves (constant Q = f / Δf), so musical intervals
//! are evenly spaced. A companion to the linear-frequency [`crate::spectrogram`]
//! for pitch- and harmony-aligned work and the terminal viewer.
//!
//! Direct time-domain evaluation: each bin `k` correlates the signal against a
//! Hann-windowed complex exponential at `f_k`, whose length `N_k = Q·sr/f_k`
//! grows toward low frequencies. Deterministic, pure Rust.

use crate::util::hann_window;

/// A constant-Q magnitude spectrogram: a column per time frame, each holding
/// `bins` log-spaced frequency rows in decibels.
#[derive(Debug, Clone)]
pub struct CqtSpec {
    /// Source sample rate, Hz.
    pub sample_rate: u32,
    /// Lowest analysed frequency (row 0), Hz.
    pub f_min: f32,
    /// Bins per octave.
    pub bins_per_octave: usize,
    /// Number of frequency bins per frame.
    pub bins: usize,
    /// Hop between frames, samples.
    pub hop: usize,
    /// Row-major dB magnitudes: `data[frame * bins + bin]`.
    pub data: Vec<f32>,
}

impl CqtSpec {
    /// Number of time frames.
    pub fn frames(&self) -> usize {
        self.data.len().checked_div(self.bins).unwrap_or(0)
    }

    /// dB magnitude at `(frame, bin)`.
    pub fn get(&self, frame: usize, bin: usize) -> f32 {
        self.data[frame * self.bins + bin]
    }

    /// Centre frequency (Hz) of a bin: `f_min · 2^(bin / bins_per_octave)`.
    pub fn bin_hz(&self, bin: usize) -> f32 {
        self.f_min * 2f32.powf(bin as f32 / self.bins_per_octave as f32)
    }
}

/// Compute the constant-Q transform of `signal`. `bins_per_octave` sets the
/// log-frequency resolution (12 = semitones); `f_min` is the lowest frequency.
pub fn cqt(signal: &[f32], sample_rate: u32, bins_per_octave: usize, f_min: f32) -> CqtSpec {
    let bpo = bins_per_octave.max(1);
    let sr = sample_rate as f32;
    let f_min = f_min.max(1.0);
    let f_max = sr * 0.5 * 0.98;
    let bins = if f_max > f_min {
        (bpo as f32 * (f_max / f_min).log2()).floor().max(1.0) as usize
    } else {
        1
    };
    // Constant Q for these geometric bins.
    let q = 1.0 / (2f32.powf(1.0 / bpo as f32) - 1.0);

    // Precompute a windowed complex kernel per bin (amplitude-normalised).
    struct Kernel {
        len: usize,
        cos: Vec<f32>,
        sin: Vec<f32>,
    }
    let mut kernels = Vec::with_capacity(bins);
    for k in 0..bins {
        let f_k = f_min * 2f32.powf(k as f32 / bpo as f32);
        let n_k = ((q * sr / f_k).round() as usize).max(4) & !1;
        let win = hann_window(n_k);
        let win_sum: f32 = win.iter().sum::<f32>().max(1e-9);
        let mut cos = vec![0.0f32; n_k];
        let mut sin = vec![0.0f32; n_k];
        for i in 0..n_k {
            let phase = 2.0 * std::f32::consts::PI * f_k * i as f32 / sr;
            let scale = 2.0 * win[i] / win_sum;
            cos[i] = scale * phase.cos();
            sin[i] = scale * phase.sin();
        }
        kernels.push(Kernel { len: n_k, cos, sin });
    }

    let n_max = kernels.first().map_or(0, |k| k.len);
    let hop = (n_max / 16).max(256);
    let half = n_max / 2;
    let mut data = Vec::new();
    if signal.len() < n_max {
        return CqtSpec { sample_rate, f_min, bins_per_octave: bpo, bins, hop, data };
    }

    let mut center = half;
    while center + half <= signal.len() {
        for kern in &kernels {
            let kh = kern.len / 2;
            let start = center - kh;
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            for i in 0..kern.len {
                let x = signal[start + i];
                re += x * kern.cos[i];
                im -= x * kern.sin[i];
            }
            let mag = (re * re + im * im).sqrt();
            data.push((20.0 * mag.max(1e-6).log10()).max(-120.0));
        }
        center += hop;
    }

    CqtSpec { sample_rate, f_min, bins_per_octave: bpo, bins, hop, data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_peaks_at_its_bin() {
        let sr = 48_000u32;
        let f_min = 55.0f32; // A1
        let bpo = 12;
        // 440 Hz = 55 · 2^3 → exactly bin 36.
        let sig: Vec<f32> = (0..sr)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let spec = cqt(&sig, sr, bpo, f_min);
        assert!(spec.frames() > 0);
        let f = spec.frames() / 2;
        let mut peak = 0;
        for b in 0..spec.bins {
            if spec.get(f, b) > spec.get(f, peak) {
                peak = b;
            }
        }
        assert!(
            (spec.bin_hz(peak) - 440.0).abs() < 15.0,
            "peak at {} Hz (bin {peak}), want ~440",
            spec.bin_hz(peak)
        );
    }

    #[test]
    fn bins_are_log_spaced() {
        let spec = CqtSpec {
            sample_rate: 48_000,
            f_min: 55.0,
            bins_per_octave: 12,
            bins: 24,
            hop: 512,
            data: Vec::new(),
        };
        // One octave up doubles the frequency.
        assert!((spec.bin_hz(12) - 110.0).abs() < 0.01);
        assert!((spec.bin_hz(0) - 55.0).abs() < 0.01);
    }
}
