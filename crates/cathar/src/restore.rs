//! Repair & reduction: de-hum, de-wind, de-click, de-clip, de-reverb,
//! spectral repair, de-plosive, de-rustle.

use crate::util::hann_window;
use realfft::RealFftPlanner;

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

// ── De-clip (LSAR — least-squares AR interpolation) ──────────────────────────

/// Levinson-Durbin recursion: solve the autocorrelation normal equations for the
/// order-`p` linear-prediction (AR) coefficients. Returns `a[0..=p]` with
/// `a[0] = 1` for the prediction-error filter `A(z) = 1 + Σ a_k z^-k`.
fn levinson(r: &[f64], p: usize) -> Vec<f64> {
    let mut a = vec![0.0f64; p + 1];
    a[0] = 1.0;
    let mut e = r[0];
    if e <= 0.0 {
        return a;
    }
    for i in 1..=p {
        let mut acc = r[i];
        for j in 1..i {
            acc += a[j] * r[i - j];
        }
        let k = -acc / e;
        let prev = a.clone();
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        e *= 1.0 - k * k;
        if e <= 1e-12 {
            break;
        }
    }
    a
}

/// Autocorrelation up to lag `p`, summed over the two reliable blocks either side
/// of the gap (no lag crosses the gap, so the clipped samples never pollute it).
fn autocorr(before: &[f32], after: &[f32], p: usize) -> Vec<f64> {
    let mut r = vec![0.0f64; p + 1];
    for d in 0..=p {
        let mut acc = 0.0f64;
        for blk in [before, after] {
            for n in 0..blk.len().saturating_sub(d) {
                acc += blk[n] as f64 * blk[n + d] as f64;
            }
        }
        r[d] = acc;
    }
    r[0] *= 1.0 + 1e-6; // tiny white-noise floor for numerical stability
    r
}

/// Solve a symmetric positive-definite system `M y = b` in place via Cholesky
/// (`M` becomes its lower factor, `b` becomes the solution). Returns false if `M`
/// is not positive definite.
#[allow(clippy::needless_range_loop)] // index loops are the clearest form for Cholesky
fn solve_spd(m: &mut [Vec<f64>], b: &mut [f64]) -> bool {
    let n = b.len();
    for i in 0..n {
        for j in 0..=i {
            let mut sum = m[i][j];
            for k in 0..j {
                sum -= m[i][k] * m[j][k];
            }
            if i == j {
                if sum <= 1e-12 {
                    return false;
                }
                m[i][j] = sum.sqrt();
            } else {
                m[i][j] = sum / m[j][j];
            }
        }
    }
    for i in 0..n {
        let mut sum = b[i];
        for k in 0..i {
            sum -= m[i][k] * b[k];
        }
        b[i] = sum / m[i][i];
    }
    for i in (0..n).rev() {
        let mut sum = b[i];
        for k in (i + 1)..n {
            sum -= m[k][i] * b[k];
        }
        b[i] = sum / m[i][i];
    }
    true
}

/// Reconstruct the clipped run `out[s..e]` by **least-squares AR interpolation**
/// (Janssen, Veldhuis & Vries 1986): fit an AR model to the surrounding reliable
/// audio, then solve for the gap samples that minimise that model's prediction
/// error. Falls back to a smooth curve when the context is too short, the gap too
/// long, or the AR solve is unstable (a resonant model can *ring* across a long
/// clipped run and overshoot wildly — on badly-clipped audio we'd rather soften
/// than introduce that, matching what a real de-clipper does).
fn lsar_interpolate(out: &mut [f32], s: usize, e: usize) {
    const MAX_P: usize = 32;
    const CTX: usize = 1024;
    const MAX_RUN: usize = 256;
    let n = out.len();
    let l = e - s;

    let before: Vec<f32> = out[s.saturating_sub(CTX)..s].to_vec();
    let after: Vec<f32> = out[e..(e + CTX).min(n)].to_vec();
    let ctx_len = before.len() + after.len();
    let p = MAX_P.min(ctx_len.saturating_sub(1));

    if l == 0 || l > MAX_RUN || p < 2 {
        cubic_interpolate(out, s.saturating_sub(1), e.min(n - 1));
        return;
    }

    let a = levinson(&autocorr(&before, &after, p), p);
    // Filter autocorrelation ra[d] = Σ_j a[j]·a[j+d] (the banded system's entries).
    let mut ra = vec![0.0f64; p + 1];
    for (d, rad) in ra.iter_mut().enumerate() {
        *rad = (0..=(p - d)).map(|j| a[j] * a[j + d]).sum();
    }

    // Minimise Σ prediction-error² over the unknown samples → M·x_U = rhs, where
    // M[i][j] = ra[|i-j|] and rhs pulls in the known neighbours within p.
    let mut m = vec![vec![0.0f64; l]; l];
    let mut rhs = vec![0.0f64; l];
    for i in 0..l {
        for (j, cell) in m[i].iter_mut().enumerate() {
            let d = i.abs_diff(j);
            if d <= p {
                *cell = ra[d];
            }
        }
        let ui = (s + i) as isize;
        let lo = (ui - p as isize).max(0);
        let hi = (ui + p as isize).min(n as isize - 1);
        let mut acc = 0.0f64;
        for jj in lo..=hi {
            let j = jj as usize;
            if (s..e).contains(&j) {
                continue; // unknown — handled by M
            }
            acc -= ra[(ui - jj).unsigned_abs()] * out[j] as f64;
        }
        rhs[i] = acc;
    }

    let solved = solve_spd(&mut m, &mut rhs);
    // Guard against an unstable solve. A real restored peak is a single smooth
    // hump (its slope reverses once); a resonant AR model rings — its slope
    // reverses many times across the gap. The magnitude cap is a blow-up backstop
    // (a faithful de-clipped peak may legitimately pass full scale — you normalise
    // afterwards — but never by 2×).
    let max_abs = rhs.iter().fold(0.0f64, |a, &v| a.max(v.abs()));
    let reversals =
        rhs.windows(3).filter(|w| (w[1] - w[0]).signum() != (w[2] - w[1]).signum()).count();
    if solved && max_abs <= 2.0 && reversals <= 2 {
        for (i, &v) in rhs.iter().enumerate() {
            out[s + i] = v as f32;
        }
    } else {
        cubic_interpolate(out, s.saturating_sub(1), e.min(n - 1));
    }
}

/// Reconstruct clipped samples with least-squares AR interpolation (LSAR).
///
/// Clipping is detected as consecutive samples at or beyond `threshold` (e.g.
/// 0.95). Each clipped run is filled with the samples that best fit an
/// autoregressive model of the surrounding reliable audio (Janssen·Veldhuis·Vries
/// 1986) — the classical audio-restoration method — so a tonal peak is rebuilt
/// toward its true amplitude rather than flattened. Works for positive and
/// negative clipping; very long runs or too-short context fall back to a smooth
/// interpolation.
pub fn declip(signal: &[f32], threshold: f32) -> Vec<f32> {
    let n = signal.len();
    let mut output = signal.to_vec();
    let mut i = 0;
    while i < n {
        if signal[i].abs() >= threshold {
            let start = i;
            while i < n && signal[i].abs() >= threshold {
                i += 1;
            }
            lsar_interpolate(&mut output, start, i);
        } else {
            i += 1;
        }
    }
    output
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
