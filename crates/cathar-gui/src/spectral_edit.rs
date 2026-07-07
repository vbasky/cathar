//! Selection-scoped spectral editing: apply a gain or a heal (interpolate-out)
//! to a time-frequency rectangle via an STFT round-trip.
//!
//! This is the piece a spectral editor needs that whole-file DSP does not: it
//! touches only the bins inside a user-drawn box, keeps phase, and reconstructs
//! the rest of the signal transparently through weighted overlap-add (WOLA).

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

const FFT_SIZE: usize = 2048;
const HOP: usize = FFT_SIZE / 4; // 75% overlap → constant Hann WOLA gain.

/// A time-frequency rectangle in physical units.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Selection {
    /// Start time, seconds.
    pub(crate) t0: f32,
    /// End time, seconds.
    pub(crate) t1: f32,
    /// Low edge, Hz.
    pub(crate) f0: f32,
    /// High edge, Hz.
    pub(crate) f1: f32,
}

/// What to do to the bins inside the selection.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SpectralOp {
    /// Scale magnitude (phase preserved). `< 1.0` attenuates, `> 1.0` boosts.
    Gain(f32),
    /// Interpolate each selected bin across time from the frames bordering the
    /// selection — paints out whistles/bursts using surrounding context.
    Heal,
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = std::f32::consts::PI * i as f32 / (n - 1) as f32;
            let s = x.sin();
            s * s
        })
        .collect()
}

/// Apply `op` to `signal` within `sel`. Returns a new buffer the same length.
pub(crate) fn apply_spectral(
    signal: &[f32],
    sample_rate: u32,
    sel: &Selection,
    op: SpectralOp,
) -> Vec<f32> {
    let n = signal.len();
    if n < FFT_SIZE {
        return signal.to_vec();
    }
    let bins = FFT_SIZE / 2 + 1;
    let sr = sample_rate as f32;

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT_SIZE);
    let c2r = planner.plan_fft_inverse(FFT_SIZE);
    let win = hann(FFT_SIZE);

    // Forward STFT → frames × bins complex.
    let mut spectra: Vec<Vec<Complex<f32>>> = Vec::new();
    let mut frame_pos: Vec<usize> = Vec::new();
    {
        let mut in_buf = r2c.make_input_vec();
        let mut out_buf = r2c.make_output_vec();
        let mut pos = 0;
        while pos + FFT_SIZE <= n {
            for (i, s) in in_buf.iter_mut().enumerate() {
                *s = signal[pos + i] * win[i];
            }
            r2c.process(&mut in_buf, &mut out_buf).expect("stft forward");
            spectra.push(out_buf.clone());
            frame_pos.push(pos);
            pos += HOP;
        }
    }

    // Which bins fall in the frequency band.
    let bin_hz = |b: usize| b as f32 * sr / FFT_SIZE as f32;
    let sel_bins: Vec<usize> =
        (0..bins).filter(|&b| bin_hz(b) >= sel.f0 && bin_hz(b) <= sel.f1).collect();
    // Which frames fall in the time span (by frame centre).
    let frame_time = |p: usize| (p + FFT_SIZE / 2) as f32 / sr;
    let sel_frames: Vec<usize> = (0..spectra.len())
        .filter(|&f| {
            let t = frame_time(frame_pos[f]);
            t >= sel.t0 && t <= sel.t1
        })
        .collect();

    if !sel_frames.is_empty() && !sel_bins.is_empty() {
        match op {
            SpectralOp::Gain(g) => {
                for &f in &sel_frames {
                    for &b in &sel_bins {
                        spectra[f][b] *= g;
                    }
                }
            }
            SpectralOp::Heal => {
                let first = sel_frames[0];
                let last = *sel_frames.last().unwrap();
                let left = first.saturating_sub(1);
                let right = (last + 1).min(spectra.len() - 1);
                let span = (last - first + 2) as f32;
                for &b in &sel_bins {
                    let a = spectra[left][b];
                    let c = spectra[right][b];
                    for &f in &sel_frames {
                        let w = (f - first + 1) as f32 / span;
                        spectra[f][b] = a * (1.0 - w) + c * w;
                    }
                }
            }
        }
    }

    // Inverse STFT with WOLA normalisation.
    let mut out = vec![0.0f32; n];
    let mut norm = vec![0.0f32; n];
    {
        let mut spec_buf = c2r.make_input_vec();
        let mut time_buf = c2r.make_output_vec();
        let scale = 1.0 / FFT_SIZE as f32;
        for (fi, spectrum) in spectra.iter().enumerate() {
            spec_buf.copy_from_slice(spectrum);
            // c2r requires real DC and Nyquist; editing can perturb them.
            spec_buf[0].im = 0.0;
            let ny = spec_buf.len() - 1;
            spec_buf[ny].im = 0.0;
            c2r.process(&mut spec_buf, &mut time_buf).expect("stft inverse");
            let pos = frame_pos[fi];
            for i in 0..FFT_SIZE {
                out[pos + i] += time_buf[i] * scale * win[i];
                norm[pos + i] += win[i] * win[i];
            }
        }
    }
    for i in 0..n {
        if norm[i] > 1e-6 {
            out[i] /= norm[i];
        } else {
            out[i] = signal[i];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin()).collect()
    }

    /// A selection that covers nothing meaningful round-trips near-identically.
    #[test]
    fn identity_reconstruction() {
        let sr = 48_000;
        let sig = tone(1000.0, sr, 16_384);
        // Gain of 1.0 over an empty (zero-width) band → passthrough via WOLA.
        let sel = Selection { t0: 0.0, t1: 0.0, f0: 0.0, f1: 0.0 };
        let out = apply_spectral(&sig, sr, &sel, SpectralOp::Gain(1.0));
        let mid = 4096..12_288;
        let err: f32 = mid.clone().map(|i| (out[i] - sig[i]).abs()).sum::<f32>() / mid.len() as f32;
        assert!(err < 1e-3, "WOLA identity error too high: {err}");
    }

    /// Attenuating the band around a tone reduces its energy.
    #[test]
    fn attenuates_selected_band() {
        let sr = 48_000;
        let sig = tone(3000.0, sr, 16_384);
        let sel = Selection { t0: 0.0, t1: 0.34, f0: 2500.0, f1: 3500.0 };
        let out = apply_spectral(&sig, sr, &sel, SpectralOp::Gain(0.1));
        let energy = |s: &[f32]| s[4096..12_288].iter().map(|x| x * x).sum::<f32>();
        assert!(energy(&out) < energy(&sig) * 0.5, "band not attenuated");
    }
}
