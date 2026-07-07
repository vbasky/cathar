//! Time-alignment by cross-correlation.
//!
//! - [`azimuth_correct`] fixes L/R skew (misaligned tape heads, off-centre
//!   grooves) by shifting the right channel to best match the left.
//! - [`align`] time-aligns a separate recording to a reference (multi-mic /
//!   reference-track workflows).
//!
//! Both estimate a sub-sample lag from the normalised cross-correlation (peak
//! with parabolic interpolation) and apply a fractional shift. Deterministic.

/// Analysis window cap — bounds the O(window × max_lag) correlation cost.
const WINDOW_CAP: usize = 1 << 17;

/// Estimate the lag (in samples, sub-sample precision) by which `signal` must be
/// advanced to best align with `reference`, searching within `±max_ms`.
pub fn estimate_lag(reference: &[f32], signal: &[f32], sample_rate: u32, max_ms: f32) -> f32 {
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
    let lag = estimate_lag(reference, signal, sample_rate, max_ms);
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
    let corrected = align(left, right, sample_rate, max_ms);
    (left.to_vec(), corrected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(sr: u32, n: usize) -> Vec<f32> {
        // A non-periodic-ish mix so the correlation peak is unambiguous.
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
        // `delayed[i] = reference[i - delay]` → signal lags reference by `delay`.
        let mut delayed = vec![0.0f32; n];
        delayed[delay..].copy_from_slice(&reference[..n - delay]);
        let lag = estimate_lag(&reference, &delayed, sr, 5.0);
        assert!((lag - delay as f32).abs() < 0.5, "estimated lag {lag}, want {delay}");

        // Aligning should bring it back onto the reference.
        let aligned = align(&reference, &delayed, sr, 5.0);
        let err: f32 =
            (5_000..35_000).map(|i| (aligned[i] - reference[i]).abs()).sum::<f32>() / 30_000.0;
        assert!(err < 0.05, "alignment residual {err}");
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
