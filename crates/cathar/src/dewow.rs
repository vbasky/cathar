//! Wow & flutter correction — remove slow pitch instability from tape/vinyl
//! captures by tracking a dominant sustained tone and time-warping the signal to
//! flatten its instantaneous frequency.
//!
//! A uniform speed error scales every partial's frequency by the same ratio, so
//! measuring one strong component's instantaneous frequency recovers the speed
//! curve `s(t)`. The signal is then resampled with the time-varying inverse rate
//! (`φ(t)=∫s`, sampled at `φ⁻¹`) so the corrected output plays at constant
//! pitch. Deterministic, pure Rust. Best on material with a stable reference
//! pitch — the classic wow/flutter case.

use crate::fundamental_hz;

/// Correct wow & flutter. Returns a pitch-stabilised signal (~same length); if
/// no stable reference pitch is found the input is returned unchanged.
pub fn dewow(signal: &[f32], sample_rate: u32) -> Vec<f32> {
    let n = signal.len();
    if n < 2048 || sample_rate == 0 {
        return signal.to_vec();
    }
    let sr = sample_rate as f32;
    let f0 = match fundamental_hz(signal, sample_rate) {
        Some(f) if f >= 20.0 && f < sr * 0.4 => f,
        _ => return signal.to_vec(),
    };
    let two_pi = 2.0 * std::f32::consts::PI;

    // Heterodyne to baseband around f0 (I/Q), then low-pass to isolate the
    // component and its slow modulation.
    let w = two_pi * f0 / sr;
    let mut re = vec![0.0f32; n];
    let mut im = vec![0.0f32; n];
    let mut phase = 0.0f64;
    let wd = w as f64;
    for i in 0..n {
        let (s, c) = phase.sin_cos();
        re[i] = signal[i] * c as f32;
        im[i] = -signal[i] * s as f32;
        phase += wd;
        if phase > std::f64::consts::TAU {
            phase -= std::f64::consts::TAU;
        }
    }
    let baseband_fc = (f0 * 0.3).clamp(10.0, 60.0);
    let lp_b = 1.0 - (-two_pi * baseband_fc / sr).exp();
    lpf_zero_phase(&mut re, lp_b);
    lpf_zero_phase(&mut im, lp_b);

    // Instantaneous frequency from the baseband phase increment; speed = f/f0.
    let mut speed = vec![1.0f32; n];
    for i in 1..n {
        // arg(b[i] · conj(b[i-1]))
        let dr = re[i] * re[i - 1] + im[i] * im[i - 1];
        let di = im[i] * re[i - 1] - re[i] * im[i - 1];
        let dphi = di.atan2(dr); // radians per sample of the baseband
        let inst = f0 + dphi * sr / two_pi;
        speed[i] = (inst / f0).clamp(0.8, 1.2);
    }
    speed[0] = speed[1];

    // Smooth the speed curve (keep flutter up to a few tens of Hz), then
    // normalise its mean to 1 so only the modulation — not the average rate —
    // is removed.
    let sp_fc = 30.0f32;
    let lp_s = 1.0 - (-two_pi * sp_fc / sr).exp();
    lpf_zero_phase(&mut speed, lp_s);
    let mean = speed.iter().sum::<f32>() / n as f32;
    if mean.abs() < 1e-6 {
        return signal.to_vec();
    }
    for s in &mut speed {
        *s /= mean;
    }

    // Cumulative warp φ (length n+1) and resample the input at φ⁻¹(m).
    let mut phi = vec![0.0f32; n + 1];
    for i in 0..n {
        phi[i + 1] = phi[i] + speed[i];
    }
    let m_max = phi[n].floor() as usize;
    let mut out = Vec::with_capacity(m_max);
    let mut i = 0usize;
    for m in 0..m_max {
        let u = m as f32;
        while i + 1 < n && phi[i + 1] <= u {
            i += 1;
        }
        let t = i as f32 + (u - phi[i]) / speed[i].max(1e-6);
        // Linear interpolation at input position t.
        let j = t.floor() as usize;
        let frac = t - j as f32;
        let a = signal[j.min(n - 1)];
        let b = signal[(j + 1).min(n - 1)];
        out.push(a * (1.0 - frac) + b * frac);
    }
    out
}

/// Forward–backward one-pole low-pass (zero phase).
fn lpf_zero_phase(x: &mut [f32], lam: f32) {
    if x.is_empty() {
        return;
    }
    let mut y = x[0];
    for v in x.iter_mut() {
        y += lam * (*v - y);
        *v = y;
    }
    y = *x.last().unwrap();
    for v in x.iter_mut().rev() {
        y += lam * (*v - y);
        *v = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One-bin DFT magnitude at frequency `f`.
    fn mag_at(x: &[f32], f: f32, sr: u32) -> f64 {
        let two_pi = 2.0 * std::f64::consts::PI;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate() {
            let p = two_pi * f as f64 * i as f64 / sr as f64;
            re += v as f64 * p.cos();
            im -= v as f64 * p.sin();
        }
        (re * re + im * im).sqrt() / x.len() as f64
    }

    #[test]
    fn flattens_wowed_tone() {
        let sr = 48_000u32;
        let n = sr as usize * 2;
        let f0 = 440.0f32;
        let two_pi = 2.0 * std::f32::consts::PI;
        // A 440 Hz tone warped by a 3 Hz, ±3 % wow.
        let mut phase = 0.0f32;
        let mut x = vec![0.0f32; n];
        for (i, xi) in x.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            let speed = 1.0 + 0.03 * (two_pi * 3.0 * t).sin();
            phase += two_pi * f0 * speed / sr as f32;
            *xi = phase.sin();
        }
        let out = dewow(&x, sr);

        // Wow spreads energy into sidebands; correction pulls it back to f0.
        let before = mag_at(&x, f0, sr);
        let after = mag_at(&out, f0, sr);
        assert!(after > before * 1.3, "carrier not restored: {before} -> {after}");
    }

    #[test]
    fn clean_tone_survives() {
        let sr = 48_000u32;
        let n = sr as usize;
        let f0 = 440.0f32;
        let x: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * f0 * i as f32 / sr as f32).sin())
            .collect();
        let out = dewow(&x, sr);
        let before = mag_at(&x, f0, sr);
        let after = mag_at(&out, f0, sr);
        // A clean tone should stay clean (carrier roughly preserved).
        assert!(after > before * 0.8, "clean tone degraded: {before} -> {after}");
    }
}
