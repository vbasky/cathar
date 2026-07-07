//! WPE de-reverberation — Weighted Prediction Error (Nakatani et al.).
//!
//! Late reverberation is modelled, per STFT frequency bin, as a linear
//! prediction of the current frame from `K` frames starting `delay` frames in
//! the past (the delay skips the direct sound + early reflections). The
//! prediction filter is estimated to minimise the *weighted* prediction error —
//! each frame weighted by the inverse of its own power — and subtracted. The
//! power estimate and filter are refined over a few iterations. Deterministic,
//! pure Rust, no weights/models.

use crate::util::hann_window;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

const FFT: usize = 1024;
const HOP: usize = 256;

/// De-reverberate `signal` with WPE. `taps` (`K`) and `delay` (prediction gap,
/// frames) control the amount removed; `iterations` refines the estimate.
pub fn wpe(
    signal: &[f32],
    sample_rate: u32,
    taps: usize,
    delay: usize,
    iterations: u32,
) -> Vec<f32> {
    let _ = sample_rate; // operates on STFT frames; kept for API symmetry
    let n = signal.len();
    let k = taps.max(1);
    let delay = delay.max(1);
    if n < FFT * 2 {
        return signal.to_vec();
    }
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
            r2c.process(&mut in_buf, &mut out_buf).expect("wpe forward");
            spectra.push(out_buf.clone());
            positions.push(pos);
            pos += HOP;
        }
    }
    let frames = spectra.len();
    if frames <= delay + k + 2 {
        return signal.to_vec();
    }

    // Per-bin WPE.
    for f in 0..bins {
        // Column of complex STFT values at this bin.
        let x: Vec<Complex<f64>> =
            spectra.iter().map(|fr| Complex::new(fr[f].re as f64, fr[f].im as f64)).collect();
        let mut d = x.clone(); // desired (dereverberated) estimate

        // Floor the per-frame power *relative* to this bin's mean power, so
        // near-silent frames don't get near-infinite 1/λ weight (which would
        // otherwise dominate the normal equations and collapse the filter).
        let mean_pow = x.iter().map(|c| c.re * c.re + c.im * c.im).sum::<f64>() / frames as f64;
        let floor = (mean_pow * 1e-3).max(1e-12);

        for _ in 0..iterations.max(1) {
            // Power estimate λ[t] from the current desired signal.
            let lambda: Vec<f64> =
                d.iter().map(|c| (c.re * c.re + c.im * c.im).max(floor)).collect();

            // Weighted normal equations: R g = r, with regressor
            // x̄[t] = [x[t-delay], …, x[t-delay-K+1]].
            let mut r_mat = vec![vec![Complex::<f64>::new(0.0, 0.0); k]; k];
            let mut r_vec = vec![Complex::<f64>::new(0.0, 0.0); k];
            for t in (delay + k)..frames {
                let w = 1.0 / lambda[t];
                let xbar: Vec<Complex<f64>> = (0..k).map(|kk| x[t - delay - kk]).collect();
                for a in 0..k {
                    for b in 0..k {
                        r_mat[a][b] += xbar[a] * xbar[b].conj() * w;
                    }
                    r_vec[a] += xbar[a] * x[t].conj() * w;
                }
            }
            // Diagonal load for a well-conditioned solve.
            let trace: f64 = (0..k).map(|i| r_mat[i][i].re).sum();
            let load = 1e-6 * trace.max(1e-12) / k as f64;
            for (i, row) in r_mat.iter_mut().enumerate() {
                row[i] += Complex::new(load, 0.0);
            }
            let g = solve_hermitian(&mut r_mat, &r_vec);

            // Subtract the predicted late reverb: d[t] = x[t] − gᴴ x̄[t].
            for t in 0..frames {
                if t < delay + k {
                    d[t] = x[t];
                    continue;
                }
                let mut pred = Complex::<f64>::new(0.0, 0.0);
                for kk in 0..k {
                    pred += g[kk].conj() * x[t - delay - kk];
                }
                d[t] = x[t] - pred;
            }
        }

        for (t, dt) in d.iter().enumerate() {
            spectra[t][f] = Complex::new(dt.re as f32, dt.im as f32);
        }
    }

    // Inverse STFT (WOLA).
    let mut out = vec![0.0f32; n];
    let mut norm = vec![0.0f32; n];
    {
        let mut spec_buf = c2r.make_input_vec();
        let mut time_buf = c2r.make_output_vec();
        let scale = 1.0 / FFT as f32;
        for (fi, spectrum) in spectra.iter().enumerate() {
            spec_buf.copy_from_slice(spectrum);
            spec_buf[0].im = 0.0;
            spec_buf[bins - 1].im = 0.0;
            c2r.process(&mut spec_buf, &mut time_buf).expect("wpe inverse");
            let pos = positions[fi];
            for i in 0..FFT {
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

/// Solve the Hermitian positive-definite system `A x = b` by complex Cholesky.
// Textbook index-based linear algebra (row/column access), keep the range loops.
#[allow(clippy::needless_range_loop)]
fn solve_hermitian(a: &mut [Vec<Complex<f64>>], b: &[Complex<f64>]) -> Vec<Complex<f64>> {
    let k = b.len();
    // A = L Lᴴ (lower L; diagonal real).
    for i in 0..k {
        for j in 0..=i {
            let mut sum = a[i][j];
            for m in 0..j {
                sum -= a[i][m] * a[j][m].conj();
            }
            if i == j {
                let d = sum.re.max(1e-12).sqrt();
                a[i][i] = Complex::new(d, 0.0);
            } else {
                a[i][j] = sum / a[j][j];
            }
        }
    }
    // Forward L y = b.
    let mut y = vec![Complex::<f64>::new(0.0, 0.0); k];
    for i in 0..k {
        let mut s = b[i];
        for m in 0..i {
            s -= a[i][m] * y[m];
        }
        y[i] = s / a[i][i];
    }
    // Back Lᴴ x = y.
    let mut x = vec![Complex::<f64>::new(0.0, 0.0); k];
    for i in (0..k).rev() {
        let mut s = y[i];
        for m in (i + 1)..k {
            s -= a[m][i].conj() * x[m];
        }
        x[i] = s / a[i][i].conj();
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_reverb_tail() {
        let sr = 16_000u32;
        let n = sr as usize * 3;
        // Dry: short noise bursts separated by silence.
        let mut rng = 0x2545_F491_4F6C_DD1Du64;
        let mut noise = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as f32 / u64::MAX as f32) - 0.5
        };
        let mut dry = vec![0.0f32; n];
        let period = sr as usize / 2; // burst every 0.5 s
        for (i, d) in dry.iter_mut().enumerate() {
            if i % period < sr as usize / 20 {
                *d = noise();
            }
        }
        // Reverberate: exponentially-decaying multi-tap reflections.
        let mut wet = dry.clone();
        let taps = [(0.03, 0.6f32), (0.06, 0.4), (0.10, 0.28), (0.16, 0.18), (0.24, 0.1)];
        for &(dt, g) in &taps {
            let shift = (dt * sr as f32) as usize;
            for i in shift..n {
                wet[i] += g * dry[i - shift];
            }
        }

        let out = wpe(&wet, sr, 12, 2, 3);

        // Measure energy in the "gap" regions (should be reverb tail only).
        let gap_energy = |x: &[f32]| -> f32 {
            let mut e = 0.0;
            let mut c = 0usize;
            for (i, &v) in x.iter().enumerate() {
                let ph = i % period;
                if ph > sr as usize / 5 {
                    // well after each burst
                    e += v * v;
                    c += 1;
                }
            }
            e / c.max(1) as f32
        };
        let before = gap_energy(&wet);
        let after = gap_energy(&out);
        assert!(after < before * 0.7, "reverb tail not reduced: {before} -> {after}");
    }
}
