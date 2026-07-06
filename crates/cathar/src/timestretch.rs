//! Time-stretching and pitch-shifting.
//!
//! [`time_stretch`] changes duration while preserving pitch; [`pitch_shift`]
//! changes pitch while preserving duration (stretch, then resample back). Two
//! engines are offered via [`StretchMode`]:
//!
//! - **WSOLA** (default) — waveform-similarity overlap-add (Verhelst & Roelands):
//!   overlap-add of analysis frames whose read position is nudged within a small
//!   tolerance to best correlate with the previous frame's natural continuation.
//!   No FFT, robust on percussive/transient material.
//! - **Phase vocoder** — STFT with per-bin phase propagation from instantaneous
//!   frequency; smoother on sustained/tonal material.
//!
//! Both are deterministic and pure Rust.

use crate::resample;
use crate::util::hann_window;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

/// Which time-stretch engine to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StretchMode {
    /// Waveform-similarity overlap-add — robust default, no FFT.
    #[default]
    Wsola,
    /// Phase vocoder — smoother on tonal material.
    PhaseVocoder,
}

const FRAME: usize = 2048;
const SYN_HOP: usize = FRAME / 4;
const TOLERANCE: usize = 512;

/// Stretch `signal` in time by `ratio` (output duration ÷ input duration):
/// `ratio > 1.0` lengthens (slows down), `< 1.0` shortens (speeds up); pitch is
/// preserved. Returns a buffer of length ≈ `signal.len() * ratio`.
pub fn time_stretch(signal: &[f32], sample_rate: u32, ratio: f32, mode: StretchMode) -> Vec<f32> {
    let _ = sample_rate; // engines work in samples; kept for API symmetry
    if signal.len() < FRAME || !ratio.is_finite() || ratio <= 0.0 {
        return signal.to_vec();
    }
    if (ratio - 1.0).abs() < 1e-4 {
        return signal.to_vec();
    }
    let target = (signal.len() as f32 * ratio).round() as usize;
    let mut out = match mode {
        StretchMode::Wsola => wsola(signal, ratio),
        StretchMode::PhaseVocoder => phase_vocoder(signal, ratio),
    };
    out.truncate(target.min(out.len()));
    out
}

/// Shift `signal` up by `semitones` (negative = down) while preserving duration.
/// Implemented as [`time_stretch`] by the pitch ratio followed by resampling
/// back to the original length.
pub fn pitch_shift(
    signal: &[f32],
    sample_rate: u32,
    semitones: f32,
    mode: StretchMode,
) -> Vec<f32> {
    if signal.is_empty() || !semitones.is_finite() || semitones.abs() < 1e-4 {
        return signal.to_vec();
    }
    let factor = 2f32.powf(semitones / 12.0);
    let stretched = time_stretch(signal, sample_rate, factor, mode);
    if stretched.is_empty() {
        return signal.to_vec();
    }
    // Resample by len(stretched) → len(signal): compresses by ~1/factor, which
    // restores the original length and raises pitch by `factor`.
    resample(&stretched, stretched.len() as u32, signal.len() as u32)
}

/// WSOLA: overlap-add with per-frame read-position search for waveform
/// continuity. `ratio` is the time-stretch factor (output ÷ input duration).
fn wsola(x: &[f32], ratio: f32) -> Vec<f32> {
    let win = hann_window(FRAME);
    let ana_hop = SYN_HOP as f32 / ratio; // input advance per synthesis frame
    let out_len = (x.len() as f32 * ratio).ceil() as usize + FRAME;
    let mut y = vec![0.0f32; out_len];
    let mut norm = vec![0.0f32; out_len];

    // "Natural progression": the segment that should follow the last written
    // frame; the next analysis frame is chosen to best correlate with it.
    let mut nat_prog: Vec<f32> = x[..FRAME].to_vec();
    let mut syn_pos = 0usize;
    let mut ana_ideal = 0.0f32;

    loop {
        let center = ana_ideal.round() as i64;
        let lo = (center - TOLERANCE as i64).max(0);
        let hi = (center + TOLERANCE as i64).min((x.len() - FRAME) as i64);
        if hi < lo || syn_pos + FRAME > out_len {
            break;
        }
        // Pick the offset whose frame best correlates with the natural progression.
        let mut best = lo;
        let mut best_score = f32::MIN;
        let mut p = lo;
        while p <= hi {
            let seg = &x[p as usize..p as usize + FRAME];
            let mut score = 0.0f32;
            for i in 0..FRAME {
                score += seg[i] * nat_prog[i];
            }
            if score > best_score {
                best_score = score;
                best = p;
            }
            p += 1;
        }
        let a = best as usize;
        for i in 0..FRAME {
            y[syn_pos + i] += x[a + i] * win[i];
            norm[syn_pos + i] += win[i];
        }
        // The natural progression for the next step is what follows `a` by SYN_HOP.
        let np = a + SYN_HOP;
        if np + FRAME <= x.len() {
            nat_prog.copy_from_slice(&x[np..np + FRAME]);
        }
        syn_pos += SYN_HOP;
        ana_ideal += ana_hop;
        if ana_ideal.round() as usize + FRAME + TOLERANCE >= x.len() {
            break;
        }
    }

    for i in 0..out_len {
        if norm[i] > 1e-6 {
            y[i] /= norm[i];
        }
    }
    y
}

fn princarg(phase: f32) -> f32 {
    let two_pi = 2.0 * std::f32::consts::PI;
    phase - two_pi * (phase / two_pi).round()
}

/// Phase-vocoder time stretch with instantaneous-frequency phase propagation.
fn phase_vocoder(x: &[f32], ratio: f32) -> Vec<f32> {
    let ana_hop = FRAME / 4;
    let syn_hop = (ana_hop as f32 * ratio).round().max(1.0) as usize;
    let bins = FRAME / 2 + 1;
    let win = hann_window(FRAME);
    let two_pi = 2.0 * std::f32::consts::PI;
    let omega: Vec<f32> = (0..bins).map(|b| two_pi * b as f32 / FRAME as f32).collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FRAME);
    let c2r = planner.plan_fft_inverse(FRAME);
    let mut in_buf = r2c.make_input_vec();
    let mut spec = r2c.make_output_vec();
    let mut syn_spec = c2r.make_input_vec();
    let mut time_buf = c2r.make_output_vec();

    let mut prev_phase = vec![0.0f32; bins];
    let mut sum_phase = vec![0.0f32; bins];

    let out_len = (x.len() as f32 * ratio).ceil() as usize + FRAME;
    let mut y = vec![0.0f32; out_len];
    let mut norm = vec![0.0f32; out_len];

    let mut read = 0usize;
    let mut write = 0usize;
    let mut first = true;
    while read + FRAME <= x.len() && write + FRAME <= out_len {
        for i in 0..FRAME {
            in_buf[i] = x[read + i] * win[i];
        }
        r2c.process(&mut in_buf, &mut spec).expect("pv forward");

        for b in 0..bins {
            let mag = (spec[b].re * spec[b].re + spec[b].im * spec[b].im).sqrt();
            let phase = spec[b].im.atan2(spec[b].re);
            if first {
                sum_phase[b] = phase;
            } else {
                let delta = princarg(phase - prev_phase[b] - omega[b] * ana_hop as f32);
                let advance = omega[b] * ana_hop as f32 + delta; // true advance over ana_hop
                sum_phase[b] += advance * (syn_hop as f32 / ana_hop as f32);
            }
            prev_phase[b] = phase;
            syn_spec[b] = Complex::new(mag * sum_phase[b].cos(), mag * sum_phase[b].sin());
        }
        // c2r needs real DC and Nyquist.
        syn_spec[0].im = 0.0;
        syn_spec[bins - 1].im = 0.0;
        c2r.process(&mut syn_spec, &mut time_buf).expect("pv inverse");

        let scale = 1.0 / FRAME as f32;
        for i in 0..FRAME {
            y[write + i] += time_buf[i] * scale * win[i];
            norm[write + i] += win[i] * win[i];
        }
        read += ana_hop;
        write += syn_hop;
        first = false;
    }

    for i in 0..out_len {
        if norm[i] > 1e-6 {
            y[i] /= norm[i];
        }
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin()).collect()
    }

    fn crossing_rate(x: &[f32], sr: u32) -> f32 {
        let crossings = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 / (x.len() as f32 / sr as f32)
    }

    #[test]
    fn passthrough_when_ratio_is_one() {
        let sr = 48_000;
        let x = tone(440.0, sr, 8192);
        assert_eq!(time_stretch(&x, sr, 1.0, StretchMode::Wsola), x);
    }

    #[test]
    fn wsola_lengthens_and_preserves_pitch() {
        let sr = 48_000;
        let x = tone(440.0, sr, 24_000);
        let out = time_stretch(&x, sr, 1.5, StretchMode::Wsola);
        // Length ≈ 1.5×.
        let got = out.len() as f32 / x.len() as f32;
        assert!((got - 1.5).abs() < 0.02, "length ratio {got}");
        // Pitch (crossing rate) preserved.
        let (a, b) = (crossing_rate(&x, sr), crossing_rate(&out, sr));
        assert!((a - b).abs() / a < 0.1, "pitch drifted {a} -> {b}");
    }

    #[test]
    fn phase_vocoder_shortens_and_preserves_pitch() {
        let sr = 48_000;
        let x = tone(660.0, sr, 24_000);
        let out = time_stretch(&x, sr, 0.7, StretchMode::PhaseVocoder);
        let got = out.len() as f32 / x.len() as f32;
        assert!((got - 0.7).abs() < 0.03, "length ratio {got}");
        let (a, b) = (crossing_rate(&x, sr), crossing_rate(&out, sr));
        assert!((a - b).abs() / a < 0.1, "pitch drifted {a} -> {b}");
    }

    #[test]
    fn pitch_shift_up_octave_preserves_length_doubles_pitch() {
        let sr = 48_000;
        let x = tone(300.0, sr, 24_000);
        let out = pitch_shift(&x, sr, 12.0, StretchMode::Wsola);
        // Duration preserved (within a few samples).
        let got = out.len() as f32 / x.len() as f32;
        assert!((got - 1.0).abs() < 0.02, "length changed {got}");
        // Pitch roughly doubled.
        let ratio = crossing_rate(&out, sr) / crossing_rate(&x, sr);
        assert!((ratio - 2.0).abs() < 0.2, "pitch ratio {ratio}");
    }
}
