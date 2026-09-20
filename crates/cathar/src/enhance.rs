//! Enhancement: voice isolation, de-essing, breath removal, bandwidth extension.

use crate::filter::highpass;
use crate::util::hann_window;
use crate::{NoisePrint, resample};
use realfft::RealFftPlanner;

/// Bandwidth-extension strategy for [`bandwidth_extend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnhanceMethod {
    /// Spectral band replication (default, shipped in `v0.5`).
    #[default]
    Replicate,
    /// Log-spaced magnitude interpolation into the empty high band.
    Interpolate,
    /// Overtone extrapolation — pick peaks in the existing top band and extend
    /// each harmonic series into the empty highs (HRAudioWizard HFP family).
    Harmonic,
    /// Nonlinear harmonic generation of the existing highs, high-passed into
    /// the empty band (DSRE / DSEE-like; no learned weights).
    Dsre,
}

/// Isolate speech from background using energy-based VAD + spectral gating.
pub fn voice_isolate(
    signal: &[f32],
    sample_rate: u32,
    noise_print: Option<&NoisePrint>,
) -> Vec<f32> {
    let fft_size = 2048;
    let hop_size = 512;
    let n = signal.len();
    if n < fft_size {
        return signal.to_vec();
    }

    let frame_len = sample_rate as usize / 50;
    let vad_frames = n / frame_len;
    let mut frame_energies: Vec<f32> = Vec::with_capacity(vad_frames);
    for fi in 0..vad_frames {
        let start = fi * frame_len;
        let end = (start + frame_len).min(n);
        let energy: f32 =
            signal[start..end].iter().map(|s| s * s).sum::<f32>() / (end - start) as f32;
        frame_energies.push(energy);
    }
    if frame_energies.is_empty() {
        return signal.to_vec();
    }

    let mut sorted = frame_energies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor = sorted[(sorted.len() / 5).min(sorted.len() - 1)];
    let threshold = noise_floor * 4.0;

    let mut is_voice = vec![false; vad_frames + 1];
    for (fi, &energy) in frame_energies.iter().enumerate() {
        is_voice[fi] = energy > threshold;
    }

    let min_voice_frames = (sample_rate as f32 * 0.05 / frame_len as f32).max(1.0) as usize;
    let max_gap_frames = (sample_rate as f32 * 0.12 / frame_len as f32).max(1.0) as usize;

    let mut smoothed = is_voice.clone();
    for fi in 0..vad_frames {
        if !is_voice[fi] {
            let voice_before = (1..=max_gap_frames).any(|j| fi >= j && is_voice[fi - j]);
            let voice_after = (1..=max_gap_frames).any(|j| fi + j < vad_frames && is_voice[fi + j]);
            if voice_before && voice_after {
                smoothed[fi] = true;
            }
        }
    }

    for fi in 0..vad_frames {
        if smoothed[fi] {
            let run = (fi..vad_frames).take_while(|&j| j < vad_frames && smoothed[j]).count();
            if run < min_voice_frames {
                for s in smoothed.iter_mut().take((fi + run).min(vad_frames)).skip(fi) {
                    *s = false;
                }
            }
        }
    }

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / (fft_size as f32);
    let n_bins = fft_size / 2 + 1;
    let frames = n / hop_size;
    let mut output = vec![0.0f32; n + fft_size];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();
    let noise_spec = noise_print.map(|np| &np.spectrum);

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        let center_sample = offset + fft_size / 2;
        let vad_idx = center_sample / frame_len;
        let voice_present = smoothed.get(vad_idx).copied().unwrap_or(false);

        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        if !voice_present {
            if let Some(ns) = noise_spec {
                for k in 0..n_bins {
                    let mag =
                        (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
                    let gate = (ns[k] * 0.3 / mag.max(1e-10)).min(1.0);
                    out_buf[k].re *= gate;
                    out_buf[k].im *= gate;
                }
            } else {
                for val in out_buf.iter_mut().take(n_bins) {
                    val.re *= 0.01;
                    val.im *= 0.01;
                }
            }
        }

        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
        }
    }
    output.truncate(n);
    output
}

// ── De-esser ────────────────────────────────────────────────────────────────

/// Reduce sibilance (harsh "s", "sh", "ch" sounds) using HF compression.
pub fn deesser(
    signal: &[f32],
    sample_rate: u32,
    crossover_freq: f32,
    threshold_db: f32,
    ratio: f32,
) -> Vec<f32> {
    let fft_size = 2048;
    let hop_size = 256;
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
    let nyquist = sample_rate as f32 / 2.0;
    let crossover_bin = ((crossover_freq / nyquist) * (n_bins - 1) as f32).round() as usize;
    let frames = n / hop_size;
    let threshold_linear = 10.0f32.powf(threshold_db / 20.0);

    let mut output = vec![0.0f32; n + fft_size];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        let mut broadband_power = 0.0f32;
        let mut hf_power = 0.0f32;
        for (k, val) in out_buf.iter().enumerate().take(n_bins) {
            let power = val.re * val.re + val.im * val.im;
            broadband_power += power;
            if k >= crossover_bin {
                hf_power += power;
            }
        }

        let broadband_rms = broadband_power.sqrt();
        let hf_rms = hf_power.sqrt();
        let hf_ratio = if broadband_rms > 1e-10 { hf_rms / broadband_rms } else { 0.0 };

        if hf_ratio > threshold_linear {
            let overshoot = hf_ratio / threshold_linear;
            let gain_reduction = 1.0 / (1.0 + (overshoot - 1.0) * ratio);
            for (k, val) in out_buf.iter_mut().enumerate().take(n_bins) {
                let frac = (k.min(crossover_bin) as f32 / crossover_bin.max(1) as f32).min(1.0);
                let bin_gain = gain_reduction + (1.0 - gain_reduction) * (1.0 - frac);
                val.re *= bin_gain;
                val.im *= bin_gain;
            }
        }

        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
        }
    }
    output.truncate(n);
    output
}

/// Multiband, adaptive de-esser. The region above `crossover_freq` is split into
/// `bands` sub-bands; each tracks its own short-term level (an exponential moving
/// average) and is compressed by `ratio` only when its instantaneous level rises
/// `threshold_db` above that adaptive average. Adapting *per band* and *over
/// time* catches sibilance concentrated in part of the band and follows a
/// speaker's changing level, where a single fixed-threshold band over- or
/// under-reacts. Falls back to a single band when `bands <= 1`.
///
/// `threshold_db` is **dB above the running average** (try 3–6). It is not the
/// single-band [`deesser`] HF/broadband-ratio threshold (default −24). A
/// negative value would compress every frame and take down the whole region
/// above `crossover_freq`; those values are floored at 0 dB.
pub fn deess_multiband(
    signal: &[f32],
    sample_rate: u32,
    crossover_freq: f32,
    threshold_db: f32,
    ratio: f32,
    bands: usize,
) -> Vec<f32> {
    let fft_size = 2048;
    let hop_size = 256;
    let n = signal.len();
    if n < fft_size {
        return signal.to_vec();
    }
    let bands = bands.max(1);
    // Negative values mean "this far *below* the average", so every frame is
    // over and the HF region is crushed. Floor at 0 dB (compress on a rise).
    let threshold_db = threshold_db.max(0.0);
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / fft_size as f32;
    let n_bins = fft_size / 2 + 1;
    let nyquist = sample_rate as f32 / 2.0;
    let crossover_bin =
        (((crossover_freq / nyquist) * (n_bins - 1) as f32).round() as usize).min(n_bins - 1);
    let frames = n / hop_size;
    // `threshold_db` is how far above the running average triggers compression.
    let threshold_linear = 10.0f32.powf(threshold_db / 20.0);
    let span = n_bins.saturating_sub(crossover_bin).max(1);
    let band_width = span.div_ceil(bands);
    // Adaptive per-band level; EMA smoothing — slow enough that a sibilant
    // transient stands above the running average, fast enough to follow speech.
    let mut avg = vec![0.0f32; bands];
    let smoothing = 0.85f32;

    let mut output = vec![0.0f32; n + fft_size];
    let mut wsum = vec![0.0f32; n + fft_size];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        for (band, level) in avg.iter_mut().enumerate() {
            let lo = crossover_bin + band * band_width;
            let hi = (lo + band_width).min(n_bins);
            if lo >= hi {
                continue;
            }
            let power: f32 = out_buf[lo..hi].iter().map(|v| v.re * v.re + v.im * v.im).sum();
            let rms = power.sqrt();
            if *level <= 0.0 {
                *level = rms; // seed
            }
            let over = rms / (*level * threshold_linear).max(1e-10);
            if over > 1.0 {
                let gr = 1.0 / (1.0 + (over - 1.0) * ratio);
                for v in out_buf[lo..hi].iter_mut() {
                    v.re *= gr;
                    v.im *= gr;
                }
            }
            // Track the pre-compression level so transient sibilance reads as "over".
            *level = smoothing * *level + (1.0 - smoothing) * rms;
        }

        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
            wsum[offset + i] += hann[i] * hann[i];
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

// ── Breath removal ──────────────────────────────────────────────────────────

/// Detect and attenuate breath sounds between speech segments.
pub fn breath_remove(signal: &[f32], sample_rate: u32) -> Vec<f32> {
    let frame_len = sample_rate as usize / 50;
    let n = signal.len();
    if n < frame_len * 10 {
        return signal.to_vec();
    }
    let vad_frames = n / frame_len;

    let mut frame_energies: Vec<f32> = Vec::with_capacity(vad_frames);
    for fi in 0..vad_frames {
        let start = fi * frame_len;
        let end = (start + frame_len).min(n);
        let energy: f32 =
            signal[start..end].iter().map(|s| s * s).sum::<f32>() / (end - start) as f32;
        frame_energies.push(energy);
    }
    if frame_energies.is_empty() {
        return signal.to_vec();
    }

    let mut sorted = frame_energies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor = sorted[(sorted.len() / 5).min(sorted.len() - 1)];
    let threshold = noise_floor * 4.0;

    let mut is_voice = vec![false; vad_frames];
    for (fi, &energy) in frame_energies.iter().enumerate() {
        is_voice[fi] = energy > threshold;
    }

    let breath_margin_frames = (sample_rate as f32 * 0.15 / frame_len as f32) as usize;
    let mut breath_mask = vec![false; n];
    for fi in 1..vad_frames {
        if is_voice[fi] && !is_voice[fi - 1] {
            let start_frame = fi.saturating_sub(breath_margin_frames);
            for bf in start_frame..fi {
                let bs = bf * frame_len;
                let be = ((bf + 1) * frame_len).min(n);
                for b in breath_mask.iter_mut().take(be).skip(bs) {
                    *b = true;
                }
            }
        }
    }

    let hpf_cutoff = 200.0;
    let hpf_coeff = (-2.0 * std::f32::consts::PI * hpf_cutoff / sample_rate as f32).exp();
    let mut output = signal.to_vec();
    let (mut prev_in, mut prev_out) = (0.0f32, 0.0f32);
    let dry_wet = 0.6;

    for (i, s) in signal.iter().enumerate() {
        if breath_mask[i] {
            let hp = hpf_coeff * prev_out + hpf_coeff * (*s - prev_in);
            prev_in = *s;
            prev_out = hp;
            output[i] = *s * (1.0 - dry_wet) + hp * dry_wet;
        }
    }
    output
}

/// Restore high-frequency content lost to compression or low sample rates.
///
/// [`EnhanceMethod::Replicate`] tiles the upper-octave envelope (SBR).
/// [`EnhanceMethod::Interpolate`] extrapolates log-magnitude.
/// [`EnhanceMethod::Harmonic`] extends detected overtone series.
/// [`EnhanceMethod::Dsre`] generates new highs by a mild nonlinearity.
/// No ML — pure DSP, zero weights.
pub fn bandwidth_extend(signal: &[f32], sample_rate: u32, target_rate: u32) -> Vec<f32> {
    bandwidth_extend_with_method(signal, sample_rate, target_rate, EnhanceMethod::Replicate)
}

/// Like [`bandwidth_extend`] but selects the upsampling strategy.
///
/// [`EnhanceMethod::Replicate`] and [`EnhanceMethod::Interpolate`] still
/// require `target_rate > sample_rate` (identity otherwise). Harmonic and
/// DSRE also fill a rolled-off top at the same rate.
pub fn bandwidth_extend_with_method(
    signal: &[f32],
    sample_rate: u32,
    target_rate: u32,
    method: EnhanceMethod,
) -> Vec<f32> {
    let out_rate = target_rate.max(sample_rate);
    let resampled = if out_rate > sample_rate {
        resample(signal, sample_rate, out_rate)
    } else {
        signal.to_vec()
    };

    match method {
        EnhanceMethod::Replicate => {
            if target_rate <= sample_rate {
                return signal.to_vec();
            }
            replicate_high_band(&resampled, sample_rate, target_rate)
        }
        EnhanceMethod::Interpolate => {
            if target_rate <= sample_rate {
                return signal.to_vec();
            }
            interpolate_high_band(&resampled, sample_rate, target_rate)
        }
        EnhanceMethod::Harmonic => harmonic_high_band(&resampled, sample_rate, out_rate),
        EnhanceMethod::Dsre => dsre_high_band(&resampled, sample_rate, out_rate),
    }
}

fn replicate_high_band(resampled: &[f32], sample_rate: u32, target_rate: u32) -> Vec<f32> {
    // ── 2. SBR: replicate low-band spectrum shape into high band ──
    let fft_size = 4096;
    let hop_size = fft_size / 4;
    let n = resampled.len();
    if n < fft_size {
        return resampled.to_vec();
    }

    let old_nyquist = sample_rate as f32 / 2.0;
    let new_nyquist = target_rate as f32 / 2.0;
    let n_bins = fft_size / 2 + 1;
    let old_nyquist_bin = ((old_nyquist / new_nyquist) * (n_bins - 1) as f32).round() as usize;
    let source_band_start = (old_nyquist_bin as f32 * 0.6).round() as usize;
    let source_band_width = old_nyquist_bin - source_band_start;

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / (fft_size as f32);
    let frames = n / hop_size;

    let mut output = vec![0.0f32; n + fft_size];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = resampled[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        // Estimate spectral envelope from source band
        let mut envelope = vec![0.0f32; source_band_width];
        for (k, env) in envelope.iter_mut().enumerate() {
            let src_k = source_band_start + k;
            *env = (out_buf[src_k].re * out_buf[src_k].re + out_buf[src_k].im * out_buf[src_k].im)
                .sqrt();
        }

        // Replicate into high band with gentle rolloff
        for tile in 0..4 {
            let target_start = old_nyquist_bin + tile * source_band_width;
            if target_start >= n_bins - 1 {
                break;
            }
            let rolloff = 1.0 - tile as f32 * 0.3;
            if rolloff <= 0.0 {
                break;
            }
            for (k, env_val) in envelope.iter().enumerate() {
                let tgt = target_start + k;
                if tgt >= n_bins - 1 {
                    break;
                }
                let existing =
                    (out_buf[tgt].re * out_buf[tgt].re + out_buf[tgt].im * out_buf[tgt].im).sqrt();
                if existing < 1e-6 {
                    let src_k = source_band_start + k;
                    let phase =
                        out_buf[src_k].im.atan2(out_buf[src_k].re) + std::f32::consts::PI * 0.25;
                    let sbr_amp = env_val * rolloff * 0.6;
                    let freq_rolloff =
                        (-(tgt as f32 - old_nyquist_bin as f32) / 300.0).exp().max(0.02);
                    let amp = sbr_amp * freq_rolloff;
                    out_buf[tgt].re += amp * phase.cos();
                    out_buf[tgt].im += amp * phase.sin();
                }
            }
        }

        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
        }
    }

    output.truncate(n);
    output
}

fn interpolate_high_band(resampled: &[f32], sample_rate: u32, target_rate: u32) -> Vec<f32> {
    let fft_size = 4096;
    let hop_size = fft_size / 4;
    let n = resampled.len();
    if n < fft_size {
        return resampled.to_vec();
    }

    let old_nyquist = sample_rate as f32 / 2.0;
    let new_nyquist = target_rate as f32 / 2.0;
    let n_bins = fft_size / 2 + 1;
    let old_nyquist_bin = ((old_nyquist / new_nyquist) * (n_bins - 1) as f32).round() as usize;

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / (fft_size as f32);
    let frames = n / hop_size;

    let mut output = vec![0.0f32; n + fft_size];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = resampled[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        let mut log_env = vec![-80.0f32; old_nyquist_bin.max(2)];
        for k in 1..old_nyquist_bin {
            let mag =
                (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt().max(1e-10);
            log_env[k] = mag.log10() * 20.0;
        }

        // Extrapolate log-magnitude with a linear slope from the top two octaves.
        let fit_start = (old_nyquist_bin as f32 * 0.25).round() as usize;
        let fit_end = old_nyquist_bin.saturating_sub(1);
        if fit_end > fit_start {
            let slope = (log_env[fit_end] - log_env[fit_start])
                / (fit_end.saturating_sub(fit_start).max(1) as f32);
            for tgt in old_nyquist_bin..n_bins - 1 {
                let delta = (tgt - fit_end) as f32;
                let target_db = log_env[fit_end] + slope * delta * 0.5;
                let rolloff = (-delta / 400.0).exp().max(0.02);
                let amp = 10.0f32.powf(target_db / 20.0) * rolloff * 0.5;
                let existing =
                    (out_buf[tgt].re * out_buf[tgt].re + out_buf[tgt].im * out_buf[tgt].im).sqrt();
                if existing < amp * 0.5 {
                    let src = tgt.saturating_sub(old_nyquist_bin / 2).max(1);
                    let phase = out_buf[src].im.atan2(out_buf[src].re);
                    out_buf[tgt].re += amp * phase.cos();
                    out_buf[tgt].im += amp * phase.sin();
                }
            }
        }

        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
        }
    }

    output.truncate(n);
    output
}

/// Extend detected overtone series into the empty high band.
///
/// Per frame: pick spectral peaks below the original Nyquist, then place
/// harmonics `k·f` above it with a decaying envelope and phase-locked to the
/// fundamental (`k · φ`). Deterministic — no Griffin-Lim, no random dither.
fn harmonic_high_band(resampled: &[f32], src_rate: u32, out_rate: u32) -> Vec<f32> {
    let fft_size = 4096;
    let hop_size = fft_size / 4;
    let n = resampled.len();
    if n < fft_size || out_rate == 0 {
        return resampled.to_vec();
    }

    let n_bins = fft_size / 2 + 1;
    let hz_per_bin = out_rate as f32 / fft_size as f32;
    let orig_nyq_bin = ((src_rate as f32 / 2.0 / hz_per_bin).round() as usize).clamp(8, n_bins - 2);
    let ceiling = spectral_ceiling_bin(resampled, fft_size, orig_nyq_bin).min(orig_nyq_bin);
    if ceiling + 4 >= n_bins {
        return resampled.to_vec();
    }

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / fft_size as f32;
    let frames = n / hop_size;

    let mut output = vec![0.0f32; n + fft_size];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = resampled[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        let mag: Vec<f32> = out_buf.iter().map(|c| (c.re * c.re + c.im * c.im).sqrt()).collect();
        let peaks = pick_overtone_peaks(&mag, ceiling);
        for &(bin, amp) in &peaks {
            if bin == 0 {
                continue;
            }
            let phase = out_buf[bin].im.atan2(out_buf[bin].re);
            let mut k = 2usize;
            loop {
                let tgt = bin * k;
                if tgt >= n_bins - 1 {
                    break;
                }
                if tgt > ceiling {
                    let decay = 0.55f32.powi((k - 1) as i32);
                    let rolloff = (-((tgt - ceiling) as f32) / 350.0).exp().max(0.02);
                    let add = amp * decay * rolloff * 0.7;
                    let existing = mag[tgt];
                    if existing < add * 0.4 {
                        let phi = phase * k as f32;
                        out_buf[tgt].re += add * phi.cos();
                        out_buf[tgt].im += add * phi.sin();
                    }
                }
                k += 1;
            }
        }

        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
        }
    }
    output.truncate(n);
    output
}

/// DSRE-style: waveshape the existing highs, then keep only the new content
/// above the original ceiling.
fn dsre_high_band(resampled: &[f32], src_rate: u32, out_rate: u32) -> Vec<f32> {
    let n = resampled.len();
    if n < 64 || out_rate == 0 {
        return resampled.to_vec();
    }
    let orig_nyq = src_rate as f32 / 2.0;
    let new_nyq = out_rate as f32 / 2.0;
    if new_nyq <= orig_nyq * 1.02 {
        let fft_size = 4096.min(n.next_power_of_two().min(n) & !1).max(256);
        let orig_bin = ((orig_nyq / (out_rate as f32 / fft_size as f32)).round() as usize)
            .clamp(8, fft_size / 2 - 2);
        let ceiling_bin = spectral_ceiling_bin(resampled, fft_size, orig_bin);
        let ceiling_hz = ceiling_bin as f32 * out_rate as f32 / fft_size as f32;
        if ceiling_hz >= orig_nyq * 0.92 {
            return resampled.to_vec();
        }
        return dsre_mix(resampled, out_rate, (ceiling_hz * 0.4).clamp(1500.0, 8000.0), ceiling_hz);
    }
    dsre_mix(resampled, out_rate, (orig_nyq * 0.2).clamp(800.0, 4000.0), orig_nyq * 0.85)
}

fn dsre_mix(resampled: &[f32], sr: u32, pre_hz: f32, post_hz: f32) -> Vec<f32> {
    let mut x = highpass(resampled, sr, pre_hz);
    x = highpass(&x, sr, pre_hz);
    for _ in 0..4 {
        for s in &mut x {
            let v = *s;
            *s = (v + 0.35 * v * v.abs() + 0.12 * v * v * v).clamp(-1.5, 1.5);
        }
    }
    x = highpass(&x, sr, post_hz);
    x = highpass(&x, sr, post_hz);
    let src_rms = rms(resampled).max(1e-9);
    let gen_rms = rms(&x).max(1e-9);
    let gain = (src_rms * 0.28 / gen_rms).min(0.6);
    resampled.iter().zip(x.iter()).map(|(o, n)| o + n * gain).collect()
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn spectral_ceiling_bin(signal: &[f32], fft_size: usize, orig_nyq_bin: usize) -> usize {
    let hop = fft_size / 2;
    let n = signal.len();
    if n < fft_size {
        return orig_nyq_bin;
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let hann = hann_window(fft_size);
    let n_bins = fft_size / 2 + 1;
    let mut acc = vec![0.0f32; n_bins];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();
    let mut frames = 0usize;
    let mut offset = 0;
    while offset + fft_size <= n {
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).ok();
        for (a, c) in acc.iter_mut().zip(out_buf.iter()) {
            *a += (c.re * c.re + c.im * c.im).sqrt();
        }
        frames += 1;
        offset += hop;
        if frames >= 32 {
            break;
        }
    }
    if frames == 0 {
        return orig_nyq_bin;
    }
    for a in &mut acc {
        *a /= frames as f32;
    }
    let mid_lo = (orig_nyq_bin / 8).max(1);
    let mid_hi = (orig_nyq_bin / 2).max(mid_lo + 1);
    let mut mid: Vec<f32> = acc[mid_lo..mid_hi.min(acc.len())].to_vec();
    if mid.is_empty() {
        return orig_nyq_bin;
    }
    mid.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = mid[mid.len() / 2] * 0.04;
    let hi = orig_nyq_bin.min(n_bins - 2);
    let mut ceiling = hi;
    for k in (8..=hi).rev() {
        if acc[k] > floor {
            ceiling = k;
            break;
        }
    }
    ceiling.max(8)
}

fn pick_overtone_peaks(mag: &[f32], ceiling: usize) -> Vec<(usize, f32)> {
    let lo = (ceiling / 5).max(2);
    let hi = ceiling.saturating_sub(2).max(lo + 1);
    if hi + 1 >= mag.len() || hi <= lo {
        return Vec::new();
    }
    let mut peaks: Vec<(usize, f32)> = Vec::new();
    for k in lo..hi {
        if mag[k] > mag[k - 1] && mag[k] >= mag[k + 1] {
            peaks.push((k, mag[k]));
        }
    }
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    peaks.truncate(16);
    let floor = peaks.first().map(|p| p.1 * 0.08).unwrap_or(0.0);
    peaks.retain(|p| p.1 >= floor);
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(sr: u32, secs: f32, freq: f32, amp: f32) -> Vec<f32> {
        let n = (sr as f32 * secs) as usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        (0..n).map(|i| amp * (two_pi * freq * i as f32 / sr as f32).sin()).collect()
    }

    fn mag_at(x: &[f32], f: f32, fs: u32) -> f64 {
        let two_pi = 2.0 * std::f64::consts::PI;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        let n = x.len();
        for (i, &v) in x.iter().enumerate() {
            let p = two_pi * f as f64 * i as f64 / fs as f64;
            re += v as f64 * p.cos();
            im -= v as f64 * p.sin();
        }
        (re * re + im * im).sqrt() / n as f64
    }

    #[test]
    fn harmonic_places_overtone_above_original_nyquist() {
        let sr = 8_000u32;
        let x = tone(sr, 1.0, 1000.0, 0.5);
        let out = bandwidth_extend_with_method(&x, sr, 16_000, EnhanceMethod::Harmonic);
        assert_eq!(out.len(), 16_000);
        // 5th harmonic of 1 kHz sits at 5 kHz, above the original 4 kHz Nyquist.
        let hi = mag_at(&out, 5000.0, 16_000);
        let fund = mag_at(&out, 1000.0, 16_000);
        assert!(fund > 0.05, "fundamental lost: {fund}");
        assert!(hi > fund * 0.02, "no overtone above original Nyquist: 5 kHz={hi}, 1 kHz={fund}");
    }

    #[test]
    fn dsre_adds_energy_above_original_nyquist() {
        let sr = 8_000u32;
        let mut x = tone(sr, 1.0, 1000.0, 0.4);
        for (i, s) in x.iter_mut().enumerate() {
            *s += 0.3 * (2.0 * std::f32::consts::PI * 1800.0 * i as f32 / sr as f32).sin();
        }
        let resampled = resample(&x, sr, 16_000);
        let out = bandwidth_extend_with_method(&x, sr, 16_000, EnhanceMethod::Dsre);
        let band = |v: &[f32]| mag_at(v, 5000.0, 16_000) + mag_at(v, 6000.0, 16_000);
        assert!(
            band(&out) > band(&resampled) * 1.5,
            "DSRE should add energy above 4 kHz: {} vs {}",
            band(&out),
            band(&resampled)
        );
    }

    #[test]
    fn replicate_identity_when_rate_does_not_increase() {
        let x = tone(48_000, 0.25, 440.0, 0.3);
        let out = bandwidth_extend_with_method(&x, 48_000, 48_000, EnhanceMethod::Replicate);
        assert_eq!(out, x);
    }

    fn two_tone(sr: u32, secs: f32, f1: f32, f2: f32, amp: f32) -> Vec<f32> {
        let n = (sr as f32 * secs) as usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                amp * (two_pi * f1 * t).sin() + amp * (two_pi * f2 * t).sin()
            })
            .collect()
    }

    fn hf_shift_db(input: &[f32], output: &[f32], sr: u32) -> f64 {
        let ratio = |x: &[f32]| mag_at(x, 8000.0, sr) / mag_at(x, 1000.0, sr).max(1e-12);
        20.0 * (ratio(output) / ratio(input)).log10()
    }

    #[test]
    fn deess_multiband_keeps_steady_highs() {
        let sr = 48_000u32;
        let x = two_tone(sr, 1.0, 1000.0, 8000.0, 0.3);
        let out = deess_multiband(&x, sr, 4000.0, 6.0, 4.0, 3);
        let db = hf_shift_db(&x, &out, sr);
        assert!(db > -6.0, "steady 8 kHz vs 1 kHz shifted {db:.1} dB");
        assert!(
            mag_at(&out, 1000.0, sr) > mag_at(&x, 1000.0, sr) * 0.8,
            "1 kHz tone not preserved"
        );
    }

    #[test]
    fn deess_multiband_negative_threshold_does_not_crush_hf() {
        // The single-band deesser default (−24) is an HF/broadband ratio. In
        // multiband mode it would compress every frame; it is floored at 0 dB.
        let sr = 48_000u32;
        let x = two_tone(sr, 1.0, 1000.0, 8000.0, 0.3);
        let out = deess_multiband(&x, sr, 4000.0, -24.0, 4.0, 3);
        let db = hf_shift_db(&x, &out, sr);
        assert!(db > -12.0, "negative threshold crushed HF by {db:.1} dB");
    }
}
