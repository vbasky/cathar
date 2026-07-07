//! Adaptive de-hum — track a drifting mains fundamental and per-harmonic
//! amplitude, instead of the fixed notches in `dehum`.
//!
//! The precise fundamental near the nominal (50/60 Hz) is found from a spectral
//! peak. Each harmonic is then cancelled by an I/Q heterodyne canceller:
//! demodulate the signal by cos/sin at the harmonic frequency to DC, low-pass
//! the in-phase/quadrature amplitudes (a zero-phase one-pole), remodulate and
//! subtract. The low-pass tracks slow amplitude *and* small frequency drift
//! (which appears as a slowly rotating I/Q vector). Deterministic, pure Rust.

use realfft::RealFftPlanner;

/// Adaptive mains-hum removal. `base_freq` is the nominal (50 or 60);
/// `num_harmonics` harmonics are tracked and cancelled.
pub fn dehum_adaptive(
    signal: &[f32],
    sample_rate: u32,
    base_freq: f32,
    num_harmonics: usize,
) -> Vec<f32> {
    let n = signal.len();
    if n < 64 || sample_rate == 0 {
        return signal.to_vec();
    }
    let sr = sample_rate as f64;
    let two_pi = 2.0 * std::f64::consts::PI;
    let f0 = estimate_f0(signal, sample_rate, base_freq) as f64;

    // ~2 Hz one-pole amplitude tracker.
    let fc = 2.0;
    let lam = 1.0 - (-two_pi * fc / sr).exp();
    let lam = lam as f32;

    let mut out = signal.to_vec();
    for h in 1..=num_harmonics {
        let freq = f0 * h as f64;
        if freq >= sr * 0.5 {
            break;
        }
        let w = two_pi * freq / sr;

        // Demodulate to DC: in-phase and quadrature products.
        let mut pc = vec![0.0f32; n];
        let mut ps = vec![0.0f32; n];
        let mut phase = 0.0f64;
        for i in 0..n {
            let (s, c) = phase.sin_cos();
            pc[i] = out[i] * c as f32;
            ps[i] = out[i] * s as f32;
            phase += w;
            if phase > two_pi {
                phase -= two_pi;
            }
        }
        // Zero-phase low-pass → slowly-varying I/Q amplitudes.
        lpf_zero_phase(&mut pc, lam);
        lpf_zero_phase(&mut ps, lam);

        // Remodulate and subtract the reconstructed harmonic.
        let mut phase = 0.0f64;
        for i in 0..n {
            let (s, c) = phase.sin_cos();
            out[i] -= 2.0 * (pc[i] * c as f32 + ps[i] * s as f32);
            phase += w;
            if phase > two_pi {
                phase -= two_pi;
            }
        }
    }
    out
}

/// Forward–backward one-pole low-pass (zero phase), seeded at the edges to tame
/// start/end transients.
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

/// Locate the precise hum fundamental within ±2 Hz of `base_freq` from the
/// magnitude spectrum of a leading segment, with parabolic peak interpolation.
fn estimate_f0(signal: &[f32], sample_rate: u32, base_freq: f32) -> f32 {
    let n = signal.len();
    let mut len = n.min(1 << 16);
    len &= !1; // even
    if len < 64 {
        return base_freq;
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(len);
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();
    in_buf.copy_from_slice(&signal[..len]);
    if r2c.process(&mut in_buf, &mut out_buf).is_err() {
        return base_freq;
    }
    let hz_per_bin = sample_rate as f32 / len as f32;
    let lo = (((base_freq - 2.0) / hz_per_bin).floor() as usize).max(1);
    let hi = (((base_freq + 2.0) / hz_per_bin).ceil() as usize).min(out_buf.len() - 2);
    if hi <= lo {
        return base_freq;
    }
    let mag = |b: usize| (out_buf[b].re * out_buf[b].re + out_buf[b].im * out_buf[b].im).sqrt();
    let mut peak = lo;
    for b in lo..=hi {
        if mag(b) > mag(peak) {
            peak = b;
        }
    }
    // Parabolic interpolation of the peak bin.
    let (a, b, c) = (mag(peak - 1), mag(peak), mag(peak + 1));
    let denom = a - 2.0 * b + c;
    let delta = if denom.abs() > 1e-12 { 0.5 * (a - c) / denom } else { 0.0 };
    (peak as f32 + delta) * hz_per_bin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hum_band_energy(x: &[f32], sr: usize, f: f32) -> f64 {
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
    fn cancels_drifting_hum_keeps_tone() {
        let sr = 48_000usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        // 60 Hz hum whose frequency drifts ±1.5 Hz, plus a 1 kHz tone.
        let mut phase = 0.0f32;
        let mut x = vec![0.0f32; sr * 2];
        for (i, xi) in x.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            let f = 60.0 + 1.5 * (two_pi * 0.5 * t).sin();
            phase += two_pi * f / sr as f32;
            *xi = 0.5 * phase.sin() + 0.3 * (two_pi * 1000.0 * t).sin();
        }
        let out = dehum_adaptive(&x, sr as u32, 60.0, 3);
        let before = hum_band_energy(&x, sr, 60.0);
        let after = hum_band_energy(&out, sr, 60.0);
        assert!(after < before * 0.3, "hum not cancelled: {before} -> {after}");
        let tone_b = hum_band_energy(&x, sr, 1000.0);
        let tone_a = hum_band_energy(&out, sr, 1000.0);
        assert!(tone_a > tone_b * 0.8, "tone not preserved: {tone_b} -> {tone_a}");
    }
}
