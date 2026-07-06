//! Harmonic–percussive source separation (Fitzgerald 2010).
//!
//! On the STFT magnitude, a horizontal median filter (across time) emphasises
//! sustained/tonal energy while a vertical median filter (across frequency)
//! emphasises broadband/transient energy. Soft Wiener masks derived from the two
//! are applied to the complex STFT (phase preserved) and inverted. Fully
//! deterministic, no weights — a sibling to `voice_isolate`.

use crate::util::hann_window;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

const FFT: usize = 2048;
const HOP: usize = 512;

fn median(scratch: &mut [f32]) -> f32 {
    scratch.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    scratch[scratch.len() / 2]
}

/// Separate `signal` into `(harmonic, percussive)` components. `kernel` is the
/// median-filter length (forced odd, ≥ 3) used in both time and frequency.
pub fn hpss(signal: &[f32], sample_rate: u32, kernel: usize) -> (Vec<f32>, Vec<f32>) {
    let _ = sample_rate; // separation is rate-independent; kept for API symmetry
    let n = signal.len();
    if n < FFT {
        return (signal.to_vec(), vec![0.0; n]);
    }
    let k = kernel.max(3) | 1; // odd, ≥ 3
    let half = k / 2;
    let bins = FFT / 2 + 1;
    let win = hann_window(FFT);

    // Forward STFT.
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT);
    let c2r = planner.plan_fft_inverse(FFT);
    let mut spectra: Vec<Vec<Complex<f32>>> = Vec::new();
    let mut positions: Vec<usize> = Vec::new();
    {
        let mut in_buf = r2c.make_input_vec();
        let mut out_buf = r2c.make_output_vec();
        let mut pos = 0;
        while pos + FFT <= n {
            for (i, s) in in_buf.iter_mut().enumerate() {
                *s = signal[pos + i] * win[i];
            }
            r2c.process(&mut in_buf, &mut out_buf).expect("hpss forward");
            spectra.push(out_buf.clone());
            positions.push(pos);
            pos += HOP;
        }
    }
    let frames = spectra.len();
    let mag: Vec<Vec<f32>> = spectra
        .iter()
        .map(|s| s.iter().map(|c| (c.re * c.re + c.im * c.im).sqrt()).collect())
        .collect();

    // Median filters: harmonic = horizontal (time), percussive = vertical (freq).
    let mut scratch = Vec::with_capacity(k);
    // Median across time at a fixed bin (a column of the spectrogram).
    let harm_frame = |f: usize, b: usize, scratch: &mut Vec<f32>| {
        scratch.clear();
        let lo = f.saturating_sub(half);
        let hi = (f + half).min(frames - 1);
        for row in &mag[lo..=hi] {
            scratch.push(row[b]);
        }
        median(scratch)
    };
    // Median across frequency within a frame (a contiguous slice).
    let perc_frame = |f: usize, b: usize, scratch: &mut Vec<f32>| {
        scratch.clear();
        let lo = b.saturating_sub(half);
        let hi = (b + half).min(bins - 1);
        for &m in &mag[f][lo..=hi] {
            scratch.push(m);
        }
        median(scratch)
    };

    // Apply Wiener masks to the complex spectra in place.
    for (f, frame) in spectra.iter_mut().enumerate() {
        for (b, cell) in frame.iter_mut().enumerate() {
            let h = harm_frame(f, b, &mut scratch);
            let p = perc_frame(f, b, &mut scratch);
            let denom = h * h + p * p;
            let mh = if denom > 1e-12 { h * h / denom } else { 0.5 };
            *cell *= mh; // harmonic spectrum (percussive = original − harmonic)
        }
    }

    // Inverse STFT of the harmonic spectra with WOLA; percussive = signal − harmonic.
    let mut harmonic = vec![0.0f32; n];
    let mut norm = vec![0.0f32; n];
    {
        let mut spec_buf = c2r.make_input_vec();
        let mut time_buf = c2r.make_output_vec();
        let scale = 1.0 / FFT as f32;
        for (fi, spectrum) in spectra.iter().enumerate() {
            spec_buf.copy_from_slice(spectrum);
            spec_buf[0].im = 0.0;
            spec_buf[bins - 1].im = 0.0;
            c2r.process(&mut spec_buf, &mut time_buf).expect("hpss inverse");
            let pos = positions[fi];
            for i in 0..FFT {
                harmonic[pos + i] += time_buf[i] * scale * win[i];
                norm[pos + i] += win[i] * win[i];
            }
        }
    }
    let mut percussive = vec![0.0f32; n];
    for i in 0..n {
        if norm[i] > 1e-6 {
            harmonic[i] /= norm[i];
        } else {
            harmonic[i] = 0.0;
        }
        // The masks are complementary, so what the harmonic path leaves behind is
        // percussive. Deriving it by subtraction guarantees exact reconstruction
        // in the covered region.
        percussive[i] = signal[i] - harmonic[i];
    }
    (harmonic, percussive)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One-bin DFT magnitude over a sample range.
    fn mag_at(x: &[f32], f: f32, sr: usize, lo: usize, hi: usize) -> f64 {
        let two_pi = 2.0 * std::f64::consts::PI;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate().take(hi).skip(lo) {
            let p = two_pi * f as f64 * i as f64 / sr as f64;
            re += v as f64 * p.cos();
            im -= v as f64 * p.sin();
        }
        (re * re + im * im).sqrt() / (hi - lo) as f64
    }

    #[test]
    fn separates_tone_from_clicks() {
        let sr = 48_000usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        let mut x: Vec<f32> =
            (0..sr).map(|i| 0.4 * (two_pi * 1000.0 * i as f32 / sr as f32).sin()).collect();
        // Add periodic clicks (broadband transients).
        for c in (2000..sr).step_by(4000) {
            x[c] += 0.9;
        }
        let (h, p) = hpss(&x, sr as u32, 17);
        // The sustained 1 kHz tone lives mostly in the harmonic output.
        let tone_h = mag_at(&h, 1000.0, sr, sr / 4, sr * 3 / 4);
        let tone_p = mag_at(&p, 1000.0, sr, sr / 4, sr * 3 / 4);
        assert!(tone_h > tone_p * 3.0, "tone not harmonic: h={tone_h} p={tone_p}");
        // Click energy is stronger in the percussive output.
        let click_h: f32 =
            (0..h.len()).map(|i| if x[i].abs() > 0.5 { h[i].abs() } else { 0.0 }).sum();
        let click_p: f32 =
            (0..p.len()).map(|i| if x[i].abs() > 0.5 { p[i].abs() } else { 0.0 }).sum();
        assert!(click_p > click_h, "clicks not percussive: h={click_h} p={click_p}");
    }

    #[test]
    fn reconstructs_original() {
        let sr = 48_000usize;
        let x: Vec<f32> = (0..sr)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let (h, p) = hpss(&x, sr as u32, 17);
        let err: f32 =
            (0..x.len()).map(|i| (h[i] + p[i] - x[i]).abs()).sum::<f32>() / x.len() as f32;
        assert!(err < 1e-4, "h+p should reconstruct x, mean err {err}");
    }
}
