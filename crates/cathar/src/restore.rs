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

/// How to redraw samples after a click is detected.
///
/// Detection is always sliding-window local RMS; methods differ only in the
/// reconstruction across the excised span. See the Rajmic et al. survey
/// discussion of reconstruction quality: AR / sparse methods beat smooth
/// polynomial fills when the gap carries residual structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeclickMethod {
    /// Autoregressive **Janssen / Godsill–Rayner** interpolation via
    /// [`crate::inpaint_gap`] (default). Same family as `cathar inpaint` —
    /// models the surrounding waveform and solves for the missing samples.
    #[default]
    Ar,
    /// Cubic-Hermite curve across the gap (legacy, fast). Fine for
    /// single-sample ticks; less natural on longer pops.
    Cubic,
}

/// Detect and interpolate impulse clicks (default: AR reconstruction).
///
/// Threshold is the number of local-RMS multiples above which a sample is a click.
/// Typical threshold: 8.0–15.0. See [`declick_with_method`] to pick the fill.
pub fn declick(signal: &[f32], threshold: f32, window: usize) -> Vec<f32> {
    declick_with_method(signal, threshold, window, DeclickMethod::default())
}

/// Detect and interpolate impulse clicks with an explicit [`DeclickMethod`].
///
/// Threshold is the number of local-RMS multiples above which a sample is a click.
/// Typical threshold: 8.0–15.0.
pub fn declick_with_method(
    signal: &[f32],
    threshold: f32,
    window: usize,
    method: DeclickMethod,
) -> Vec<f32> {
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
            // Grow a contiguous run so multi-sample pops become one gap, then
            // pad a few reliable shoulders for the interpolator.
            let mut start = i;
            while start > half && signal[start - 1].abs() > threshold * rms[start - 1] {
                start -= 1;
            }
            let mut end = i + 1;
            while end + half < n && signal[end].abs() > threshold * rms[end] {
                end += 1;
            }
            // Shoulder pad: leave known samples at the edges for the solver.
            let pad = half.clamp(2, 8);
            let gap_start = start.saturating_sub(pad);
            let gap_end = (end + pad).min(n);
            let gap_len = gap_end.saturating_sub(gap_start);
            if gap_len >= 2 && gap_start > 0 && gap_end < n {
                match method {
                    DeclickMethod::Ar => {
                        output = crate::inpaint::inpaint_gap(&output, gap_start, gap_len, 3);
                    }
                    DeclickMethod::Cubic => {
                        cubic_interpolate(&mut output, gap_start, gap_end - 1);
                    }
                }
            } else if gap_end > gap_start + 2 {
                // Edge of file or tiny hole — cubic is safe and always available.
                cubic_interpolate(&mut output, gap_start, (gap_end - 1).min(n - 1));
            }
            i = end.max(i + 1) + half.saturating_sub(1);
            continue;
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

/// How to tame plosive pops.
///
/// Detection of *where* a plosive is always looks at the low band; methods
/// differ in how widely they act. The default is event-gated so undamaged
/// material is returned bit-identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeplosiveMethod {
    /// Event-gated downward expander on the low band (default).
    ///
    /// A blast under 150 Hz that stands over the low band's running level and
    /// leads the mid band is taken down to the level the band held just before
    /// it; nothing else is touched. Measured free on undamaged material, unlike
    /// [`Transients`](Self::Transients).
    #[default]
    Events,
    /// Whole-file STFT band-transient suppression (legacy). Acts on every
    /// frame, so it costs fidelity on files with no plosives.
    Transients,
}

/// Tame plosive pops — the low-frequency bursts on "p"/"b" sounds.
///
/// Default is event-gated ([`DeplosiveMethod::Events`]): undamaged material is
/// returned unchanged. `strength` 1–10 maps to the detection threshold
/// (4 → 12 dB of low-band excess, the measured default). See
/// [`deplosive_with_method`] to keep the legacy whole-file path.
pub fn deplosive(signal: &[f32], sample_rate: u32, strength: f32) -> Vec<f32> {
    deplosive_with_method(signal, sample_rate, strength, DeplosiveMethod::default())
}

/// Tame plosive pops with an explicit [`DeplosiveMethod`].
pub fn deplosive_with_method(
    signal: &[f32],
    sample_rate: u32,
    strength: f32,
    method: DeplosiveMethod,
) -> Vec<f32> {
    match method {
        DeplosiveMethod::Transients => {
            attenuate_band_transients(signal, sample_rate, 0.0, 250.0, strength)
        }
        DeplosiveMethod::Events => deplosive_events(signal, sample_rate, strength),
    }
}

/// Suppress lavalier / clothing rustle — transient bursts in the ~1.5–6 kHz band
/// are scaled back toward the local temporal median. `strength` 1–10. Sustained
/// speech in that band is left largely intact.
pub fn derustle(signal: &[f32], sample_rate: u32, strength: f32) -> Vec<f32> {
    attenuate_band_transients(signal, sample_rate, 1500.0, 6000.0, strength)
}

// ── Event-gated plosive control ──────────────────────────────────────────────

const PLOSIVE_HOP: usize = 256;
const PLOSIVE_CROSSOVER_HZ: f32 = 150.0;
const PLOSIVE_MID_LO_HZ: f32 = 300.0;
const PLOSIVE_MID_HI_HZ: f32 = 3400.0;
const PLOSIVE_BASELINE_S: f32 = 1.0;
const PLOSIVE_ATTACK_HOPS: usize = 5;
const PLOSIVE_LEAD_DB: f32 = 6.0;
const PLOSIVE_MIN_MS: f32 = 10.0;
const PLOSIVE_MAX_MS: f32 = 120.0;
const PLOSIVE_ISOLATION_MS: f32 = 250.0;
const PLOSIVE_DENSITY_WINDOW_S: f32 = 10.0;
const PLOSIVE_DENSITY_MAX: usize = 6;
const PLOSIVE_RAMP_MS: f32 = 5.0;
const PLOSIVE_LEVEL_FLOOR: f32 = 1e-12;
/// Butterworth 4th-order section Qs (same pair as [`dewind`]).
const BUTTER_Q4: [f32; 2] = [0.541_196_1, 1.306_563];

fn deplosive_events(signal: &[f32], sample_rate: u32, strength: f32) -> Vec<f32> {
    let n = signal.len();
    let sr = sample_rate as f32;
    if n < PLOSIVE_HOP * 8 || sample_rate == 0 {
        return signal.to_vec();
    }
    // strength 1–10; CLI default 4 → 12 dB (the measured threshold).
    let excess_db = (16.0 - strength).clamp(6.0, 18.0);

    let low = filtfilt_lowpass(signal, sr, PLOSIVE_CROSSOVER_HZ);
    let mid = filtfilt_bandpass(signal, sr, PLOSIVE_MID_LO_HZ, PLOSIVE_MID_HI_HZ);
    let low_db = hop_levels_db(&low);
    let mid_db = hop_levels_db(&mid);
    if low_db.len() < 8 {
        return signal.to_vec();
    }
    let baseline = running_median(&low_db, median_radius(sr, PLOSIVE_BASELINE_S));
    let mid_base = running_median(&mid_db, median_radius(sr, PLOSIVE_BASELINE_S));

    let mut events = detect_plosive_events(&low_db, &mid_db, &baseline, &mid_base, excess_db, sr);
    // Butterworth filtfilt rings at the file edges; those hops are not plosives.
    let edge = ((0.05 * sr) as usize).max(PLOSIVE_HOP * 2);
    events.retain(|&(start, stop)| start >= edge && stop + edge <= n);
    if events.is_empty() {
        return signal.to_vec();
    }
    let gain = plosive_gain(&events, &low_db, &baseline, n, sr);
    let mut out = signal.to_vec();
    for ((o, g), l) in out.iter_mut().zip(gain.iter()).zip(low.iter()) {
        *o -= (1.0 - g) * l;
    }
    out
}

fn median_radius(sr: f32, seconds: f32) -> usize {
    let hops = ((seconds * sr / PLOSIVE_HOP as f32).round() as usize).max(3);
    hops / 2
}

fn hop_levels_db(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks_exact(PLOSIVE_HOP)
        .map(|c| {
            let e = c.iter().map(|v| v * v).sum::<f32>() / PLOSIVE_HOP as f32;
            10.0 * (e + PLOSIVE_LEVEL_FLOOR).log10()
        })
        .collect()
}

fn running_median(x: &[f32], radius: usize) -> Vec<f32> {
    let n = x.len();
    let mut out = vec![0.0f32; n];
    let mut buf = Vec::with_capacity(2 * radius + 1);
    for (i, slot) in out.iter_mut().enumerate() {
        let lo = i.saturating_sub(radius);
        let hi = (i + radius).min(n - 1);
        buf.clear();
        buf.extend_from_slice(&x[lo..=hi]);
        buf.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        *slot = buf[buf.len() / 2];
    }
    out
}

fn detect_plosive_events(
    low_db: &[f32],
    mid_db: &[f32],
    baseline: &[f32],
    mid_base: &[f32],
    excess_db: f32,
    sr: f32,
) -> Vec<(usize, usize)> {
    let n = low_db.len();
    let mut cand = vec![false; n];
    for i in 0..n {
        let rise = low_db[i] - baseline[i];
        let lead = rise - (mid_db[i] - mid_base[i]);
        cand[i] = rise >= excess_db && lead >= PLOSIVE_LEAD_DB;
    }
    let mut runs = Vec::new();
    let mut i = 0;
    while i < n {
        if cand[i] {
            let start = i;
            while i < n && cand[i] {
                i += 1;
            }
            let mut stop = i;
            if qualifies_plosive(low_db, baseline, start, stop, excess_db, sr) {
                // Extend through the decaying tail while the low band stays
                // half the excess up.
                let limit =
                    (start + ((PLOSIVE_MAX_MS * sr / 1000.0 / PLOSIVE_HOP as f32) as usize)).min(n);
                while stop < limit && low_db[stop] - baseline[stop] >= excess_db / 2.0 {
                    stop += 1;
                }
                runs.push((start * PLOSIVE_HOP, stop * PLOSIVE_HOP));
            }
        } else {
            i += 1;
        }
    }
    let isolated = isolate_plosives(&runs, sr);
    unrhythmic_plosives(&isolated, sr)
}

fn qualifies_plosive(
    low_db: &[f32],
    baseline: &[f32],
    start: usize,
    stop: usize,
    _excess_db: f32,
    sr: f32,
) -> bool {
    let length_ms = (stop - start) as f32 * PLOSIVE_HOP as f32 * 1000.0 / sr;
    if !(PLOSIVE_MIN_MS..=PLOSIVE_MAX_MS).contains(&length_ms) {
        return false;
    }
    let mut peak_at = 0usize;
    let mut peak = f32::NEG_INFINITY;
    for (k, i) in (start..stop).enumerate() {
        let rise = low_db[i] - baseline[i];
        if rise > peak {
            peak = rise;
            peak_at = k;
        }
    }
    peak_at <= PLOSIVE_ATTACK_HOPS
}

fn isolate_plosives(events: &[(usize, usize)], sr: f32) -> Vec<(usize, usize)> {
    if events.is_empty() {
        return Vec::new();
    }
    let gap = (PLOSIVE_ISOLATION_MS * sr / 1000.0) as usize;
    let mut kept = Vec::new();
    for (i, &(start, stop)) in events.iter().enumerate() {
        let prev_close = i > 0 && start.saturating_sub(events[i - 1].1) < gap;
        let next_close = i + 1 < events.len() && events[i + 1].0.saturating_sub(stop) < gap;
        if !prev_close && !next_close {
            kept.push((start, stop));
        }
    }
    kept
}

fn unrhythmic_plosives(events: &[(usize, usize)], sr: f32) -> Vec<(usize, usize)> {
    let half = (PLOSIVE_DENSITY_WINDOW_S * sr / 2.0) as usize;
    events
        .iter()
        .copied()
        .filter(|&(start, _)| {
            let around = events.iter().filter(|&&(s, _)| s.abs_diff(start) <= half).count();
            around <= PLOSIVE_DENSITY_MAX
        })
        .collect()
}

fn plosive_gain(
    events: &[(usize, usize)],
    low_db: &[f32],
    baseline: &[f32],
    length: usize,
    sr: f32,
) -> Vec<f32> {
    let mut hop_gain = vec![1.0f32; low_db.len()];
    for &(start, stop) in events {
        let first = (start / PLOSIVE_HOP).min(low_db.len().saturating_sub(1));
        let last = (stop / PLOSIVE_HOP).min(low_db.len());
        let target = baseline[first];
        for h in first..last {
            let g = 10.0f32.powf((target - low_db[h]) / 20.0);
            hop_gain[h] = hop_gain[h].min(g).min(1.0);
        }
    }
    let mut gain = vec![1.0f32; length];
    for (i, g) in gain.iter_mut().enumerate() {
        let pos = i as f32 / PLOSIVE_HOP as f32 - 0.5;
        if pos <= 0.0 {
            *g = hop_gain[0];
            continue;
        }
        let i0 = pos.floor() as usize;
        if i0 + 1 >= hop_gain.len() {
            *g = *hop_gain.last().unwrap();
            continue;
        }
        let t = pos - i0 as f32;
        *g = hop_gain[i0] * (1.0 - t) + hop_gain[i0 + 1] * t;
    }
    let ramp = ((PLOSIVE_RAMP_MS * sr / 1000.0) as usize).max(1);
    smooth_gain(&mut gain, ramp);
    gain
}

fn smooth_gain(gain: &mut [f32], ramp: usize) {
    if gain.is_empty() || ramp == 0 {
        return;
    }
    let n = gain.len();
    let mut kernel = vec![0.0f32; ramp + 2];
    let scale = std::f32::consts::PI / (ramp + 1) as f32;
    for (i, k) in kernel.iter_mut().enumerate() {
        *k = 0.5 - 0.5 * (scale * i as f32).cos();
    }
    kernel[0] = 0.0;
    *kernel.last_mut().unwrap() = 0.0;
    let ksum: f32 = kernel.iter().sum();
    if ksum <= 0.0 {
        return;
    }
    let orig = gain.to_vec();
    for (i, g) in gain.iter_mut().enumerate() {
        let mut acc = 0.0;
        let mut w = 0.0;
        for (k, &kv) in kernel.iter().enumerate() {
            let j = i as isize + k as isize - ramp as isize;
            if j >= 0 && (j as usize) < n {
                acc += orig[j as usize] * kv;
                w += kv;
            }
        }
        if w > 0.0 {
            *g = acc / w;
        }
    }
    for g in gain.iter_mut() {
        if (*g - 1.0).abs() < 1e-6 {
            *g = 1.0;
        }
    }
}

struct Bq {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Bq {
    fn lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos = w0.cos();
        let b0 = (1.0 - cos) / 2.0;
        let b1 = 1.0 - cos;
        let b2 = (1.0 - cos) / 2.0;
        let a0 = 1.0 + alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn highpass(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos = w0.cos();
        let b0 = (1.0 + cos) / 2.0;
        let b1 = -(1.0 + cos);
        let b2 = (1.0 + cos) / 2.0;
        let a0 = 1.0 + alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

fn filtfilt_cascade(signal: &[f32], sections: &mut [Bq]) -> Vec<f32> {
    let mut y = signal.to_vec();
    for s in sections.iter_mut() {
        s.reset();
        for v in y.iter_mut() {
            *v = s.tick(*v);
        }
        s.reset();
        for v in y.iter_mut().rev() {
            *v = s.tick(*v);
        }
    }
    y
}

fn filtfilt_lowpass(signal: &[f32], sr: f32, cutoff: f32) -> Vec<f32> {
    let mut secs = [Bq::lowpass(sr, cutoff, BUTTER_Q4[0]), Bq::lowpass(sr, cutoff, BUTTER_Q4[1])];
    filtfilt_cascade(signal, &mut secs)
}

fn filtfilt_bandpass(signal: &[f32], sr: f32, lo: f32, hi: f32) -> Vec<f32> {
    let mut secs = [
        Bq::highpass(sr, lo, BUTTER_Q4[0]),
        Bq::highpass(sr, lo, BUTTER_Q4[1]),
        Bq::lowpass(sr, hi, BUTTER_Q4[0]),
        Bq::lowpass(sr, hi, BUTTER_Q4[1]),
    ];
    filtfilt_cascade(signal, &mut secs)
}
