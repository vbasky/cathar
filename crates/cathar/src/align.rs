//! Time-alignment by cross-correlation.
//!
//! - [`azimuth_correct`] fixes L/R skew (misaligned tape heads, off-centre
//!   grooves) by shifting the right channel to best match the left.
//! - [`align`] time-aligns a separate recording to a reference (multi-mic /
//!   reference-track workflows).
//!
//! Both estimate a sub-sample lag from either plain normalised cross-correlation
//! or **GCC-PHAT** (generalised cross-correlation with phase transform) and
//! apply a fractional shift. Deterministic.

use realfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Analysis window cap — bounds correlation cost.
const WINDOW_CAP: usize = 1 << 17;

/// How lag is estimated between two signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LagMethod {
    /// Time-domain normalised cross-correlation (default; cheap, robust on
    /// similar signals).
    #[default]
    Correlation,
    /// Generalised cross-correlation with phase transform — better on
    /// dissimilar levels or mild reverb (multi-mic, reference tracks).
    GccPhat,
}

/// Estimate the lag (in samples, sub-sample precision) by which `signal` must be
/// advanced to best align with `reference`, searching within `±max_ms`.
///
/// Uses plain cross-correlation ([`LagMethod::Correlation`]).
pub fn estimate_lag(reference: &[f32], signal: &[f32], sample_rate: u32, max_ms: f32) -> f32 {
    estimate_lag_with_method(reference, signal, sample_rate, max_ms, LagMethod::Correlation)
}

/// Like [`estimate_lag`] but with an explicit [`LagMethod`].
pub fn estimate_lag_with_method(
    reference: &[f32],
    signal: &[f32],
    sample_rate: u32,
    max_ms: f32,
    method: LagMethod,
) -> f32 {
    match method {
        LagMethod::Correlation => estimate_lag_correlation(reference, signal, sample_rate, max_ms),
        LagMethod::GccPhat => estimate_lag_gcc_phat(reference, signal, sample_rate, max_ms),
    }
}

fn estimate_lag_correlation(
    reference: &[f32],
    signal: &[f32],
    sample_rate: u32,
    max_ms: f32,
) -> f32 {
    let n = reference.len().min(signal.len());
    if n < 16 || sample_rate == 0 {
        return 0.0;
    }
    let max_lag = (((max_ms / 1000.0) * sample_rate as f32) as isize).max(1);
    let win = n.min(WINDOW_CAP);

    let corr = |lag: isize| -> f32 {
        let mut s = 0.0f32;
        let mut cnt = 0usize;
        for (i, &r) in reference[..win].iter().enumerate() {
            let j = i as isize + lag;
            if j >= 0 && (j as usize) < win {
                s += r * signal[j as usize];
                cnt += 1;
            }
        }
        if cnt > 0 { s / cnt as f32 } else { f32::MIN }
    };

    peak_lag(max_lag, corr)
}

/// GCC-PHAT: `IFFT( X·conj(Y) / |X·conj(Y)| )` peak within `±max_ms`.
fn estimate_lag_gcc_phat(reference: &[f32], signal: &[f32], sample_rate: u32, max_ms: f32) -> f32 {
    let n = reference.len().min(signal.len());
    if n < 16 || sample_rate == 0 {
        return 0.0;
    }
    let max_lag = (((max_ms / 1000.0) * sample_rate as f32) as isize).max(1);
    let win = n.min(WINDOW_CAP);

    // Zero-padded FFT long enough for linear (non-circular) correlation.
    let mut n_fft = 1usize;
    while n_fft < 2 * win {
        n_fft <<= 1;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let ifft = planner.plan_fft_inverse(n_fft);

    let mut x = vec![Complex::new(0.0, 0.0); n_fft];
    let mut y = vec![Complex::new(0.0, 0.0); n_fft];
    // Hann window reduces spectral leakage so PHAT peaks stay sharp.
    for i in 0..win {
        let w = 0.5
            - 0.5
                * (2.0 * std::f32::consts::PI * i as f32 / (win.saturating_sub(1).max(1) as f32))
                    .cos();
        x[i] = Complex::new(reference[i] * w, 0.0);
        y[i] = Complex::new(signal[i] * w, 0.0);
    }
    fft.process(&mut x);
    fft.process(&mut y);

    // PHAT weighting: unit-magnitude cross-spectrum.
    for i in 0..n_fft {
        let mut r = y[i] * x[i].conj(); // peak lag matches time-domain sum r[i]·s[i+lag]
        let mag = r.norm();
        r = if mag > 1e-12 { r / mag } else { Complex::new(0.0, 0.0) };
        x[i] = r;
    }
    ifft.process(&mut x);

    // rustfft is unnormalised; relative peak location is all we need.
    let corr = |lag: isize| -> f32 {
        let idx = if lag >= 0 { lag as usize } else { (n_fft as isize + lag) as usize };
        x[idx % n_fft].re
    };

    peak_lag(max_lag, corr)
}

fn peak_lag(max_lag: isize, corr: impl Fn(isize) -> f32) -> f32 {
    let mut best = 0isize;
    let mut best_v = f32::MIN;
    for lag in -max_lag..=max_lag {
        let v = corr(lag);
        if v > best_v {
            best_v = v;
            best = lag;
        }
    }
    // Parabolic interpolation of the correlation peak.
    let (a, b, c) = (corr(best - 1), corr(best), corr(best + 1));
    let denom = a - 2.0 * b + c;
    let delta = if denom.abs() > 1e-12 { 0.5 * (a - c) / denom } else { 0.0 };
    best as f32 + delta.clamp(-1.0, 1.0)
}

/// Resample `signal` at positions `i + lag` (linear interpolation) — a
/// fractional time shift. Out-of-range positions read as silence.
fn shift_fractional(signal: &[f32], lag: f32) -> Vec<f32> {
    let n = signal.len();
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let pos = i as f32 + lag;
        let j = pos.floor() as isize;
        let frac = pos - j as f32;
        let a = if j >= 0 && (j as usize) < n { signal[j as usize] } else { 0.0 };
        let b = if j + 1 >= 0 && ((j + 1) as usize) < n { signal[(j + 1) as usize] } else { 0.0 };
        *o = a * (1.0 - frac) + b * frac;
    }
    out
}

/// Align `signal` to `reference`, returning the shifted `signal` (same length).
pub fn align(reference: &[f32], signal: &[f32], sample_rate: u32, max_ms: f32) -> Vec<f32> {
    align_with_method(reference, signal, sample_rate, max_ms, LagMethod::Correlation)
}

/// Like [`align`] with an explicit [`LagMethod`].
pub fn align_with_method(
    reference: &[f32],
    signal: &[f32],
    sample_rate: u32,
    max_ms: f32,
    method: LagMethod,
) -> Vec<f32> {
    let lag = estimate_lag_with_method(reference, signal, sample_rate, max_ms, method);
    shift_fractional(signal, lag)
}

/// Correct stereo azimuth skew: keep the left channel and shift the right to
/// best align with it. Returns `(left, corrected_right)`.
pub fn azimuth_correct(
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    max_ms: f32,
) -> (Vec<f32>, Vec<f32>) {
    azimuth_correct_with_method(left, right, sample_rate, max_ms, LagMethod::Correlation)
}

/// Like [`azimuth_correct`] with an explicit [`LagMethod`].
pub fn azimuth_correct_with_method(
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    max_ms: f32,
    method: LagMethod,
) -> (Vec<f32>, Vec<f32>) {
    let corrected = align_with_method(left, right, sample_rate, max_ms, method);
    (left.to_vec(), corrected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(sr: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                (2.0 * std::f32::consts::PI * 137.0 * t).sin()
                    + 0.6 * (2.0 * std::f32::consts::PI * 523.0 * t).sin()
            })
            .collect()
    }

    #[test]
    fn recovers_known_delay() {
        let sr = 48_000u32;
        let n = 40_000usize;
        let reference = signal(sr, n);
        let delay = 17usize;
        let mut delayed = vec![0.0f32; n];
        delayed[delay..].copy_from_slice(&reference[..n - delay]);
        let lag = estimate_lag(&reference, &delayed, sr, 5.0);
        assert!((lag - delay as f32).abs() < 0.5, "estimated lag {lag}, want {delay}");

        let aligned = align(&reference, &delayed, sr, 5.0);
        let err: f32 =
            (5_000..35_000).map(|i| (aligned[i] - reference[i]).abs()).sum::<f32>() / 30_000.0;
        assert!(err < 0.05, "alignment residual {err}");
    }

    #[test]
    fn gcc_phat_recovers_known_delay() {
        let sr = 48_000u32;
        let n = 40_000usize;
        // Broadband-ish content (PHAT is for impulsive / multi-mic pairs, not pure tones).
        let reference: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                let mut s = 0.0f32;
                for k in 1..40 {
                    let f = 80.0 * k as f32;
                    s += (2.0 * std::f32::consts::PI * f * t).sin() / k as f32;
                }
                // Mild deterministic "noise" for spectral fill.
                s + 0.05
                    * ((i as u32).wrapping_mul(1103515245).wrapping_add(12345) as f32
                        / u32::MAX as f32
                        * 2.0
                        - 1.0)
            })
            .collect();
        let delay = 23usize;
        let mut delayed = vec![0.0f32; n];
        delayed[delay..].copy_from_slice(&reference[..n - delay]);
        // Level mismatch — PHAT should still find the delay.
        let delayed: Vec<f32> = delayed.iter().map(|s| s * 0.15).collect();
        let lag = estimate_lag_with_method(&reference, &delayed, sr, 5.0, LagMethod::GccPhat);
        assert!((lag - delay as f32).abs() < 0.75, "GCC-PHAT lag {lag}, want {delay}");
    }

    #[test]
    fn azimuth_reduces_interchannel_lag() {
        let sr = 48_000u32;
        let n = 40_000usize;
        let left = signal(sr, n);
        let mut right = vec![0.0f32; n];
        right[9..].copy_from_slice(&left[..n - 9]); // 9-sample skew
        let (l, r) = azimuth_correct(&left, &right, sr, 5.0);
        let residual = estimate_lag(&l, &r, sr, 5.0);
        assert!(residual.abs() < 1.0, "azimuth not corrected, residual {residual}");
    }
}
