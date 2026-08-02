//! Audio de-clipping — rebuild samples flattened by hard clipping.
//!
//! Methods track the preferred families in the Rajmic et al. 2020 survey and
//! related literature (Kitić/Bertin/Gribonval SPADE, Siedenburg social
//! sparsity / PEW, Adler constrained OMP, Bilen NMF). All paths are pure Rust
//! and deterministic. The default is A-SPADE.

// Dense index-form loops (STFT frames × bins, NMF W/H, PEW neighbourhoods)
// are the readable shape for these algorithms; iterators obscure the math.
#![allow(clippy::needless_range_loop)]

use crate::util::hann_window;
use realfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Reconstruction strategy for clipped (flat-topped) samples.
///
/// Detection is always "samples at/above `threshold`". Methods differ in how
/// the missing peaks are rebuilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeclipMethod {
    /// **A-SPADE** sparse reconstruction over a Gabor tight frame (default).
    /// Preferred choice in the Rajmic et al. survey under most conditions.
    #[default]
    Spade,
    /// Cubic-Hermite fill across each clipped run (shoulders ±4 samples).
    /// Legacy / fast path; does not restore true peak height on heavy clips.
    Cubic,
    /// **Social sparsity** via Persistent Empirical Wiener (PEW) shrink on a
    /// time-frequency neighbourhood (Siedenburg / Kowalski), with iterative
    /// consistency projection onto the clipping set Γ.
    Social,
    /// **Constrained Orthogonal Matching Pursuit** on a DFT dictionary per
    /// frame (Adler et al.): greedy sparse recovery on reliable samples, then
    /// clipping-consistent synthesis + overlap-add.
    Omp,
    /// **Non-negative matrix factorization** of the STFT magnitude (Bilen et
    /// al. lineage): low-rank spectrogram model, phase retained from the
    /// observation, iterated with consistency projection.
    Nmf,
    /// **Deep-unfolded soft-threshold ISTA** (LISTA-style multi-layer STFT
    /// residual): several layers of complex soft-threshold + consistency
    /// projection. Inspectable, weight-free neural architecture — not a
    /// supervised DeclipNet/WaveNet (those need trained checkpoints).
    Neural,
}

/// Reconstruct clipped samples (default: A-SPADE). See [`declip_with_method`].
pub fn declip(signal: &[f32], threshold: f32) -> Vec<f32> {
    declip_with_method(signal, threshold, DeclipMethod::default())
}

/// Reconstruct clipped samples with an explicit [`DeclipMethod`].
///
/// Signals with no samples at/above `threshold` pass through unchanged. Short
/// signals (shorter than the method's frame size) pass through unchanged.
pub fn declip_with_method(signal: &[f32], threshold: f32, method: DeclipMethod) -> Vec<f32> {
    if !signal.iter().any(|&v| v.abs() >= threshold) {
        return signal.to_vec();
    }
    match method {
        DeclipMethod::Spade => declip_spade(signal, threshold),
        DeclipMethod::Cubic => declip_cubic(signal, threshold),
        DeclipMethod::Social => declip_social(signal, threshold),
        DeclipMethod::Omp => declip_omp(signal, threshold),
        DeclipMethod::Nmf => declip_nmf(signal, threshold),
        DeclipMethod::Neural => declip_neural(signal, threshold),
    }
}

// ── Shared helpers ───────────────────────────────────────────────────────────

/// Project one sample onto the clipping consistency set Γ.
#[inline]
fn project_gamma(cand: f32, obs: f32, threshold: f32) -> f32 {
    if obs.abs() < threshold {
        obs
    } else if obs >= threshold {
        cand.max(threshold)
    } else {
        cand.min(-threshold)
    }
}

fn project_signal(x: &mut [f32], obs: &[f32], threshold: f32) {
    for (xi, &o) in x.iter_mut().zip(obs) {
        *xi = project_gamma(*xi, o, threshold);
    }
}

fn cubic_fill(signal: &mut [f32], start: usize, end: usize) {
    if end - start < 4 {
        return;
    }
    let y0 = signal[start];
    let y1 = signal[end];
    let len = (end - start) as f32;
    for (i, s) in signal.iter_mut().enumerate().skip(start + 1).take(end - start - 1) {
        let t = (i - start) as f32 / len;
        let t2 = t * t;
        let t3 = t2 * t;
        *s = y0 * (1.0 - 3.0 * t2 + 2.0 * t3) + y1 * (3.0 * t2 - 2.0 * t3);
    }
}

/// Frame starts for a 4×-overlap Gabor layout, last frame flush to the end.
fn frame_starts(n: usize, l: usize, hop: usize) -> Vec<usize> {
    if n < l {
        return Vec::new();
    }
    let mut starts: Vec<usize> = (0..=n - l).step_by(hop).collect();
    if *starts.last().unwrap() != n - l {
        starts.push(n - l);
    }
    starts
}

fn cola_divisor(n: usize, starts: &[usize], win: &[f32]) -> Vec<f32> {
    let l = win.len();
    let mut cola = vec![0.0f32; n];
    for &s in starts {
        for j in 0..l {
            cola[s + j] += win[j] * win[j];
        }
    }
    for c in cola.iter_mut() {
        *c = c.max(1e-3);
    }
    cola
}

/// Keep the `k` largest-magnitude bins of `c` in place, zeroing the rest.
fn hard_threshold_k(c: &mut [Complex<f32>], k: usize) {
    if k >= c.len() {
        return;
    }
    let mags: Vec<f32> = c.iter().map(|v| v.norm_sqr()).collect();
    let mut sorted = mags.clone();
    sorted.sort_unstable_by(|a, b| b.partial_cmp(a).unwrap());
    let cutoff = sorted[k.saturating_sub(1)];
    for (cj, &m) in c.iter_mut().zip(mags.iter()) {
        if m < cutoff {
            *cj = Complex::new(0.0, 0.0);
        }
    }
}

/// Complex soft-threshold: shrink magnitude by `lambda`, keep phase.
fn soft_threshold(c: &mut [Complex<f32>], lambda: f32) {
    for v in c.iter_mut() {
        let mag = v.norm();
        if mag <= lambda {
            *v = Complex::new(0.0, 0.0);
        } else {
            *v *= (mag - lambda) / mag;
        }
    }
}

// ── Cubic ────────────────────────────────────────────────────────────────────

fn declip_cubic(signal: &[f32], threshold: f32) -> Vec<f32> {
    let n = signal.len();
    let mut output = signal.to_vec();
    let mut i = 0;
    while i < n {
        if signal[i].abs() >= threshold {
            let start = i;
            while i < n && signal[i].abs() >= threshold {
                i += 1;
            }
            let end = i.min(n - 1);
            let clip_start = start.saturating_sub(4);
            let clip_end = (end + 4).min(n - 1);
            if clip_end > clip_start + 4 {
                cubic_fill(&mut output, clip_start, clip_end);
            }
        }
        i += 1;
    }
    output
}

// ── A-SPADE ──────────────────────────────────────────────────────────────────

fn declip_spade(signal: &[f32], threshold: f32) -> Vec<f32> {
    const L: usize = 1024;
    const HOP: usize = 256;
    let n = signal.len();
    if n < L {
        return signal.to_vec();
    }
    const RELAX_BY: usize = 2;
    const MAX_ITER: usize = 100;

    let win = hann_window(L);
    let scale = 1.0 / (L as f32).sqrt();
    let starts = frame_starts(n, L, HOP);
    let nf = starts.len();
    let cola = cola_divisor(n, &starts, &win);

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(L);
    let ifft = planner.plan_fft_inverse(L);

    let analyze = |x: &[f32]| -> Vec<Vec<Complex<f32>>> {
        starts
            .iter()
            .map(|&s| {
                let mut buf: Vec<Complex<f32>> =
                    (0..L).map(|j| Complex::new(x[s + j] * win[j] * scale, 0.0)).collect();
                fft.process(&mut buf);
                buf
            })
            .collect()
    };
    let synth = |z: &[Vec<Complex<f32>>]| -> Vec<f32> {
        let mut y = vec![0.0f32; n];
        for (m, &s) in starts.iter().enumerate() {
            let mut buf = z[m].clone();
            ifft.process(&mut buf);
            for j in 0..L {
                y[s + j] += win[j] * scale * buf[j].re;
            }
        }
        y
    };

    let energy: f32 = signal.iter().map(|v| v * v).sum::<f32>().sqrt();
    let eps = 1e-3 * energy.max(1e-9);

    let mut x = signal.to_vec();
    let mut u = vec![vec![Complex::new(0.0, 0.0); L]; nf];
    let mut k = 1usize;

    for _ in 0..MAX_ITER {
        let ax = analyze(&x);
        let mut z = ax;
        for (zm, um) in z.iter_mut().zip(&u) {
            for (zv, uv) in zm.iter_mut().zip(um) {
                *zv += *uv;
            }
            hard_threshold_k(zm, k);
        }
        let zmu: Vec<Vec<Complex<f32>>> = z
            .iter()
            .zip(&u)
            .map(|(zm, um)| zm.iter().zip(um).map(|(zv, uv)| zv - uv).collect())
            .collect();
        let ahw = synth(&zmu);
        for (i, xi) in x.iter_mut().enumerate() {
            *xi = project_gamma(ahw[i] / cola[i], signal[i], threshold);
        }
        let ax2 = analyze(&x);
        let mut resid = 0.0f32;
        for ((zm, um), axm) in z.iter().zip(u.iter_mut()).zip(&ax2) {
            for ((zv, uv), av) in zm.iter().zip(um.iter_mut()).zip(axm) {
                let d = av - zv;
                resid += d.norm_sqr();
                *uv += d;
            }
        }
        if resid.sqrt() <= eps {
            break;
        }
        k += RELAX_BY;
        if k >= L {
            break;
        }
    }
    x
}

// ── Social sparsity (PEW) ────────────────────────────────────────────────────
//
// Analysis-side PEW (Persistent Empirical Wiener) on a time-spread tonal
// neighbourhood Γ (1×5 in frequency×time by default), with iterative
// consistency projection. See Siedenburg et al. / Gaultier et al. Algorithm 1
// (social analysis variant).

fn declip_social(signal: &[f32], threshold: f32) -> Vec<f32> {
    const L: usize = 1024;
    const HOP: usize = 256;
    // Neighbourhood half-widths: 0 freq ± 2 time frames (tonal continuity).
    const DF: usize = 0;
    const DT: usize = 2;
    const MAX_ITER: usize = 40;
    let n = signal.len();
    if n < L {
        return signal.to_vec();
    }

    let win = hann_window(L);
    let scale = 1.0 / (L as f32).sqrt();
    let starts = frame_starts(n, L, HOP);
    let cola = cola_divisor(n, &starts, &win);

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(L);
    let ifft = planner.plan_fft_inverse(L);

    let analyze = |x: &[f32]| -> Vec<Vec<Complex<f32>>> {
        starts
            .iter()
            .map(|&s| {
                let mut buf: Vec<Complex<f32>> =
                    (0..L).map(|j| Complex::new(x[s + j] * win[j] * scale, 0.0)).collect();
                fft.process(&mut buf);
                buf
            })
            .collect()
    };
    let synth = |z: &[Vec<Complex<f32>>]| -> Vec<f32> {
        let mut y = vec![0.0f32; n];
        for (m, &s) in starts.iter().enumerate() {
            let mut buf = z[m].clone();
            ifft.process(&mut buf);
            for j in 0..L {
                y[s + j] += win[j] * scale * buf[j].re;
            }
        }
        y
    };

    let mut x = signal.to_vec();
    // Initial μ from median frame energy (strong shrink → relax).
    let z0 = analyze(&x);
    let mut energies: Vec<f32> = z0.iter().flat_map(|f| f.iter().map(|c| c.norm_sqr())).collect();
    energies.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
    let med = energies[energies.len() / 2].sqrt().max(1e-6);
    let mut mu = med * 0.5;
    const ALPHA: f32 = 0.92; // geometric relaxation of μ

    for _ in 0..MAX_ITER {
        let mut z = analyze(&x);
        pew_shrink(&mut z, mu, DF, DT);
        let recon = synth(&z);
        for (i, xi) in x.iter_mut().enumerate() {
            *xi = project_gamma(recon[i] / cola[i], signal[i], threshold);
        }
        mu *= ALPHA;
        if mu < 1e-8 {
            break;
        }
    }
    x
}

/// PEW shrink: `Z_ft *= max(0, 1 − μ² / ‖Z_Pft‖_F²)`.
fn pew_shrink(z: &mut [Vec<Complex<f32>>], mu: f32, df: usize, dt: usize) {
    let nf = z.len();
    if nf == 0 {
        return;
    }
    let n_bins = z[0].len();
    let mu2 = mu * mu;
    // Snapshot magnitudes for neighbourhood energy (read while writing shrink).
    let mags: Vec<Vec<f32>> = z.iter().map(|f| f.iter().map(|c| c.norm_sqr()).collect()).collect();

    for t in 0..nf {
        for f in 0..n_bins {
            let mut e = 0.0f32;
            let t0 = t.saturating_sub(dt);
            let t1 = (t + dt + 1).min(nf);
            let f0 = f.saturating_sub(df);
            let f1 = (f + df + 1).min(n_bins);
            for tt in t0..t1 {
                for ff in f0..f1 {
                    e += mags[tt][ff];
                }
            }
            let factor = (1.0 - mu2 / e.max(1e-20)).max(0.0);
            z[t][f] *= factor;
        }
    }
}

// ── Constrained OMP ──────────────────────────────────────────────────────────
//
// Per-frame Matching Pursuit on a DFT dictionary using only reliable samples
// as the residual support (Adler-style constrained pursuit), then consistency
// projection and overlap-add.

fn declip_omp(signal: &[f32], threshold: f32) -> Vec<f32> {
    const L: usize = 512;
    const HOP: usize = 128;
    const MAX_ATOMS: usize = 64;
    let n = signal.len();
    if n < L {
        return signal.to_vec();
    }

    let win = hann_window(L);
    let starts = frame_starts(n, L, HOP);
    let cola = cola_divisor(n, &starts, &win);

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(L);
    let ifft = planner.plan_fft_inverse(L);
    let inv_l = 1.0 / L as f32;

    let mut acc = vec![0.0f32; n];

    for &s in &starts {
        // Unwindowed samples for reliability / consistency; window for pursuit.
        let samples: Vec<f32> = (0..L).map(|j| signal[s + j]).collect();
        let reliable: Vec<bool> = samples.iter().map(|&v| v.abs() < threshold).collect();
        let n_rel = reliable.iter().filter(|&&r| r).count();
        let has_clip = n_rel < L;

        let recon = if !has_clip || n_rel < 8 {
            samples.clone()
        } else {
            // Windowed residual on reliable support; OMP in the DFT dictionary.
            let mut residual: Vec<f32> =
                (0..L).map(|j| if reliable[j] { samples[j] * win[j] } else { 0.0 }).collect();
            let mut coeffs = vec![Complex::new(0.0, 0.0); L];
            let mut used = vec![false; L];
            let kmax = MAX_ATOMS.min(n_rel.saturating_sub(1).max(1));

            for _ in 0..kmax {
                let mut buf: Vec<Complex<f32>> =
                    residual.iter().map(|&v| Complex::new(v, 0.0)).collect();
                fft.process(&mut buf);

                let mut best_j = 0usize;
                let mut best_m = -1.0f32;
                for (j, c) in buf.iter().enumerate() {
                    if used[j] {
                        continue;
                    }
                    let m = c.norm_sqr();
                    if m > best_m {
                        best_m = m;
                        best_j = j;
                    }
                }
                if best_m < 1e-20 {
                    break;
                }
                used[best_j] = true;
                // Accumulate this Fourier mode; rustfft: IFFT(FFT(x)) = L·x.
                coeffs[best_j] += buf[best_j];

                let mut atom = vec![Complex::new(0.0, 0.0); L];
                atom[best_j] = buf[best_j];
                ifft.process(&mut atom);
                for j in 0..L {
                    if reliable[j] {
                        residual[j] -= atom[j].re * inv_l;
                    }
                }
                let e: f32 = residual
                    .iter()
                    .zip(&reliable)
                    .map(|(&v, &r)| if r { v * v } else { 0.0 })
                    .sum();
                if e < 1e-12 {
                    break;
                }
            }

            let mut buf = coeffs;
            ifft.process(&mut buf);
            // Unwindow approximate recon (divide by win where safe).
            (0..L)
                .map(|j| {
                    let y = buf[j].re * inv_l;
                    if win[j] > 1e-3 { y / win[j] } else { samples[j] }
                })
                .collect()
        };

        for j in 0..L {
            let y = project_gamma(recon[j], samples[j], threshold) * win[j];
            acc[s + j] += y;
        }
    }

    for (i, a) in acc.iter_mut().enumerate() {
        *a /= cola[i];
    }
    project_signal(&mut acc, signal, threshold);
    acc
}

// ── NMF spectrogram ──────────────────────────────────────────────────────────
//
// Multiplicative-update NMF on STFT magnitudes, phase from the current estimate,
// consistency projection, a few outer iterations.

fn declip_nmf(signal: &[f32], threshold: f32) -> Vec<f32> {
    const L: usize = 1024;
    const HOP: usize = 256;
    const RANK: usize = 16;
    const NMF_ITERS: usize = 40;
    const OUTER: usize = 4;
    let n = signal.len();
    if n < L {
        return signal.to_vec();
    }

    let win = hann_window(L);
    let scale = 1.0 / (L as f32).sqrt();
    let starts = frame_starts(n, L, HOP);
    let nf = starts.len();
    let cola = cola_divisor(n, &starts, &win);
    let n_bins = L / 2 + 1;

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(L);
    let ifft = planner.plan_fft_inverse(L);

    let mut x = signal.to_vec();

    for _outer in 0..OUTER {
        // Complex STFT of current estimate.
        let mut stft: Vec<Vec<Complex<f32>>> = starts
            .iter()
            .map(|&s| {
                let mut buf: Vec<Complex<f32>> =
                    (0..L).map(|j| Complex::new(x[s + j] * win[j] * scale, 0.0)).collect();
                fft.process(&mut buf);
                buf
            })
            .collect();

        // Magnitude matrix V[bin][frame] (one-sided).
        let mut v = vec![vec![0.0f32; nf]; n_bins];
        for (t, frame) in stft.iter().enumerate() {
            for b in 0..n_bins {
                v[b][t] = frame[b].norm().max(1e-12);
            }
        }

        // Deterministic NMF init from per-bin energy and a fixed rank basis.
        let mut w = vec![vec![0.0f32; RANK]; n_bins];
        let mut h = vec![vec![0.0f32; nf]; RANK];
        for b in 0..n_bins {
            let row_mean: f32 = v[b].iter().sum::<f32>() / nf as f32;
            for r in 0..RANK {
                // Smooth spectral templates (overlapping raised-cosine bands).
                let centre = (r as f32 + 0.5) / RANK as f32 * (n_bins as f32);
                let dist = (b as f32 - centre).abs() / (n_bins as f32 / RANK as f32);
                w[b][r] = (1.0 - dist).max(0.05) * row_mean.sqrt().max(1e-6);
            }
        }
        for r in 0..RANK {
            for t in 0..nf {
                let mut s = 0.0f32;
                for b in 0..n_bins {
                    s += v[b][t] * w[b][r];
                }
                h[r][t] = (s / n_bins as f32).max(1e-6);
            }
        }

        // Multiplicative updates (Euclidean NMF).
        for _ in 0..NMF_ITERS {
            // H ← H ⊙ (Wᵀ V) / (Wᵀ W H)
            let mut wt_v = vec![vec![0.0f32; nf]; RANK];
            let mut wt_w_h = vec![vec![0.0f32; nf]; RANK];
            for r in 0..RANK {
                for t in 0..nf {
                    let mut num = 0.0f32;
                    for b in 0..n_bins {
                        num += w[b][r] * v[b][t];
                    }
                    wt_v[r][t] = num;
                    let mut den = 0.0f32;
                    for b in 0..n_bins {
                        let mut wh = 0.0f32;
                        for rr in 0..RANK {
                            wh += w[b][rr] * h[rr][t];
                        }
                        den += w[b][r] * wh;
                    }
                    wt_w_h[r][t] = den.max(1e-12);
                }
            }
            for r in 0..RANK {
                for t in 0..nf {
                    h[r][t] *= wt_v[r][t] / wt_w_h[r][t];
                }
            }
            // W ← W ⊙ (V Hᵀ) / (W H Hᵀ)
            let mut v_ht = vec![vec![0.0f32; RANK]; n_bins];
            let mut w_h_ht = vec![vec![0.0f32; RANK]; n_bins];
            for b in 0..n_bins {
                for r in 0..RANK {
                    let mut num = 0.0f32;
                    for t in 0..nf {
                        num += v[b][t] * h[r][t];
                    }
                    v_ht[b][r] = num;
                    let mut den = 0.0f32;
                    for t in 0..nf {
                        let mut wh = 0.0f32;
                        for rr in 0..RANK {
                            wh += w[b][rr] * h[rr][t];
                        }
                        den += wh * h[r][t];
                    }
                    w_h_ht[b][r] = den.max(1e-12);
                }
            }
            for b in 0..n_bins {
                for r in 0..RANK {
                    w[b][r] *= v_ht[b][r] / w_h_ht[b][r];
                }
            }
        }

        // Rebuild complex STFT: NMF magnitude × original phase (full spectrum).
        for (t, frame) in stft.iter_mut().enumerate() {
            for b in 0..n_bins {
                let mut mag = 0.0f32;
                for r in 0..RANK {
                    mag += w[b][r] * h[r][t];
                }
                mag = mag.max(1e-12);
                let phase = frame[b] / frame[b].norm().max(1e-12);
                frame[b] = phase * mag;
                // Hermitian mirror for real IFFT (bins 1..n_bins-2).
                if b > 0 && b < n_bins - 1 {
                    let mir = L - b;
                    frame[mir] = frame[b].conj();
                }
            }
            // DC and Nyquist stay real-ish; leave as set.
        }

        let mut recon = vec![0.0f32; n];
        for (t, &s) in starts.iter().enumerate() {
            let mut buf = stft[t].clone();
            ifft.process(&mut buf);
            for j in 0..L {
                recon[s + j] += win[j] * scale * buf[j].re;
            }
        }
        for (i, xi) in x.iter_mut().enumerate() {
            *xi = project_gamma(recon[i] / cola[i], signal[i], threshold);
        }
    }
    x
}

// ── Deep-unfolded soft-threshold ISTA (LISTA-style "neural") ─────────────────
//
// Multi-layer STFT residual: each layer soft-thresholds the spectrum with a
// decreasing λ schedule, synthesises, and projects onto Γ. Weight-free,
// inspectable deep-unfolded architecture (the LISTA / ISTA-net family without
// trained parameters). Distinct from SPADE (hard k) and social (PEW).

fn declip_neural(signal: &[f32], threshold: f32) -> Vec<f32> {
    const L: usize = 1024;
    const HOP: usize = 256;
    const LAYERS: usize = 12;
    let n = signal.len();
    if n < L {
        return signal.to_vec();
    }

    let win = hann_window(L);
    let scale = 1.0 / (L as f32).sqrt();
    let starts = frame_starts(n, L, HOP);
    let cola = cola_divisor(n, &starts, &win);

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(L);
    let ifft = planner.plan_fft_inverse(L);

    let mut x = signal.to_vec();

    // λ schedule: start at a fraction of median spectral magnitude, decay.
    let mut probe: Vec<Complex<f32>> =
        (0..L).map(|j| Complex::new(x[starts[0] + j] * win[j] * scale, 0.0)).collect();
    fft.process(&mut probe);
    let mut mags: Vec<f32> = probe.iter().map(|c| c.norm()).collect();
    mags.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
    let mut lambda = mags[mags.len() / 2] * 0.35;
    const DECAY: f32 = 0.75;

    for _ in 0..LAYERS {
        // Analyze
        let z: Vec<Vec<Complex<f32>>> = starts
            .iter()
            .map(|&s| {
                let mut buf: Vec<Complex<f32>> =
                    (0..L).map(|j| Complex::new(x[s + j] * win[j] * scale, 0.0)).collect();
                fft.process(&mut buf);
                soft_threshold(&mut buf, lambda);
                buf
            })
            .collect();

        // Synthesise
        let mut recon = vec![0.0f32; n];
        for (m, &s) in starts.iter().enumerate() {
            let mut buf = z[m].clone();
            ifft.process(&mut buf);
            for j in 0..L {
                recon[s + j] += win[j] * scale * buf[j].re;
            }
        }
        for (i, xi) in x.iter_mut().enumerate() {
            *xi = project_gamma(recon[i] / cola[i], signal[i], threshold);
        }
        lambda *= DECAY;
    }
    x
}
