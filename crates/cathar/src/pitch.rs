//! Monophonic pitch detection via the YIN algorithm (de Cheveigné & Kawahara).
//!
//! For each analysis frame: difference function → cumulative-mean-normalised
//! difference → absolute-threshold trough pick → parabolic interpolation. The
//! per-frame `f0` (0.0 = unvoiced) feeds `stats` and later pitch work.

const FRAME: usize = 2048;
const THRESHOLD: f32 = 0.15;
const TAU_MIN: usize = 2;

/// Estimate `f0` (Hz) for a single frame; returns 0.0 when unvoiced.
fn yin_frame(frame: &[f32], sample_rate: u32) -> f32 {
    let w = frame.len();
    let tau_max = w / 2;
    if tau_max <= TAU_MIN {
        return 0.0;
    }
    // Silent / near-silent frames are unvoiced (the difference function is
    // degenerate at zero energy and would otherwise report a spurious pitch).
    let energy: f32 = frame.iter().map(|s| s * s).sum();
    if energy < 1e-6 {
        return 0.0;
    }

    // Difference function d(tau).
    let mut d = vec![0.0f32; tau_max];
    for tau in 1..tau_max {
        let mut sum = 0.0f32;
        for j in 0..(w - tau_max) {
            let diff = frame[j] - frame[j + tau];
            sum += diff * diff;
        }
        d[tau] = sum;
    }

    // Cumulative mean normalised difference d'(tau).
    let mut dp = vec![1.0f32; tau_max];
    let mut running = 0.0f32;
    for tau in 1..tau_max {
        running += d[tau];
        dp[tau] = d[tau] * tau as f32 / running.max(1e-9);
    }

    // Absolute threshold: first trough below THRESHOLD.
    let mut tau_est = 0usize;
    let mut t = TAU_MIN;
    while t < tau_max {
        if dp[t] < THRESHOLD {
            while t + 1 < tau_max && dp[t + 1] < dp[t] {
                t += 1;
            }
            tau_est = t;
            break;
        }
        t += 1;
    }
    if tau_est == 0 {
        return 0.0; // unvoiced
    }

    // Parabolic interpolation of the trough for sub-sample precision.
    let refined = if tau_est >= 1 && tau_est + 1 < tau_max {
        let (s0, s1, s2) = (dp[tau_est - 1], dp[tau_est], dp[tau_est + 1]);
        let denom = 2.0 * (s0 - 2.0 * s1 + s2);
        if denom.abs() < 1e-9 { tau_est as f32 } else { tau_est as f32 + (s0 - s2) / denom }
    } else {
        tau_est as f32
    };

    if refined > 0.0 { sample_rate as f32 / refined } else { 0.0 }
}

/// Per-frame fundamental frequency (Hz), stepping by `hop` samples. `0.0`
/// marks an unvoiced frame. An empty vector is returned for signals shorter
/// than one analysis frame.
pub fn detect_pitch(signal: &[f32], sample_rate: u32, hop: usize) -> Vec<f32> {
    if signal.len() < FRAME {
        return Vec::new();
    }
    let hop = hop.max(1);
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + FRAME <= signal.len() {
        out.push(yin_frame(&signal[pos..pos + FRAME], sample_rate));
        pos += hop;
    }
    out
}

/// A single dominant fundamental for the whole clip: the median of the voiced
/// frames. Returns `None` when nothing voiced is found.
pub fn fundamental_hz(signal: &[f32], sample_rate: u32) -> Option<f32> {
    let mut voiced: Vec<f32> =
        detect_pitch(signal, sample_rate, FRAME / 2).into_iter().filter(|&f| f > 0.0).collect();
    if voiced.is_empty() {
        return None;
    }
    voiced.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(voiced[voiced.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin()).collect()
    }

    #[test]
    fn detects_pure_tones() {
        let sr = 48_000;
        for &f in &[110.0f32, 220.0, 440.0, 880.0] {
            let x = tone(f, sr, 24_000);
            let est = fundamental_hz(&x, sr).expect("voiced");
            assert!((est - f).abs() / f < 0.02, "want {f}, got {est}");
        }
    }

    #[test]
    fn silence_is_unvoiced() {
        let sr = 48_000;
        assert_eq!(fundamental_hz(&vec![0.0f32; 24_000], sr), None);
    }

    #[test]
    fn short_signal_is_empty() {
        assert!(detect_pitch(&[0.0; 100], 48_000, 256).is_empty());
    }
}
