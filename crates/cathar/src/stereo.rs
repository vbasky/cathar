//! Mid-side / stereo toolkit.
//!
//! Exact M/S encode–decode, width scaling, mono-maker below a cutoff (same
//! crossover idea as [`crate::elliptical_mono`]), Haas-style mono→stereo
//! upmix, and a zero-lag phase-correlation meter for L/R health checks.

use crate::digitize::elliptical_mono;
use crate::filter::lowpass;

/// Encode left/right into mid/side: `M = (L+R)/2`, `S = (L−R)/2`.
///
/// Exact inverse of [`ms_decode`]. Length is `min(L, R)`.
pub fn ms_encode(left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let n = left.len().min(right.len());
    let mut mid = Vec::with_capacity(n);
    let mut side = Vec::with_capacity(n);
    for i in 0..n {
        mid.push((left[i] + right[i]) * 0.5);
        side.push((left[i] - right[i]) * 0.5);
    }
    (mid, side)
}

/// Decode mid/side into left/right: `L = M+S`, `R = M−S`.
pub fn ms_decode(mid: &[f32], side: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let n = mid.len().min(side.len());
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        left.push(mid[i] + side[i]);
        right.push(mid[i] - side[i]);
    }
    (left, right)
}

/// Scale stereo width by factoring through mid/side.
///
/// - `width = 0` collapses to mono (identical L/R)
/// - `width = 1` is a no-op (identity)
/// - `width > 1` widens; `width < 1` narrows
pub fn stereo_width(left: &[f32], right: &[f32], width: f32) -> (Vec<f32>, Vec<f32>) {
    let (mid, side) = ms_encode(left, right);
    let w = width.max(0.0);
    let side: Vec<f32> = side.into_iter().map(|s| s * w).collect();
    ms_decode(&mid, &side)
}

/// Sum to mono below `cutoff_hz`, leave the highs stereo.
///
/// Same crossover approach as vinyl elliptical mono — useful as a general
/// mono-maker for bass mono / phase-cleanup, not only RIAA chains.
pub fn mono_below(
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    cutoff_hz: f32,
) -> (Vec<f32>, Vec<f32>) {
    elliptical_mono(left, right, sample_rate, cutoff_hz)
}

/// Mono → pseudo-stereo via a short Haas delay on the right channel (~0.7 ms).
///
/// Pure delay decorrelation keeps mono-sum clean at low frequencies while
/// giving a simple stereo image for dry mono sources.
pub fn upmix_mono(mono: &[f32], sample_rate: u32) -> (Vec<f32>, Vec<f32>) {
    if mono.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let delay = ((sample_rate as f32 * 0.0007).round() as usize).max(1);
    let mut right = vec![0.0f32; mono.len()];
    right[delay..].copy_from_slice(&mono[..mono.len() - delay]);
    // Mild low-pass on the delayed leg softens comb notches when summed.
    let right = lowpass(&right, sample_rate, (sample_rate as f32 * 0.45).min(18_000.0));
    (mono.to_vec(), right)
}

/// Apply a fractional Haas inter-channel delay (ms) for subtle widening.
///
/// Positive `delay_ms` delays the right channel; negative delays the left.
/// Returns `(left, right)` of equal length to the shorter input.
pub fn haas_delay(
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    delay_ms: f32,
) -> (Vec<f32>, Vec<f32>) {
    let n = left.len().min(right.len());
    if n == 0 || sample_rate == 0 {
        return (left[..n].to_vec(), right[..n].to_vec());
    }
    let lag = (delay_ms / 1000.0) * sample_rate as f32;
    if lag.abs() < 1e-6 {
        return (left[..n].to_vec(), right[..n].to_vec());
    }
    if lag > 0.0 {
        (left[..n].to_vec(), shift_fractional(&right[..n], lag))
    } else {
        (shift_fractional(&left[..n], -lag), right[..n].to_vec())
    }
}

/// Zero-lag normalised correlation of L and R in `[-1, +1]`.
///
/// - near `+1` — in phase / highly mono-compatible
/// - near `0` — uncorrelated (wide or noisy)
/// - near `−1` — out of phase (mono sum cancels)
pub fn phase_correlation(left: &[f32], right: &[f32]) -> f32 {
    let n = left.len().min(right.len());
    if n == 0 {
        return 0.0;
    }
    let mut sum_lr = 0.0f64;
    let mut sum_l2 = 0.0f64;
    let mut sum_r2 = 0.0f64;
    for i in 0..n {
        let l = left[i] as f64;
        let r = right[i] as f64;
        sum_lr += l * r;
        sum_l2 += l * l;
        sum_r2 += r * r;
    }
    let denom = (sum_l2 * sum_r2).sqrt();
    if denom < 1e-20 {
        return 0.0;
    }
    (sum_lr / denom).clamp(-1.0, 1.0) as f32
}

/// Linear-interpolation fractional delay (positive lag advances the read head).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(sr: u32, n: usize, hz: f32) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / sr as f32).sin()).collect()
    }

    #[test]
    fn ms_roundtrip_is_identity() {
        let sr = 48_000u32;
        let n = 2048usize;
        let l = tone(sr, n, 440.0);
        let r = tone(sr, n, 554.0);
        let (m, s) = ms_encode(&l, &r);
        let (l2, r2) = ms_decode(&m, &s);
        let err_l: f32 = l.iter().zip(&l2).map(|(a, b)| (a - b).abs()).sum::<f32>() / n as f32;
        let err_r: f32 = r.iter().zip(&r2).map(|(a, b)| (a - b).abs()).sum::<f32>() / n as f32;
        assert!(err_l < 1e-6 && err_r < 1e-6, "roundtrip err L={err_l} R={err_r}");
    }

    #[test]
    fn width_zero_collapses_to_mono() {
        let l = vec![0.5f32, -0.2, 0.8, 0.1];
        let r = vec![-0.3f32, 0.4, -0.1, 0.6];
        let (ol, or) = stereo_width(&l, &r, 0.0);
        for i in 0..l.len() {
            assert!((ol[i] - or[i]).abs() < 1e-6, "not mono at {i}");
        }
    }

    #[test]
    fn width_one_is_identity() {
        let l = vec![0.5f32, -0.2, 0.8];
        let r = vec![-0.3f32, 0.4, -0.1];
        let (ol, or) = stereo_width(&l, &r, 1.0);
        for i in 0..l.len() {
            assert!((ol[i] - l[i]).abs() < 1e-6);
            assert!((or[i] - r[i]).abs() < 1e-6);
        }
    }

    #[test]
    fn mono_below_collapses_lows() {
        let sr = 48_000u32;
        let n = sr as usize * 2;
        // In-phase lows at different levels collapse to a shared mono low band.
        let l = tone(sr, n, 60.0);
        let r: Vec<f32> = l.iter().map(|x| x * 0.8).collect();
        let (ol, or) = mono_below(&l, &r, sr, 200.0);
        let out_low_l = lowpass(&ol, sr, 200.0);
        let out_low_r = lowpass(&or, sr, 200.0);
        let mean_diff: f32 =
            out_low_l.iter().zip(&out_low_r).map(|(a, b)| (a - b).abs()).sum::<f32>() / n as f32;
        assert!(mean_diff < 0.08, "lows not mono, mean |L-R|={mean_diff}");
    }

    #[test]
    fn phase_correlation_detects_polarity() {
        let a = vec![0.3f32, -0.5, 0.8, 0.1, -0.2];
        assert!((phase_correlation(&a, &a) - 1.0).abs() < 1e-5);
        let inv: Vec<f32> = a.iter().map(|x| -x).collect();
        assert!((phase_correlation(&a, &inv) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn upmix_produces_two_channels() {
        let sr = 48_000u32;
        let mono = tone(sr, 4800, 220.0);
        let (l, r) = upmix_mono(&mono, sr);
        assert_eq!(l.len(), mono.len());
        assert_eq!(r.len(), mono.len());
        // Delayed right should not be identical to left.
        let same = l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(!same, "upmix should decorrelate");
    }
}
