//! De-crackle — suppress the dense field of low-amplitude surface crackle on
//! vinyl captures, distinct from `declick`'s isolated impulses.
//!
//! A second-difference (Laplacian) detector emphasises the impulsive
//! high-frequency crackle; samples whose detector value exceeds a running
//! noise-floor estimate by a sensitivity-controlled factor are flagged, and each
//! flagged micro-run is repaired with cubic-Hermite interpolation from its
//! surviving neighbours. Deterministic, pure Rust.

/// Suppress dense low-level crackle. `sensitivity` in `1..=10` (higher removes
/// more; 5 is a sensible default).
pub fn decrackle(signal: &[f32], sample_rate: u32, sensitivity: f32) -> Vec<f32> {
    let _ = sample_rate; // detector is rate-independent; kept for API symmetry
    let n = signal.len();
    if n < 8 {
        return signal.to_vec();
    }
    let sensitivity = sensitivity.clamp(1.0, 10.0);
    // Higher sensitivity → lower threshold factor → more samples flagged.
    let factor = 12.0 - sensitivity; // 11 (gentle) … 2 (aggressive)

    // Detector: |second difference|, which peaks on impulsive crackle and stays
    // small on smooth programme material.
    let mut det = vec![0.0f32; n];
    for i in 1..n - 1 {
        det[i] = signal[i] - 0.5 * (signal[i - 1] + signal[i + 1]);
    }

    // Flag samples exceeding a forward-EMA floor of the detector magnitude.
    let alpha = 0.001f32;
    let mut floor = 1e-6f32;
    let mut flags = vec![false; n];
    for i in 0..n {
        let a = det[i].abs();
        floor = floor * (1.0 - alpha) + a * alpha;
        if a > factor * floor.max(1e-6) {
            flags[i] = true;
        }
    }

    // Repair each flagged run.
    let mut out = signal.to_vec();
    let mut i = 0;
    while i < n {
        if flags[i] {
            let s = i;
            while i < n && flags[i] {
                i += 1;
            }
            repair_hermite(&mut out, s, i);
        } else {
            i += 1;
        }
    }
    out
}

/// Replace `out[s..e]` with a cubic-Hermite curve anchored at the surviving
/// neighbours `out[s-1]` and `out[e]`, matching their local slopes.
fn repair_hermite(out: &mut [f32], s: usize, e: usize) {
    let n = out.len();
    if s == 0 || e >= n {
        // Edge run: fall back to linear from whichever anchor exists.
        let left = if s > 0 { out[s - 1] } else { out[e.min(n - 1)] };
        let right = if e < n { out[e] } else { out[s.saturating_sub(1)] };
        let span = (e - s + 1) as f32;
        for (k, o) in out[s..e].iter_mut().enumerate() {
            let t = (k + 1) as f32 / span;
            *o = left * (1.0 - t) + right * t;
        }
        return;
    }
    let p0 = out[s - 1];
    let p1 = out[e];
    let m0 = if s >= 2 { out[s - 1] - out[s - 2] } else { 0.0 };
    let m1 = if e + 1 < n { out[e + 1] - out[e] } else { 0.0 };
    let span = (e - (s - 1)) as f32; // steps from anchor s-1 to anchor e
    for (k, o) in out[s..e].iter_mut().enumerate() {
        let t = (k + 1) as f32 / span; // idx − (s−1) = k + 1
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        // Tangents are per-sample slopes scaled to the [0,1] parameterisation.
        *o = h00 * p0 + h10 * (m0 * span) + h01 * p1 + h11 * (m1 * span);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_crackle_keeps_tone() {
        let sr = 48_000usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        let clean: Vec<f32> =
            (0..sr).map(|i| 0.4 * (two_pi * 500.0 * i as f32 / sr as f32).sin()).collect();
        // Sprinkle sparse impulse crackle with a seeded xorshift.
        let mut rng = 0x1234_5678u64;
        let mut rnd = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as f32 / u64::MAX as f32) - 0.5
        };
        let mut noisy = clean.clone();
        for _ in 0..600 {
            let pos = (rnd().abs() * (sr as f32 - 4.0)) as usize + 1;
            noisy[pos] += if rnd() > 0.0 { 0.5 } else { -0.5 };
        }
        let out = decrackle(&noisy, sr as u32, 6.0);

        let err = |x: &[f32]| x.iter().zip(&clean).map(|(a, b)| (a - b).powi(2)).sum::<f32>();
        assert!(err(&out) < err(&noisy) * 0.5, "crackle not reduced");

        // Tone preserved (one-bin DFT magnitude).
        let mag = |x: &[f32]| {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate() {
                let p = 2.0 * std::f64::consts::PI * 500.0 * i as f64 / sr as f64;
                re += v as f64 * p.cos();
                im -= v as f64 * p.sin();
            }
            (re * re + im * im).sqrt()
        };
        assert!(mag(&out) > mag(&clean) * 0.8, "tone not preserved");
    }

    #[test]
    fn clean_signal_barely_changes() {
        let sr = 48_000usize;
        let clean: Vec<f32> = (0..sr)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let out = decrackle(&clean, sr as u32, 5.0);
        let diff: f32 = out.iter().zip(&clean).map(|(a, b)| (a - b).abs()).sum::<f32>() / sr as f32;
        assert!(diff < 0.02, "clean signal altered too much: {diff}");
    }
}
