//! Repair & reduction: de-hum, de-wind, de-click, de-clip, de-reverb,
//! spectral repair, de-plosive, de-rustle.

use crate::util::hann_window;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Remove mains hum (50/60 Hz + harmonics) using cascaded notch filters.
pub fn dehum(signal: &[f32], sample_rate: u32, base_freq: f32, num_harmonics: usize) -> Vec<f32> {
    let mut output = signal.to_vec();
    for h in 1..=num_harmonics {
        let freq = base_freq * h as f32;
        if freq >= sample_rate as f32 * 0.45 {
            break;
        }
        notch_filter(&mut output, freq, sample_rate, 30.0);
    }
    output
}

/// Apply a second-order IIR notch filter in-place.
fn notch_filter(signal: &mut [f32], freq: f32, sample_rate: u32, q: f32) {
    let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
    let alpha = w0.sin() / (2.0 * q);
    let b0 = 1.0;
    let b1 = -2.0 * w0.cos();
    let b2 = 1.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;
    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for s in signal.iter_mut() {
        let x0 = *s;
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
        *s = y0;
    }
}

// ── De-wind ──────────────────────────────────────────────────────────────────

/// Apply a second-order IIR high-pass (RBJ cookbook) in-place at the given `q`.
fn highpass_biquad(signal: &mut [f32], freq: f32, sample_rate: u32, q: f32) {
    let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
    let cos = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = ((1.0 + cos) / 2.0) / a0;
    let b1 = (-(1.0 + cos)) / a0;
    let b2 = ((1.0 + cos) / 2.0) / a0;
    let a1 = (-2.0 * cos) / a0;
    let a2 = (1.0 - alpha) / a0;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for s in signal.iter_mut() {
        let x0 = *s;
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
        *s = y0;
    }
}

/// Remove low-frequency wind rumble with a 4th-order Butterworth high-pass
/// (two cascaded biquads, ~24 dB/octave). `cutoff_hz` is the corner frequency
/// (≈ 80 Hz suits most handheld/outdoor wind); content above it is untouched.
pub fn dewind(signal: &[f32], sample_rate: u32, cutoff_hz: f32) -> Vec<f32> {
    let mut out = signal.to_vec();
    // Butterworth 4th-order section Qs.
    for q in [0.541_196_1, 1.306_563] {
        highpass_biquad(&mut out, cutoff_hz, sample_rate, q);
    }
    out
}

// ── De-click ─────────────────────────────────────────────────────────────────

/// Detect and interpolate impulse clicks.
///
/// Threshold is the number of local-RMS multiples above which a sample is a click.
/// Typical threshold: 8.0–15.0.
pub fn declick(signal: &[f32], threshold: f32, window: usize) -> Vec<f32> {
    let n = signal.len();
    let half = window / 2;
    let mut output = signal.to_vec();
    // A signal shorter than the analysis window has no interior to scan; bail
    // out before `n - half` (computed below) can underflow `usize`.
    if half == 0 || n <= window {
        return output;
    }
    let rms = local_rms(signal, window);

    let mut i = half;
    while i + half < n {
        if signal[i].abs() > threshold * rms[i] {
            let start = i.saturating_sub(half);
            let end = (i + half).min(n - 1);
            if end > start + 2 {
                cubic_interpolate(&mut output, start, end);
            }
            i += half;
        }
        i += 1;
    }
    output
}

fn local_rms(signal: &[f32], window: usize) -> Vec<f32> {
    let n = signal.len();
    let half = window / 2;
    let mut rms = vec![0.0f32; n];
    let mut sum_sq = 0.0f32;
    let mut count = 0usize;
    for s in signal.iter().take(half.min(n)) {
        sum_sq += s * s;
        count += 1;
    }
    for i in 0..n {
        if i >= half {
            let out = i - half;
            sum_sq -= signal[out] * signal[out];
            count -= 1;
        }
        if i + half < n {
            sum_sq += signal[i + half] * signal[i + half];
            count += 1;
        }
        rms[i] = (sum_sq / count as f32).sqrt().max(1e-10);
    }
    rms
}

fn cubic_interpolate(signal: &mut [f32], start: usize, end: usize) {
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

// ── De-clip (A-SPADE sparse declipping) ──────────────────────────────────────
//
// A-SPADE over a Hann-windowed, 4x-overlapping Gabor tight frame. The earlier
// per-block *rectangular* DFT diverged because spectral leakage left real audio
// non-sparse (no small-k consistent solution); the windowed overlapping frame
// fixes that, and the iteration now converges monotonically — a clipped tone
// rebuilds to within ~0.02 RMS of the original with the peak restored.

/// Keep the `k` largest-magnitude bins of `c` in place, zeroing the rest
/// (the hard-thresholding / sparse-approximation step of A-SPADE).
fn hard_threshold_k(c: &mut [Complex<f32>], k: usize) {
    if k >= c.len() {
        return;
    }
    let mut mags: Vec<f32> = c.iter().map(|v| v.norm_sqr()).collect();
    let mut sorted = mags.clone();
    sorted.sort_unstable_by(|a, b| b.partial_cmp(a).unwrap());
    let cutoff = sorted[k.saturating_sub(1)];
    for (cj, &m) in c.iter_mut().zip(mags.iter()) {
        if m < cutoff {
            *cj = Complex::new(0.0, 0.0);
        }
    }
    mags.clear();
}

/// Reconstruct clipped samples with **A-SPADE** sparse declipping (Kitić, Bertin
/// & Gribonval 2015) over a Hann-windowed, 4×-overlapping **Gabor tight frame**.
///
/// Clipping is detected as samples at or beyond `threshold`. Rather than guessing
/// an interpolation curve, A-SPADE recovers the signal that is *sparsest in the
/// Gabor (windowed-DFT) domain* while keeping every reliable sample exact and
/// every clipped sample beyond the threshold with its original sign. The windowed
/// overlapping frame (`AᴴA = diag(COLA)`, a Parseval frame once normalised) is
/// what makes real audio sparse: a single rectangular-block DFT leaks energy
/// across bins, so no sparse consistent solution exists and the iteration
/// diverges. Signals shorter than one frame pass through unchanged.
pub fn declip(signal: &[f32], threshold: f32) -> Vec<f32> {
    const L: usize = 1024;
    const HOP: usize = 256;
    let n = signal.len();
    if n < L || !signal.iter().any(|&v| v.abs() >= threshold) {
        return signal.to_vec();
    }
    // A-SPADE k-relaxation: keep `RELAX_BY` more coefficients per frame each
    // iteration until the sparse estimate and the consistency set agree. Tuned so
    // a clipped tone converges in ~50 iterations with sub-0.02 RMS error.
    const RELAX_BY: usize = 2;
    const MAX_ITER: usize = 100;

    let win = hann_window(L);
    let scale = 1.0 / (L as f32).sqrt();

    // Frame starts (4× overlap), with a final frame flush to the end.
    let mut starts: Vec<usize> = (0..=n - L).step_by(HOP).collect();
    if *starts.last().unwrap() != n - L {
        starts.push(n - L);
    }
    let nf = starts.len();

    // COLA divisor: AᴴA = diag(cola). The interior is fully covered; floor the
    // thin edge coverage so synthesis there is attenuated rather than NaN.
    let mut cola = vec![0.0f32; n];
    for &s in &starts {
        for j in 0..L {
            cola[s + j] += win[j] * win[j];
        }
    }
    for c in cola.iter_mut() {
        *c = c.max(1e-3);
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(L);
    let ifft = planner.plan_fft_inverse(L);

    // A: windowed analysis of every frame → per-frame (scaled) spectra.
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
    // Aᴴ: window-weighted overlap-add of the inverse-transformed frames.
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
    let mut u = vec![vec![Complex::new(0.0, 0.0); L]; nf]; // per-frame dual
    let mut k = 1usize; // largest coefficients kept per frame

    for _ in 0..MAX_ITER {
        // z = H_k(A x + u) per frame
        let ax = analyze(&x);
        let mut z = ax;
        for (zm, um) in z.iter_mut().zip(&u) {
            for (zv, uv) in zm.iter_mut().zip(um) {
                *zv += *uv;
            }
            hard_threshold_k(zm, k);
        }
        // x = proj_Γ( diag(1/cola) · Aᴴ(z - u) )
        let zmu: Vec<Vec<Complex<f32>>> = z
            .iter()
            .zip(&u)
            .map(|(zm, um)| zm.iter().zip(um).map(|(zv, uv)| zv - uv).collect())
            .collect();
        let ahw = synth(&zmu);
        for (i, xi) in x.iter_mut().enumerate() {
            let cand = ahw[i] / cola[i];
            let obs = signal[i];
            *xi = if obs.abs() < threshold {
                obs
            } else if obs >= threshold {
                cand.max(threshold)
            } else {
                cand.min(-threshold)
            };
        }
        // dual update u += A x - z, with consistency residual ||A x - z||
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

/// Remove room reverb using spectral envelope decay gating.
///
/// For each frequency bin, tracks the short-term envelope, detects the decay
/// tail (reverb) vs the direct onset, and attenuates the tail.
pub fn dereverb(signal: &[f32], sample_rate: u32, strength: f32) -> Vec<f32> {
    let fft_size = 2048;
    let hop_size = 512;
    let n = signal.len();
    if n < fft_size {
        return signal.to_vec();
    }

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / (fft_size as f32);
    let n_bins = fft_size / 2 + 1;
    let frames = n / hop_size;

    let attack_coeff = (-2.0f32 / (sample_rate as f32 * 0.008)).exp(); // 8ms attack
    let release_coeff = (-2.0f32 / (sample_rate as f32 * 0.050)).exp(); // 50ms release

    let mut env = vec![0.0f32; n_bins];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    let mut reverb_floor = vec![f32::MAX; n_bins];
    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        for k in 0..n_bins {
            let mag = (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
            if mag > env[k] {
                env[k] = attack_coeff * env[k] + (1.0 - attack_coeff) * mag;
            } else {
                env[k] = release_coeff * env[k] + (1.0 - release_coeff) * mag;
            }
            reverb_floor[k] = reverb_floor[k].min(env[k]);
        }
    }

    for item in reverb_floor.iter_mut().take(n_bins) {
        *item *= 1.5;
    }

    let threshold_db = strength * 6.0;
    let threshold_linear = 10.0f32.powf(threshold_db / 20.0);
    env.fill(0.0);
    let mut output = vec![0.0f32; n + fft_size];
    let mut in_buf2 = r2c.make_input_vec();
    let mut out_buf2 = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf2[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf2, &mut out_buf2).unwrap();

        for k in 0..n_bins {
            let mag = (out_buf2[k].re * out_buf2[k].re + out_buf2[k].im * out_buf2[k].im).sqrt();
            if mag > env[k] {
                env[k] = attack_coeff * env[k] + (1.0 - attack_coeff) * mag;
            } else {
                env[k] = release_coeff * env[k] + (1.0 - release_coeff) * mag;
            }
            let ratio = env[k] / reverb_floor[k].max(1e-10);
            let gate_gain = if ratio < threshold_linear {
                (ratio / threshold_linear).powf(2.0).max(0.01)
            } else {
                1.0
            };
            out_buf2[k].re *= gate_gain;
            out_buf2[k].im *= gate_gain;
        }

        c2r.process(&mut out_buf2, &mut in_buf2).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf2[i] * hann[i] * scale;
        }
    }
    output.truncate(n);
    output
}

// ── Spectral repair ─────────────────────────────────────────────────────────

/// Paint out isolated transient spectral artifacts — brief whistles, bursts,
/// and glitches that appear in only a few STFT frames.
///
/// Each time-frequency bin is compared against the median of the *same bin* in
/// neighbouring frames. A bin whose magnitude spikes far above that temporal
/// median is a transient anomaly: it is pulled back to the median while its
/// phase is preserved. Sustained content (tones, formants, broadband texture)
/// matches its own temporal median and is left untouched, so unrepaired audio
/// passes through transparently (the overlap-add is window-normalised to unity).
///
/// `strength` (1–10) lowers the outlier threshold — higher removes more.
pub fn spectral_repair(signal: &[f32], strength: f32) -> Vec<f32> {
    let fft_size = 2048;
    let hop_size = 512;
    let n = signal.len();
    if n < fft_size {
        return signal.to_vec();
    }

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / fft_size as f32;
    let n_bins = fft_size / 2 + 1;

    // ── 1. Forward STFT — keep every frame's spectrum. ──
    let mut spectra = Vec::new();
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();
    let mut offset = 0;
    while offset + fft_size <= n {
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();
        spectra.push(out_buf.clone());
        offset += hop_size;
    }
    let frames = spectra.len();
    if frames == 0 {
        return signal.to_vec();
    }

    // Original magnitudes (detection uses these, so replacements don't cascade).
    let mags: Vec<Vec<f32>> = spectra
        .iter()
        .map(|fr| fr.iter().map(|c| (c.re * c.re + c.im * c.im).sqrt()).collect())
        .collect();

    // ── 2. Replace transient outliers with the temporal median per bin. ──
    let ratio = 1.0 + 8.0 / strength.max(0.1);
    const T: usize = 4; // temporal half-window, in frames
    for k in 0..n_bins {
        for t in 0..frames {
            let mag = mags[t][k];
            let lo = t.saturating_sub(T);
            let hi = (t + T).min(frames - 1);
            let mut nb: Vec<f32> = (lo..=hi).filter(|&s| s != t).map(|s| mags[s][k]).collect();
            if nb.is_empty() {
                continue;
            }
            nb.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let med = nb[nb.len() / 2];
            if mag > ratio * med.max(1e-9) {
                let g = med / mag;
                spectra[t][k].re *= g;
                spectra[t][k].im *= g;
            }
        }
    }

    // ── 3. Inverse STFT with unity-gain overlap-add (window-normalised). ──
    let mut output = vec![0.0f32; n + fft_size];
    let mut wsum = vec![0.0f32; n + fft_size];
    let mut spec_buf = c2r.make_input_vec();
    let mut time_buf = c2r.make_output_vec();
    for (t, frame) in spectra.iter().enumerate() {
        spec_buf.copy_from_slice(frame);
        c2r.process(&mut spec_buf, &mut time_buf).unwrap();
        let off = t * hop_size;
        for i in 0..fft_size {
            output[off + i] += time_buf[i] * hann[i] * scale;
            wsum[off + i] += hann[i] * hann[i];
        }
    }
    for i in 0..n {
        if wsum[i] > 1e-6 {
            output[i] /= wsum[i];
        }
    }
    output.truncate(n);
    output
}

// ── De-plosive / De-rustle (band-limited transient suppression) ──────────────

/// Suppress transient energy bursts confined to a frequency band. STFT, then per
/// frame measure the energy in `[lo_hz, hi_hz]`; a frame whose band energy spikes
/// far above its temporal median is a transient (a plosive pop, a rustle), and
/// its band bins are scaled down toward the median with phase preserved.
/// Sustained band content matches its own median and is left alone; the
/// overlap-add is window-normalised to unity gain.
fn attenuate_band_transients(
    signal: &[f32],
    sample_rate: u32,
    lo_hz: f32,
    hi_hz: f32,
    strength: f32,
) -> Vec<f32> {
    let fft_size = 2048;
    let hop_size = 512;
    let n = signal.len();
    if n < fft_size {
        return signal.to_vec();
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / fft_size as f32;
    let n_bins = fft_size / 2 + 1;

    let mut spectra = Vec::new();
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();
    let mut offset = 0;
    while offset + fft_size <= n {
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();
        spectra.push(out_buf.clone());
        offset += hop_size;
    }
    let frames = spectra.len();
    if frames == 0 {
        return signal.to_vec();
    }

    let bin =
        |hz: f32| ((hz * fft_size as f32 / sample_rate as f32).round() as usize).min(n_bins - 1);
    let (lo, hi) = (bin(lo_hz), bin(hi_hz).max(bin(lo_hz)));

    // Per-frame energy in the target band (from the original spectra).
    let band: Vec<f32> = spectra
        .iter()
        .map(|fr| fr[lo..=hi].iter().map(|c| c.re * c.re + c.im * c.im).sum::<f32>())
        .collect();

    let ratio = 1.0 + 8.0 / strength.max(0.1);
    const T: usize = 6;
    for t in 0..frames {
        let a = t.saturating_sub(T);
        let b = (t + T).min(frames - 1);
        let mut nb: Vec<f32> = (a..=b).filter(|&s| s != t).map(|s| band[s]).collect();
        if nb.is_empty() {
            continue;
        }
        nb.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let med = nb[nb.len() / 2];
        if band[t] > ratio * med.max(1e-12) {
            // Bring the band energy down to the median (energy ratio → amplitude gain).
            let g = (med / band[t]).sqrt().clamp(0.0, 1.0);
            for c in spectra[t][lo..=hi].iter_mut() {
                c.re *= g;
                c.im *= g;
            }
        }
    }

    let mut output = vec![0.0f32; n + fft_size];
    let mut wsum = vec![0.0f32; n + fft_size];
    let mut spec_buf = c2r.make_input_vec();
    let mut time_buf = c2r.make_output_vec();
    for (t, frame) in spectra.iter().enumerate() {
        spec_buf.copy_from_slice(frame);
        c2r.process(&mut spec_buf, &mut time_buf).unwrap();
        let off = t * hop_size;
        for i in 0..fft_size {
            output[off + i] += time_buf[i] * hann[i] * scale;
            wsum[off + i] += hann[i] * hann[i];
        }
    }
    for i in 0..n {
        if wsum[i] > 1e-6 {
            output[i] /= wsum[i];
        }
    }
    output.truncate(n);
    output
}

/// Tame plosive pops — the low-frequency bursts on "p"/"b" sounds — by
/// attenuating transient energy below ~250 Hz. `strength` 1–10 (higher removes
/// more). Sustained low-frequency content is preserved.
pub fn deplosive(signal: &[f32], sample_rate: u32, strength: f32) -> Vec<f32> {
    attenuate_band_transients(signal, sample_rate, 0.0, 250.0, strength)
}

/// Suppress lavalier / clothing rustle — transient bursts in the ~1.5–6 kHz band
/// are scaled back toward the local temporal median. `strength` 1–10. Sustained
/// speech in that band is left largely intact.
pub fn derustle(signal: &[f32], sample_rate: u32, strength: f32) -> Vec<f32> {
    attenuate_band_transients(signal, sample_rate, 1500.0, 6000.0, strength)
}
