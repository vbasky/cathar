//! Spectral-subtraction / Wiener denoiser.

use crate::util::hann_window;
use crate::{AudioData, Error};
use realfft::RealFftPlanner;

/// A denoising strategy: turn noisy [`AudioData`] into a cleaner copy.
pub trait Denoiser {
    /// Denoise every channel of `input`, returning a new [`AudioData`].
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error>;
}

// ── NoisePrint ───────────────────────────────────────────────────────────────

/// Pre-computed noise profile from a silence segment.
/// Feed into `SpectralDenoiser::with_noise_print` instead of auto-detection.
#[derive(Debug, Clone)]
pub struct NoisePrint {
    /// FFT size the spectrum was measured at (must match the denoiser's).
    pub fft_size: usize,
    /// Per-bin noise magnitude spectrum (`fft_size / 2 + 1` bins).
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

/// STFT spectral-subtraction / Wiener denoiser (see [`Denoiser`]).
pub struct SpectralDenoiser {
    /// Analysis/synthesis FFT size in samples.
    pub fft_size: usize,
    /// Hop between successive frames in samples (overlap = `fft_size - hop_size`).
    pub hop_size: usize,
    /// Over-subtraction factor: how many times the noise estimate to subtract
    /// (1.0 = gentle, 6.0 = aggressive).
    pub alpha: f32,
    /// Spectral floor as a fraction of the input magnitude, limiting "musical
    /// noise" artifacts (0.0–0.1).
    pub beta: f32,
    /// Fraction of the quietest frames taken as the noise estimate when no
    /// `noise_print` is supplied (minimum-statistics).
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
    /// Build a denoiser driven by a pre-learned [`NoisePrint`] (FFT size and hop
    /// follow the print; minimum-statistics auto-detection is disabled).
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

    /// Denoise **phase-coherently** across channels: the per-bin suppression gain
    /// is computed once from the mid signal (the channel average) and applied
    /// identically to every channel. Independent per-channel denoising gates bins
    /// differently in L and R, which makes the residual noise floor pan; one
    /// shared gain keeps the stereo image stable while each channel keeps its own
    /// phase. Equivalent to [`denoise`](Self::denoise) for mono input.
    pub fn denoise_coherent(&self, input: &AudioData) -> Result<AudioData, Error> {
        let n_ch = input.channels.len();
        if n_ch <= 1 {
            return self.denoise(input);
        }
        let len = input.channels.iter().map(|c| c.len()).min().unwrap_or(0);
        if len < self.fft_size {
            return Err(Error::TooShort);
        }
        let mid: Vec<f32> = (0..len)
            .map(|i| input.channels.iter().map(|c| c[i]).sum::<f32>() / n_ch as f32)
            .collect();
        let noise_spectrum = self.noise_spectrum(&mid)?;

        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(self.fft_size);
        let c2r = planner.plan_fft_inverse(self.fft_size);
        let hann = hann_window(self.fft_size);
        let scale = 1.0f32 / self.fft_size as f32;
        let frames = len / self.hop_size;
        let n_bins = self.fft_size / 2 + 1;

        let mut outputs = vec![vec![0.0f32; len + self.fft_size]; n_ch];
        let mut mid_in = r2c.make_input_vec();
        let mut mid_out = r2c.make_output_vec();
        let mut ch_in = r2c.make_input_vec();
        let mut ch_out = r2c.make_output_vec();
        let mut inv = c2r.make_output_vec();
        let mut gains = vec![1.0f32; n_bins];

        for fi in 0..frames {
            let offset = fi * self.hop_size;
            if offset + self.fft_size > len {
                break;
            }
            // One shared gain mask from the mid signal.
            for i in 0..self.fft_size {
                mid_in[i] = mid[offset + i] * hann[i];
            }
            r2c.process(&mut mid_in, &mut mid_out).unwrap();
            for (k, ns) in noise_spectrum.iter().enumerate() {
                let mag = (mid_out[k].re * mid_out[k].re + mid_out[k].im * mid_out[k].im).sqrt();
                let clean = (mag - self.alpha * ns).max(self.beta * mag).max(0.0);
                gains[k] = if mag > 1e-10 { clean / mag } else { 0.0 };
            }
            // Apply it to every channel, preserving each channel's phase.
            for (ch, out) in outputs.iter_mut().enumerate() {
                for i in 0..self.fft_size {
                    ch_in[i] = input.channels[ch][offset + i] * hann[i];
                }
                r2c.process(&mut ch_in, &mut ch_out).unwrap();
                for (c, &g) in ch_out.iter_mut().zip(gains.iter()) {
                    c.re *= g;
                    c.im *= g;
                }
                c2r.process(&mut ch_out, &mut inv).unwrap();
                for i in 0..self.fft_size {
                    out[offset + i] += inv[i] * hann[i] * scale;
                }
            }
        }
        let channels = outputs
            .into_iter()
            .map(|mut o| {
                o.truncate(len);
                o
            })
            .collect();
        Ok(AudioData { sample_rate: input.sample_rate, channels })
    }
}

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
