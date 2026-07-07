//! Sinusoidal modeling (SMS / McAulay–Quatieri) — analysis-resynthesis.
//!
//! Analysis picks spectral peaks per STFT frame (parabolic-interpolated
//! frequency/amplitude), tracks them across frames into partials
//! (birth/continue/death by nearest-frequency matching), and resynthesis sums
//! each partial as a phase-continuous oscillator with linearly-interpolated
//! frequency and amplitude. Keeping only the tracked partials discards the
//! stochastic residual — a "tonal purify". Deterministic, pure Rust.

use crate::util::hann_window;
use realfft::RealFftPlanner;

const FFT: usize = 2048;
const HOP: usize = 512;
const MAX_PEAKS: usize = 80;

/// One tracked partial: a contiguous run of frames with per-frame frequency and
/// amplitude.
struct Partial {
    start: usize,
    freq: Vec<f32>,
    amp: Vec<f32>,
}

/// A sinusoidal model: the partials extracted from a signal.
pub struct SinusoidalModel {
    /// Source sample rate.
    pub sample_rate: u32,
    /// Hop between analysis frames, samples.
    pub hop: usize,
    /// Total number of analysis frames.
    pub frames: usize,
    partials: Vec<Partial>,
}

impl SinusoidalModel {
    /// Number of tracked partials.
    pub fn partial_count(&self) -> usize {
        self.partials.len()
    }
}

struct Peak {
    freq: f32,
    amp: f32,
}

/// Analyse `signal` into a [`SinusoidalModel`].
pub fn analyze_sms(signal: &[f32], sample_rate: u32) -> SinusoidalModel {
    let n = signal.len();
    if n < FFT {
        return SinusoidalModel { sample_rate, hop: HOP, frames: 0, partials: Vec::new() };
    }
    let sr = sample_rate as f32;
    let bins = FFT / 2 + 1;
    let win = hann_window(FFT);
    let win_sum: f32 = win.iter().sum::<f32>().max(1e-9);

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT);
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    // Per-frame peaks.
    let mut frame_peaks: Vec<Vec<Peak>> = Vec::new();
    let mut pos = 0;
    while pos + FFT <= n {
        for (i, s) in in_buf.iter_mut().enumerate() {
            *s = signal[pos + i] * win[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).expect("sms forward");
        let mag: Vec<f32> =
            out_buf.iter().map(|c| (c.re * c.re + c.im * c.im).sqrt() * 2.0 / win_sum).collect();
        frame_peaks.push(pick_peaks(&mag, sr, bins));
        pos += HOP;
    }
    let frames = frame_peaks.len();

    // Track partials by nearest-frequency continuation.
    let mut partials: Vec<Partial> = Vec::new();
    let mut active: Vec<usize> = Vec::new(); // indices into `partials`
    for (f, peaks) in frame_peaks.iter().enumerate() {
        let mut used = vec![false; peaks.len()];
        let mut still_active = Vec::new();
        for &pi in &active {
            let last_f = *partials[pi].freq.last().unwrap();
            let tol = (last_f * 0.03).max(30.0);
            // Nearest unused peak within tolerance.
            let mut best = None;
            let mut best_df = tol;
            for (qi, pk) in peaks.iter().enumerate() {
                if used[qi] {
                    continue;
                }
                let df = (pk.freq - last_f).abs();
                if df < best_df {
                    best_df = df;
                    best = Some(qi);
                }
            }
            if let Some(qi) = best {
                used[qi] = true;
                partials[pi].freq.push(peaks[qi].freq);
                partials[pi].amp.push(peaks[qi].amp);
                still_active.push(pi);
            }
            // else: partial dies (dropped from active).
        }
        // Birth new partials from unmatched peaks.
        for (qi, pk) in peaks.iter().enumerate() {
            if !used[qi] {
                partials.push(Partial { start: f, freq: vec![pk.freq], amp: vec![pk.amp] });
                still_active.push(partials.len() - 1);
            }
        }
        active = still_active;
    }

    SinusoidalModel { sample_rate, hop: HOP, frames, partials }
}

/// Resynthesise the signal from a [`SinusoidalModel`] by additive synthesis.
pub fn synthesize_sms(model: &SinusoidalModel) -> Vec<f32> {
    let sr = model.sample_rate as f32;
    let hop = model.hop;
    let len = if model.frames > 0 { (model.frames - 1) * hop + FFT } else { 0 };
    let mut out = vec![0.0f32; len];
    let two_pi = 2.0 * std::f32::consts::PI;

    for p in &model.partials {
        if p.freq.len() < 3 {
            continue; // too short to be a stable partial (drops noise fragments)
        }
        let mut phase = 0.0f32;
        for seg in 0..p.freq.len() - 1 {
            let (f0, f1) = (p.freq[seg], p.freq[seg + 1]);
            let (mut a0, mut a1) = (p.amp[seg], p.amp[seg + 1]);
            // Fade birth in and death out over one hop to avoid clicks.
            if seg == 0 {
                a0 = 0.0;
            }
            if seg == p.freq.len() - 2 {
                a1 = 0.0;
            }
            let base = (p.start + seg) * hop;
            for j in 0..hop {
                let frac = j as f32 / hop as f32;
                let freq = f0 + (f1 - f0) * frac;
                let amp = a0 + (a1 - a0) * frac;
                phase += two_pi * freq / sr;
                if base + j < len {
                    out[base + j] += amp * phase.sin();
                }
            }
        }
    }
    out
}

/// Pick spectral peaks (parabolic-interpolated), keeping the strongest.
fn pick_peaks(mag: &[f32], sr: f32, bins: usize) -> Vec<Peak> {
    let max_m = mag.iter().copied().fold(0.0f32, f32::max);
    let thresh = max_m * 0.01 + 1e-6;
    let mut peaks = Vec::new();
    for b in 1..bins - 1 {
        if mag[b] > thresh && mag[b] >= mag[b - 1] && mag[b] > mag[b + 1] {
            let (a, c) = (mag[b - 1], mag[b + 1]);
            let denom = a - 2.0 * mag[b] + c;
            let delta = if denom.abs() > 1e-12 { 0.5 * (a - c) / denom } else { 0.0 };
            let freq = (b as f32 + delta) * sr / FFT as f32;
            let amp = (mag[b] - 0.25 * (a - c) * delta).max(0.0);
            peaks.push(Peak { freq, amp });
        }
    }
    // Keep the strongest MAX_PEAKS.
    peaks.sort_by(|x, y| y.amp.partial_cmp(&x.amp).unwrap_or(std::cmp::Ordering::Equal));
    peaks.truncate(MAX_PEAKS);
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn resynthesises_two_tones() {
        let sr = 48_000u32;
        let n = sr as usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                0.5 * (two_pi * 440.0 * t).sin() + 0.3 * (two_pi * 660.0 * t).sin()
            })
            .collect();
        let model = analyze_sms(&x, sr);
        assert!(model.partial_count() >= 2, "expected ≥2 partials, got {}", model.partial_count());
        let out = synthesize_sms(&model);
        assert!(!out.is_empty());

        // Both tones reappear in the interior. Measure over a short window so the
        // small per-frame frequency jitter inherent in SMS doesn't smear the
        // one-bin DFT.
        let s = sr as usize / 2;
        let mid = &out[s..(s + 4096).min(out.len())];
        assert!(mag_at(mid, 440.0, sr) > 0.12, "440 missing: {}", mag_at(mid, 440.0, sr));
        assert!(mag_at(mid, 660.0, sr) > 0.05, "660 missing: {}", mag_at(mid, 660.0, sr));
    }

    #[test]
    fn purifies_noisy_tone() {
        let sr = 48_000u32;
        let n = sr as usize;
        let mut rng = 0x9E37_79B9_7F4A_7C15u64;
        let mut noise = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as f32 / u64::MAX as f32 - 0.5) * 0.4
        };
        let clean: Vec<f32> = (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 500.0 * i as f32 / sr as f32).sin())
            .collect();
        let noisy: Vec<f32> = clean.iter().map(|&c| c + noise()).collect();
        let out = synthesize_sms(&analyze_sms(&noisy, sr));
        // The 500 Hz tone survives; broadband noise (measured off-tone) drops.
        let s = sr as usize / 2;
        let mid = &out[s..(s + 4096).min(out.len())];
        assert!(mag_at(mid, 500.0, sr) > 0.12, "tone lost: {}", mag_at(mid, 500.0, sr));
        let off = mag_at(mid, 3333.0, sr);
        assert!(off < 0.03, "residual noise too high off-tone: {off}");
    }
}
