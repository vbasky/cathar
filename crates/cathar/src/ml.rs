//! Neural spectral-gain denoiser (candle) — behind the `ml` feature.
//!
//! A DNS-Challenge / DeepFilterNet-style suppression network. For every STFT
//! frame a small recurrent network predicts a per-frequency-bin **gain mask** in
//! `(0, 1)` from the log-magnitude spectrum; the mask scales the complex
//! spectrum (phase preserved) and the signal is rebuilt by overlap-add. This
//! mirrors the pure-DSP [`SpectralDenoiser`](crate::SpectralDenoiser) exactly,
//! except the per-bin gain is *learned* rather than derived from a noise
//! estimate, so it can suppress non-stationary noise a noise print can't model.
//!
//! No black box: the architecture is the open, inspectable code below, weights
//! load from an open `.safetensors` checkpoint, and inference is deterministic —
//! same input, same weights, same samples out.
//!
//! ```no_run
//! use cathar::{AudioData, Denoiser, NeuralDenoiser, NeuralConfig};
//!
//! let audio = AudioData::from_file("noisy.wav")?;
//! // Load a trained checkpoint…
//! let nn = NeuralDenoiser::from_safetensors("denoiser.safetensors", NeuralConfig::default())?;
//! let clean = nn.denoise(&audio)?;
//! clean.to_file("clean.wav")?;
//! # Ok::<(), cathar::Error>(())
//! ```
//!
//! Without a checkpoint, [`NeuralDenoiser::new`] builds a deterministic
//! passthrough-initialised model (every gain ≈ 1) — a safe no-op that proves the
//! inference path end-to-end and gives training a known starting point.

use std::collections::HashMap;

use candle_core::{DType, Device, Tensor};
use candle_nn::{GRU, GRUConfig, Linear, Module, RNN, VarBuilder};
use realfft::RealFftPlanner;

use crate::util::hann_window;
use crate::{AudioData, Denoiser, Error};

/// Geometry and capacity of a [`NeuralDenoiser`]. The STFT size fixes the input
/// width (`fft_size / 2 + 1` bins), so it must match the checkpoint the weights
/// were trained at; `hidden` is the GRU width.
#[derive(Debug, Clone, Copy)]
pub struct NeuralConfig {
    /// Analysis/synthesis FFT size in samples.
    pub fft_size: usize,
    /// Hop between successive frames in samples (overlap = `fft_size - hop_size`).
    pub hop_size: usize,
    /// GRU hidden width.
    pub hidden: usize,
}

impl Default for NeuralConfig {
    /// 512-point STFT (257 bins) at 75 % overlap, 256-wide GRU — a light,
    /// general-purpose default suitable for 16–48 kHz speech.
    fn default() -> Self {
        Self { fft_size: 512, hop_size: 128, hidden: 256 }
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

/// enc → GRU → dec gain predictor. Linear/GRU parameter names match PyTorch's
/// (`weight`/`bias`, `weight_ih_l0`, …) so checkpoints exported from a standard
/// training script load without renaming.
struct Model {
    enc: Linear,
    gru: GRU,
    dec: Linear,
}

impl Model {
    fn build(vb: VarBuilder, n_bins: usize, hidden: usize) -> Result<Self, Error> {
        let enc = candle_nn::linear(n_bins, hidden, vb.pp("enc"))?;
        let gru = candle_nn::gru(hidden, hidden, GRUConfig::default(), vb.pp("gru"))?;
        let dec = candle_nn::linear(hidden, n_bins, vb.pp("dec"))?;
        Ok(Self { enc, gru, dec })
    }
}

// ── NeuralDenoiser ───────────────────────────────────────────────────────────

/// Bundled pretrained checkpoint (synthetic noisy-tone pairs; retrain on
/// DNS-Challenge for speech). 2 MB.
const PRETRAINED_CHECKPOINT: &[u8] = include_bytes!("denoiser.safetensors");

/// Learned spectral-gain denoiser — a candle GRU predicts a per-bin suppression
/// mask (see the module-level documentation for the architecture). Implements
/// the [`Denoiser`] trait, so it is a drop-in alternative to `SpectralDenoiser`.
pub struct NeuralDenoiser {
    model: Model,
    fft_size: usize,
    hop_size: usize,
    n_bins: usize,
    device: Device,
}

impl NeuralDenoiser {
    /// Build a deterministic **passthrough** model with the default config: the
    /// encoder, GRU and decoder weights are zero and the decoder bias is a large
    /// positive constant, so every predicted gain is ≈ 1 and the audio passes
    /// through essentially untouched. Useful as a safe default and as a known
    /// initialisation for training; for real denoising load trained weights with
    /// [`from_safetensors`](Self::from_safetensors).
    pub fn new() -> Result<Self, Error> {
        Self::with_config(NeuralConfig::default())
    }

    /// Like [`new`](Self::new) but with an explicit [`NeuralConfig`].
    pub fn with_config(cfg: NeuralConfig) -> Result<Self, Error> {
        let device = Device::Cpu;
        let n_bins = cfg.fft_size / 2 + 1;
        let vb = VarBuilder::from_tensors(passthrough_tensors(&cfg, &device)?, DType::F32, &device);
        let model = Model::build(vb, n_bins, cfg.hidden)?;
        Ok(Self { model, fft_size: cfg.fft_size, hop_size: cfg.hop_size, n_bins, device })
    }

    /// Load the **bundled pretrained checkpoint**. A small model trained on
    /// synthetic noisy tones — it suppresses broadband noise but is not tuned for
    /// speech. For production use on speech, load a checkpoint trained on the
    /// [DNS-Challenge](https://github.com/microsoft/DNS-Challenge) dataset with
    /// [`from_safetensors`](Self::from_safetensors).
    ///
    /// The bundled weights are compiled into the binary, so this works offline
    /// with no download.
    pub fn pretrained() -> Result<Self, Error> {
        Self::pretrained_with_config(NeuralConfig::default())
    }

    /// Like [`pretrained`](Self::pretrained) but with an explicit [`NeuralConfig`].
    pub fn pretrained_with_config(cfg: NeuralConfig) -> Result<Self, Error> {
        let device = Device::Cpu;
        let n_bins = cfg.fft_size / 2 + 1;
        let vb = candle_nn::VarBuilder::from_buffered_safetensors(
            PRETRAINED_CHECKPOINT.to_vec(),
            DType::F32,
            &device,
        )?;
        let model = Model::build(vb, n_bins, cfg.hidden)?;
        Ok(Self { model, fft_size: cfg.fft_size, hop_size: cfg.hop_size, n_bins, device })
    }

    /// Load weights from an open `.safetensors` checkpoint. `cfg` must match the
    /// geometry the weights were trained at (mismatched shapes surface as an
    /// [`Error::Ml`]).
    pub fn from_safetensors<P: AsRef<std::path::Path>>(
        path: P,
        cfg: NeuralConfig,
    ) -> Result<Self, Error> {
        let device = Device::Cpu;
        let n_bins = cfg.fft_size / 2 + 1;
        // Safe: the file is mapped read-only and never mutated for the model's life.
        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[path.as_ref()], DType::F32, &device)? };
        let model = Model::build(vb, n_bins, cfg.hidden)?;
        Ok(Self { model, fft_size: cfg.fft_size, hop_size: cfg.hop_size, n_bins, device })
    }

    /// Run the network over the per-frame magnitude spectra, returning one gain
    /// mask (`n_bins` values in `(0, 1)`) per frame.
    fn predict_gains(&self, mags: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, Error> {
        let frames = mags.len();
        let n_bins = self.n_bins;
        // Log-magnitude features — the standard compressed input for suppression
        // nets (a wide dynamic range squashed into a near-linear scale).
        let mut feat = Vec::with_capacity(frames * n_bins);
        for frame in mags {
            for &m in frame {
                feat.push((m + 1e-6).ln());
            }
        }
        let x = Tensor::from_vec(feat, (1, frames, n_bins), &self.device)?;
        let x = self.model.enc.forward(&x)?.relu()?; // [1, frames, hidden]
        let states = self.model.gru.seq(&x)?; // one GRUState per frame
        let hs: Vec<Tensor> = states.iter().map(|s| s.h.clone()).collect();
        let h = Tensor::stack(&hs, 1)?; // [1, frames, hidden]
        let g = candle_nn::ops::sigmoid(&self.model.dec.forward(&h)?)?; // [1, frames, n_bins]
        Ok(g.squeeze(0)?.to_vec2::<f32>()?)
    }

    fn denoise_channel(&self, signal: &[f32]) -> Result<Vec<f32>, Error> {
        let n = signal.len();
        if n < self.fft_size {
            return Err(Error::TooShort);
        }
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(self.fft_size);
        let c2r = planner.plan_fft_inverse(self.fft_size);
        let hann = hann_window(self.fft_size);
        let n_bins = self.n_bins;
        let mut in_buf = r2c.make_input_vec();
        let mut out_buf = r2c.make_output_vec();

        // Analysis: one windowed FFT per hop; keep the complex spectra (for phase)
        // and their magnitudes (for the network).
        let mut specs = Vec::new();
        let mut mags = Vec::new();
        let mut fi = 0usize;
        loop {
            let offset = fi * self.hop_size;
            if offset + self.fft_size > n {
                break;
            }
            for i in 0..self.fft_size {
                in_buf[i] = signal[offset + i] * hann[i];
            }
            r2c.process(&mut in_buf, &mut out_buf)?;
            mags.push(out_buf.iter().map(|c| (c.re * c.re + c.im * c.im).sqrt()).collect());
            specs.push(out_buf.clone());
            fi += 1;
        }

        let gains = self.predict_gains(&mags)?;

        // Synthesis: scale each frame's complex spectrum by its gain mask (phase
        // untouched), overlap-add, and divide by the accumulated window energy so
        // a unity mask reconstructs the signal exactly (no COLA level offset, no
        // edge attenuation), for any fft_size/hop_size.
        let scale = 1.0f32 / self.fft_size as f32;
        let mut output = vec![0.0f32; n + self.fft_size];
        let mut norm = vec![0.0f32; n + self.fft_size];
        for (fi, spec) in specs.iter_mut().enumerate() {
            let g = &gains[fi];
            for k in 0..n_bins {
                spec[k].re *= g[k];
                spec[k].im *= g[k];
            }
            // DC and Nyquist must stay real for the inverse real-FFT.
            spec[0].im = 0.0;
            spec[n_bins - 1].im = 0.0;
            c2r.process(spec, &mut in_buf)?;
            let offset = fi * self.hop_size;
            for i in 0..self.fft_size {
                output[offset + i] += in_buf[i] * hann[i] * scale;
                norm[offset + i] += hann[i] * hann[i];
            }
        }
        for (o, w) in output.iter_mut().zip(&norm) {
            if *w > 1e-8 {
                *o /= *w;
            }
        }
        output.truncate(n);
        Ok(output)
    }
}

impl Denoiser for NeuralDenoiser {
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error> {
        let mut channels = Vec::with_capacity(input.channels.len());
        for ch in &input.channels {
            channels.push(self.denoise_channel(ch)?);
        }
        Ok(AudioData { sample_rate: input.sample_rate, channels })
    }
}

/// Tensors for a passthrough-initialised model: all weights zero, GRU biases
/// zero, and a large positive decoder bias so `sigmoid(dec) ≈ 1`. Names match
/// [`Model::build`]'s `VarBuilder` lookups.
fn passthrough_tensors(cfg: &NeuralConfig, dev: &Device) -> Result<HashMap<String, Tensor>, Error> {
    let n_bins = cfg.fft_size / 2 + 1;
    let h = cfg.hidden;
    let mut m = HashMap::new();
    m.insert("enc.weight".into(), Tensor::zeros((h, n_bins), DType::F32, dev)?);
    m.insert("enc.bias".into(), Tensor::zeros(h, DType::F32, dev)?);
    m.insert("gru.weight_ih_l0".into(), Tensor::zeros((3 * h, h), DType::F32, dev)?);
    m.insert("gru.weight_hh_l0".into(), Tensor::zeros((3 * h, h), DType::F32, dev)?);
    m.insert("gru.bias_ih_l0".into(), Tensor::zeros(3 * h, DType::F32, dev)?);
    m.insert("gru.bias_hh_l0".into(), Tensor::zeros(3 * h, DType::F32, dev)?);
    m.insert("dec.weight".into(), Tensor::zeros((n_bins, h), DType::F32, dev)?);
    // sigmoid(6) ≈ 0.9975 → near-unity gain, audio passes through.
    m.insert("dec.bias".into(), Tensor::full(6.0f32, n_bins, dev)?);
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generate_wave, variance};

    /// The passthrough model runs end-to-end and preserves shape and rate.
    #[test]
    fn neural_denoiser_preserves_shape() {
        let audio = generate_wave(48_000, 440.0, 1.0, 0.1);
        let nn = NeuralDenoiser::new().unwrap();
        let out = nn.denoise(&audio).unwrap();
        assert_eq!(out.sample_rate, audio.sample_rate);
        assert_eq!(out.channels.len(), audio.channels.len());
        assert_eq!(out.channels[0].len(), audio.channels[0].len());
        assert!(out.channels[0].iter().all(|s| s.is_finite()));
    }

    /// Inference is deterministic: identical input + weights → identical samples.
    #[test]
    fn neural_denoiser_is_deterministic() {
        let audio = generate_wave(48_000, 440.0, 1.0, 0.1);
        let nn = NeuralDenoiser::new().unwrap();
        let a = nn.denoise(&audio).unwrap();
        let b = nn.denoise(&audio).unwrap();
        assert_eq!(a.channels[0], b.channels[0]);
    }

    /// With gains ≈ 1 the passthrough model reconstructs the signal: the output
    /// tracks the input (high correlation) and neither collapses nor explodes.
    #[test]
    fn neural_denoiser_passthrough_reconstructs_signal() {
        let audio = generate_wave(48_000, 440.0, 1.0, 0.0); // clean tone
        let nn = NeuralDenoiser::new().unwrap();
        let out = nn.denoise(&audio).unwrap();
        let (x, y) = (&audio.channels[0], &out.channels[0]);
        // Skip the first/last window where overlap-add cover is one-sided.
        let (lo, hi) = (512usize, x.len() - 512);
        let mean = |v: &[f32]| v[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
        let (mx, my) = (mean(x), mean(y));
        let mut num = 0.0f64;
        let mut dx = 0.0f64;
        let mut dy = 0.0f64;
        for i in lo..hi {
            let (a, b) = ((x[i] - mx) as f64, (y[i] - my) as f64);
            num += a * b;
            dx += a * a;
            dy += b * b;
        }
        let corr = num / (dx.sqrt() * dy.sqrt());
        assert!(corr > 0.99, "passthrough should track the input, corr = {corr}");
        // Window-sum normalisation makes a unity mask reconstruct at unity level.
        let std_ratio = (variance(&y[lo..hi]) / variance(&x[lo..hi])).sqrt();
        assert!((0.95..1.05).contains(&std_ratio), "passthrough not unity: {std_ratio}");
    }

    /// Weights round-trip through a `.safetensors` file: a model loaded from
    /// disk denoises identically to the in-memory model with the same weights.
    #[test]
    fn neural_denoiser_loads_safetensors() {
        let cfg = NeuralConfig::default();
        let path = std::env::temp_dir().join("cathar_ml_rt.safetensors");
        candle_core::safetensors::save(&passthrough_tensors(&cfg, &Device::Cpu).unwrap(), &path)
            .unwrap();

        let audio = generate_wave(48_000, 440.0, 1.0, 0.1);
        let loaded = NeuralDenoiser::from_safetensors(&path, cfg).unwrap().denoise(&audio).unwrap();
        std::fs::remove_file(&path).ok();
        let in_memory = NeuralDenoiser::new().unwrap().denoise(&audio).unwrap();
        assert_eq!(loaded.channels[0], in_memory.channels[0]);
    }

    /// The pretrained model produces output that differs from the passthrough —
    /// it is not a no-op. (The bundled checkpoint was trained on synthetic
    /// harmonic-tone mixes, not pure sine waves; retrain on DNS-Challenge for
    /// speech-quality denoising.)
    #[test]
    fn pretrained_denoiser_is_not_passthrough() {
        let audio = generate_wave(48_000, 440.0, 1.0, 0.3);
        let pthru = NeuralDenoiser::new().unwrap().denoise(&audio).unwrap();
        let pretrained = NeuralDenoiser::pretrained().unwrap().denoise(&audio).unwrap();
        let max_diff = pthru.channels[0]
            .iter()
            .zip(&pretrained.channels[0])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff > 1e-4, "pretrained should differ from passthrough, max_diff={max_diff}");
    }

    /// A signal shorter than one FFT window is rejected, not panicked on.
    #[test]
    fn neural_denoiser_rejects_short_signal() {
        let audio = AudioData { sample_rate: 48_000, channels: vec![vec![0.1; 100]] };
        assert!(matches!(NeuralDenoiser::new().unwrap().denoise(&audio), Err(Error::TooShort)));
    }
}
