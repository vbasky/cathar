//! Audio restoration toolbox — denoise, de-hum, de-click, de-clip, normalise.
//!
//! Default denoising uses **spectral subtraction** (pure Rust, zero weights).
//! Enable the `ml` feature for candle-based neural denoising (Demucs, DNS Challenge).
//!
//! # Quick start
//!
//! ```rust
//! use cathar::{Denoiser, SpectralDenoiser, generate_wave};
//!
//! let audio = generate_wave(44100, 440.0, 1.0, 0.2);
//! let denoiser = SpectralDenoiser::default();
//! let clean = denoiser.denoise(&audio)?;
//! assert_eq!(clean.channels[0].len(), audio.channels[0].len());
//! # Ok::<(), cathar::Error>(())
//! ```

use hound::{WavSpec, WavWriter};
use realfft::RealFftPlanner;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use thiserror::Error;

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("audio write error: {0}")]
    Hound(#[from] hound::Error),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("no audio track found")]
    NoAudioTrack,
    #[error("unsupported format")]
    UnsupportedFormat,
    #[error("signal too short")]
    TooShort,
    #[error("FFT error: {0}")]
    Fft(String),
    #[error("noise print FFT size mismatch")]
    NoisePrintMismatch,
}

// ── AudioData ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AudioData {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl AudioData {
    pub fn from_file(path: &str) -> Result<Self, Error> {
        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let mut format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| Error::Decode(format!("{e}")))?;

        // Pull the first audio track's parameters and build its decoder. The
        // immutable borrow of `format` is scoped to this block so the decode
        // loop below can borrow it mutably.
        let (track_id, sample_rate, num_channels, mut decoder) = {
            let track = format.default_track(TrackType::Audio).ok_or(Error::NoAudioTrack)?;
            let Some(CodecParameters::Audio(params)) = &track.codec_params else {
                return Err(Error::NoAudioTrack);
            };
            let sample_rate = params.sample_rate.ok_or(Error::UnsupportedFormat)?;
            let num_channels = params.channels.as_ref().ok_or(Error::UnsupportedFormat)?.count();
            let decoder = symphonia::default::get_codecs()
                .make_audio_decoder(params, &AudioDecoderOptions::default())
                .map_err(|e| Error::Decode(format!("{e}")))?;
            (track.id, sample_rate, num_channels, decoder)
        };

        let mut channels = vec![Vec::new(); num_channels];
        let mut interleaved: Vec<f32> = Vec::new();
        while let Some(packet) = format.next_packet().map_err(|e| Error::Decode(format!("{e}")))? {
            if packet.track_id != track_id {
                continue;
            }
            let decoded = decoder.decode(&packet).map_err(|e| Error::Decode(format!("{e}")))?;
            interleaved.clear();
            decoded.copy_to_vec_interleaved(&mut interleaved);
            for (i, sample) in interleaved.iter().enumerate() {
                channels[i % num_channels].push(*sample);
            }
        }
        Ok(Self { sample_rate, channels })
    }

    pub fn to_file(&self, path: &str) -> Result<(), Error> {
        let spec = WavSpec {
            channels: self.channels.len() as u16,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = WavWriter::create(path, spec)?;
        let len = self.channels.first().map_or(0, |c| c.len());
        for i in 0..len {
            for ch in &self.channels {
                writer.write_sample(ch[i])?;
            }
        }
        writer.finalize()?;
        Ok(())
    }

    /// Map a single-channel operation across all channels.
    pub fn map_channels<F: Fn(&[f32]) -> Vec<f32>>(&self, f: F) -> Self {
        Self {
            sample_rate: self.sample_rate,
            channels: self.channels.iter().map(|c| f(c)).collect(),
        }
    }
}

// ── Denoiser trait ───────────────────────────────────────────────────────────

pub trait Denoiser {
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error>;
}

// ── NoisePrint ───────────────────────────────────────────────────────────────

/// Pre-computed noise profile from a silence segment.
/// Feed into `SpectralDenoiser::with_noise_print` instead of auto-detection.
#[derive(Debug, Clone)]
pub struct NoisePrint {
    pub fft_size: usize,
    pub spectrum: Vec<f32>,
}

/// Learn a noise profile from an audio segment (should be silence/noise-only).
pub fn learn_noise_print(audio: &AudioData) -> Result<NoisePrint, Error> {
    let fft_size = 2048;
    let signal = &audio.channels[0];
    if signal.len() < fft_size {
        return Err(Error::TooShort);
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let hann = hann_window(fft_size);
    let hop = fft_size / 4;
    let frames = signal.len() / hop;
    let n_bins = fft_size / 2 + 1;
    let mut spectrum = vec![0.0f32; n_bins];
    let mut count = 0usize;
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop;
        if offset + fft_size > signal.len() {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();
        for k in 0..n_bins {
            spectrum[k] += (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
        }
        count += 1;
    }
    if count == 0 {
        return Err(Error::TooShort);
    }
    for v in &mut spectrum {
        *v /= count as f32;
    }
    Ok(NoisePrint { fft_size, spectrum })
}

// ── SpectralDenoiser ─────────────────────────────────────────────────────────

pub struct SpectralDenoiser {
    pub fft_size: usize,
    pub hop_size: usize,
    pub alpha: f32,
    pub beta: f32,
    pub noise_frame_ratio: f32,
    /// Optional pre-computed noise print. Takes priority over auto-detection.
    pub noise_print: Option<NoisePrint>,
}

impl Default for SpectralDenoiser {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            hop_size: 512,
            alpha: 3.0,
            beta: 0.01,
            noise_frame_ratio: 0.15,
            noise_print: None,
        }
    }
}

impl SpectralDenoiser {
    pub fn with_noise_print(noise_print: NoisePrint, alpha: f32, beta: f32) -> Self {
        Self {
            fft_size: noise_print.fft_size,
            hop_size: noise_print.fft_size / 4,
            alpha,
            beta,
            noise_frame_ratio: 0.0,
            noise_print: Some(noise_print),
        }
    }
}

impl Denoiser for SpectralDenoiser {
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error> {
        let mut output_channels = Vec::with_capacity(input.channels.len());
        for channel in &input.channels {
            output_channels.push(self.denoise_channel(channel)?);
        }
        Ok(AudioData { sample_rate: input.sample_rate, channels: output_channels })
    }
}

impl SpectralDenoiser {
    fn noise_spectrum(&self, signal: &[f32]) -> Result<Vec<f32>, Error> {
        if let Some(ref np) = self.noise_print {
            if np.fft_size != self.fft_size {
                return Err(Error::NoisePrintMismatch);
            }
            return Ok(np.spectrum.clone());
        }
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(self.fft_size);
        let hann = hann_window(self.fft_size);
        let noise_frames = signal.len() / self.hop_size;
        let n_bins = self.fft_size / 2 + 1;
        let mut spectrum = vec![f32::MAX; n_bins];
        let mut in_buf = r2c.make_input_vec();
        let mut out_buf = r2c.make_output_vec();
        for fi in 0..noise_frames {
            let offset = fi * self.hop_size;
            if offset + self.fft_size > signal.len() {
                break;
            }
            for i in 0..self.fft_size {
                in_buf[i] = signal[offset + i] * hann[i];
            }
            r2c.process(&mut in_buf, &mut out_buf).unwrap();
            for (k, item) in spectrum.iter_mut().enumerate() {
                let mag = (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
                *item = (*item).min(mag);
            }
        }
        for v in &mut spectrum {
            *v *= 2.0;
        }
        Ok(spectrum)
    }

    fn denoise_channel(&self, signal: &[f32]) -> Result<Vec<f32>, Error> {
        if signal.len() < self.fft_size {
            return Err(Error::TooShort);
        }
        let n = signal.len();
        let noise_spectrum = self.noise_spectrum(signal)?;
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(self.fft_size);
        let c2r = planner.plan_fft_inverse(self.fft_size);
        let hann = hann_window(self.fft_size);
        let scale = 1.0f32 / (self.fft_size as f32);
        let frames = n / self.hop_size;
        let mut output = vec![0.0f32; n + self.fft_size];
        let mut in_buf = r2c.make_input_vec();
        let mut out_buf = r2c.make_output_vec();

        for fi in 0..frames {
            let offset = fi * self.hop_size;
            if offset + self.fft_size > n {
                break;
            }
            for i in 0..self.fft_size {
                in_buf[i] = signal[offset + i] * hann[i];
            }
            r2c.process(&mut in_buf, &mut out_buf).unwrap();
            for (k, ns) in noise_spectrum.iter().enumerate() {
                let mag = (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
                let phase = out_buf[k].im.atan2(out_buf[k].re);
                let clean_mag = (mag - self.alpha * ns).max(self.beta * mag).max(0.0);
                if k == 0 || k == noise_spectrum.len() - 1 {
                    out_buf[k].re = clean_mag;
                    out_buf[k].im = 0.0;
                } else {
                    out_buf[k].re = clean_mag * phase.cos();
                    out_buf[k].im = clean_mag * phase.sin();
                }
            }
            c2r.process(&mut out_buf, &mut in_buf).unwrap();
            for i in 0..self.fft_size {
                output[offset + i] += in_buf[i] * hann[i] * scale;
            }
        }
        output.truncate(n);
        Ok(output)
    }
}

// ── De-hum ───────────────────────────────────────────────────────────────────

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

// ── De-clip ──────────────────────────────────────────────────────────────────

/// Detect and reconstruct clipped samples.
///
/// Clipping is detected as consecutive samples at or above `threshold` (e.g. 0.95).
/// Clipped segments are reconstructed by cubic interpolation.
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
            let end = (i).min(n - 1);
            // Extend detection a few samples to catch the rounded shoulders
            let clip_start = start.saturating_sub(4);
            let clip_end = (end + 4).min(n - 1);
            if clip_end > clip_start + 4 {
                cubic_interpolate(&mut output, clip_start, clip_end);
            }
        }
        i += 1;
    }
    output
}

// ── Normalise ────────────────────────────────────────────────────────────────

/// Scale to target peak level in dBFS (0 dBFS = ±1.0, -3 dBFS = ~±0.707).
pub fn normalize_peak(signal: &[f32], target_dbfs: f32) -> Vec<f32> {
    let peak = signal.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
    if peak < 1e-10 {
        return signal.to_vec();
    }
    let target_linear = 10.0f32.powf(target_dbfs / 20.0);
    let gain = target_linear / peak;
    signal.iter().map(|s| s * gain).collect()
}

/// Scale to target integrated loudness (EBU R128 approximation via RMS).
/// Target in LUFS: -23 = broadcast, -16 = podcast, -14 = streaming.
pub fn normalize_loudness(signal: &[f32], target_lufs: f32) -> Vec<f32> {
    let rms: f32 = (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt();
    if rms < 1e-10 {
        return signal.to_vec();
    }
    let target_linear = 10.0f32.powf(target_lufs / 20.0);
    let gain = target_linear / rms;
    signal.iter().map(|s| s * gain).collect()
}

// ── De-reverb ───────────────────────────────────────────────────────────────

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

// ── Voice isolation ─────────────────────────────────────────────────────────

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

// ── Bandwidth extension ─────────────────────────────────────────────────────

/// Restore high-frequency content lost to compression or low sample rates.
///
/// Uses spectral band replication: the spectral envelope from the upper octave
/// of the source signal is transposed into the missing high band, shaped, and
/// mixed back. No ML — pure DSP, zero weights.
pub fn bandwidth_extend(signal: &[f32], sample_rate: u32, target_rate: u32) -> Vec<f32> {
    if target_rate <= sample_rate {
        return signal.to_vec();
    }

    // ── 1. Resample to target rate (windowed sinc) ──
    let ratio = target_rate as f64 / sample_rate as f64;
    let out_len = (signal.len() as f64 * ratio).round() as usize;
    let mut resampled = vec![0.0f32; out_len];
    let sinc_width = 32;

    for (i, r) in resampled.iter_mut().enumerate() {
        let src_pos = i as f64 / ratio;
        let src_idx = src_pos as isize;
        let frac = src_pos - src_idx as f64;
        let mut sum = 0.0f64;
        let mut norm = 0.0f64;
        for j in -sinc_width..=sinc_width {
            let idx = src_idx + j;
            if idx >= 0 && (idx as usize) < signal.len() {
                let x = (j as f64 - frac) * std::f64::consts::PI;
                let sinc = if x.abs() < 1e-8 { 1.0 } else { x.sin() / x };
                let w = (j as f64 - frac) * std::f64::consts::PI / sinc_width as f64;
                let win = if w.abs() < 1e-8 { 1.0 } else { w.sin() / w };
                let kernel = sinc * win;
                sum += signal[idx as usize] as f64 * kernel;
                norm += kernel;
            }
        }
        *r = (sum / norm.max(1e-10)) as f32;
    }

    // ── 2. SBR: replicate low-band spectrum shape into high band ──
    let fft_size = 4096;
    let hop_size = fft_size / 4;
    let n = resampled.len();
    if n < fft_size {
        return resampled;
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

// ── Wiener filter denoiser ───────────────────────────────────────────────────

/// Wiener-filter denoiser — statistically optimal, better transients.
pub fn wiener_denoise(
    signal: &[f32],
    noise_print: &NoisePrint,
    alpha: f32,
) -> Result<Vec<f32>, Error> {
    let fft_size = noise_print.fft_size;
    let hop_size = fft_size / 4;
    let n = signal.len();
    if n < fft_size {
        return Err(Error::TooShort);
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / (fft_size as f32);
    let frames = n / hop_size;
    let n_bins = fft_size / 2 + 1;

    // Smooth the noise spectrum
    let noise: Vec<f32> = noise_print.spectrum.iter().map(|&v| v * alpha).collect();

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

        for k in 0..n_bins {
            let signal_power = out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im;
            let noise_power = noise[k] * noise[k];
            // Wiener gain: signal / (signal + noise)
            let gain = signal_power / (signal_power + noise_power).max(1e-10);
            out_buf[k].re *= gain;
            out_buf[k].im *= gain;
        }
        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
        }
    }
    output.truncate(n);
    Ok(output)
}

// ── Utilities ────────────────────────────────────────────────────────────────

fn hann_window(size: usize) -> Vec<f32> {
    let n = size as f32 - 1.0;
    (0..size)
        .map(|i| {
            let x = i as f32 / n;
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * x).cos()
        })
        .collect()
}

pub fn generate_wave(
    sample_rate: u32,
    frequency: f32,
    duration_secs: f32,
    noise_level: f32,
) -> AudioData {
    let num_samples = (sample_rate as f32 * duration_secs) as usize;
    let mut rng: u64 = 42;
    let samples: Vec<f32> = (0..num_samples)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let signal = (2.0 * std::f32::consts::PI * frequency * t).sin() * 0.5;
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let noise = ((rng as f32) / (u64::MAX as f32) - 0.5) * noise_level;
            signal + noise
        })
        .collect();
    AudioData { sample_rate, channels: vec![samples] }
}

pub fn variance(samples: &[f32]) -> f32 {
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    samples.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / samples.len() as f32
}

impl From<realfft::FftError> for Error {
    fn from(e: realfft::FftError) -> Self {
        Error::Fft(format!("{e:?}"))
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectral_denoiser_preserves_signal_shape() {
        let audio = generate_wave(44100, 440.0, 2.0, 0.15);
        let denoiser = SpectralDenoiser::default();
        let clean = denoiser.denoise(&audio).unwrap();
        assert_eq!(clean.sample_rate, audio.sample_rate);
        assert_eq!(clean.channels.len(), audio.channels.len());
        assert_eq!(clean.channels[0].len(), audio.channels[0].len());
    }

    #[test]
    fn spectral_denoiser_reduces_noise_power() {
        let audio = generate_wave(44100, 440.0, 3.0, 0.3);
        let denoiser = SpectralDenoiser { alpha: 4.0, beta: 0.02, ..Default::default() };
        let clean = denoiser.denoise(&audio).unwrap();
        let noisy_power = variance(&audio.channels[0]);
        let clean_power = variance(&clean.channels[0]);
        assert!(clean_power < noisy_power, "clean {clean_power:.4} < noisy {noisy_power:.4}");
    }

    #[test]
    fn noise_print_denoise() {
        // Generate a known noise profile, then denoise with it
        let noise = generate_wave(44100, 440.0, 2.0, 0.3); // tone + noise
        let np = learn_noise_print(&noise).unwrap();
        let denoiser = SpectralDenoiser::with_noise_print(np, 3.0, 0.01);
        let clean = denoiser.denoise(&noise).unwrap();
        assert!(variance(&clean.channels[0]) < variance(&noise.channels[0]));
    }

    #[test]
    fn generate_wave_bounds() {
        let audio = generate_wave(48000, 1000.0, 2.5, 0.0);
        assert_eq!(audio.sample_rate, 48000);
        assert_eq!(audio.channels.len(), 1);
        assert_eq!(audio.channels[0].len(), 120_000);
        for s in &audio.channels[0] {
            assert!(*s >= -0.5 && *s <= 0.5);
        }
    }

    #[test]
    fn generate_wave_with_noise() {
        let audio = generate_wave(44100, 440.0, 0.5, 0.3);
        let has_outlier = audio.channels[0].iter().any(|s| *s - 0.5 > 1e-6 || *s + 0.5 < -1e-6);
        assert!(has_outlier);
    }

    #[test]
    fn dehum_reduces_hum() {
        // Generate 60 Hz hum + white noise
        let sr = 48000;
        let n = sr as usize * 2; // 2 seconds
        let mut signal: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 60.0 * i as f32 / sr as f32).sin() * 0.5)
            .collect();
        // Add a little 1 kHz tone as the "wanted" signal
        for (i, s) in signal.iter_mut().enumerate().take(n) {
            *s += (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin() * 0.3;
        }
        let cleaned = dehum(&signal, sr, 60.0, 5);
        // Power should reduce since 60 Hz hum is removed
        assert!(variance(&cleaned) < variance(&signal) * 0.9);
    }

    #[test]
    fn declick_detects_spike() {
        let mut signal = vec![0.01f32; 1000];
        signal[500] = 10.0; // big click
        let cleaned = declick(&signal, 5.0, 32);
        assert!(cleaned[500].abs() < 5.0, "click should be attenuated");
    }

    #[test]
    fn declick_handles_short_signal() {
        // Regression: a signal shorter than the window used to underflow
        // `n - half` and panic. It should now pass through untouched.
        for len in [0usize, 1, 5, 31, 32, 64] {
            let signal = vec![0.2f32; len];
            let out = declick(&signal, 5.0, 64);
            assert_eq!(out, signal, "short signal (len {len}) should be unchanged");
        }
    }

    #[test]
    fn declip_reconstructs() {
        let mut signal = vec![0.1f32; 200];
        // Create a flat-topped clip
        for s in signal.iter_mut().skip(90).take(20) {
            *s = 0.98;
        }
        let cleaned = declip(&signal, 0.95);
        // Middle sample should be interpolated, not exactly 0.98
        let mid = cleaned[99];
        assert!(mid < 0.97, "clipped sample should be reconstructed, got {mid}");
    }

    #[test]
    fn normalize_peak_target() {
        let signal = vec![0.5f32, -0.5, 0.25, -0.25, 0.1];
        let normalized = normalize_peak(&signal, -3.0); // target -3 dBFS ≈ 0.707
        let peak = normalized.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        assert!((peak - 0.707).abs() < 0.01, "peak should be ~0.707, got {peak}");
    }

    #[test]
    fn normalize_loudness_target() {
        let signal: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.001).sin() * 0.1).collect();
        let normalized = normalize_loudness(&signal, -23.0);
        let rms = (normalized.iter().map(|s| s * s).sum::<f32>() / normalized.len() as f32).sqrt();
        // -23 LUFS ≈ 0.07 RMS... actually -23 dBFS = 10^(-23/20) ≈ 0.0708
        assert!((rms - 0.0708).abs() < 0.01, "RMS should be ~0.071, got {rms}");
    }

    #[test]
    fn wiener_reduces_noise() {
        let noisy = generate_wave(44100, 440.0, 2.0, 0.2);
        let np = learn_noise_print(&noisy).unwrap();
        let clean = wiener_denoise(&noisy.channels[0], &np, 2.0).unwrap();
        assert!(variance(&clean) < variance(&noisy.channels[0]) * 0.9);
    }

    #[test]
    fn map_channels_applies_to_all() {
        let audio = generate_wave(44100, 440.0, 1.0, 0.1);
        let result = audio.map_channels(|c| c.iter().map(|s| s * 2.0).collect());
        assert_eq!(result.channels[0][42], audio.channels[0][42] * 2.0);
    }
}
