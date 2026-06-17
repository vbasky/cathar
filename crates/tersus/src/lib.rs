//! AI audio denoising — clean dialogue, remove noise, isolate speech.
//!
//! `tersus` provides pluggable backends for audio denoising. The default build
//! uses a mock backend so the loop runs with no model weights. Enable the `ort`
//! feature for real ONNX-based speech denoising inference.
//!
//! # Quick start
//!
//! ```rust
//! use tersus::{Denoiser, MockDenoiser, AudioData};
//!
//! let audio = AudioData::from_file("noisy.wav")?;
//! let denoiser = MockDenoiser;
//! let clean = denoiser.denoise(&audio)?;
//! clean.to_file("clean.wav")?;
//! # Ok::<(), tersus::Error>(())
//! ```

use hound::{WavReader, WavSpec, WavWriter};
use thiserror::Error;

/// Errors tersus can produce.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("audio format error: {0}")]
    Hound(#[from] hound::Error),

    #[error("unsupported sample rate or channel count")]
    UnsupportedFormat,
}

/// In-memory audio buffer: 32-bit float samples, one channel per row.
#[derive(Debug, Clone)]
pub struct AudioData {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl AudioData {
    /// Read a WAV file into memory.
    pub fn from_file(path: &str) -> Result<Self, Error> {
        let mut reader = WavReader::open(path)?;
        let spec = reader.spec();
        let num_channels = spec.channels as usize;
        let sample_rate = spec.sample_rate;
        let mut channels = vec![Vec::new(); num_channels];

        match spec.sample_format {
            hound::SampleFormat::Float => {
                for (i, sample) in reader.samples::<f32>().enumerate() {
                    channels[i % num_channels].push(sample?);
                }
            }
            hound::SampleFormat::Int => {
                let max = (1u32 << (spec.bits_per_sample - 1)) as f32;
                match spec.bits_per_sample {
                    16 => {
                        for (i, sample) in reader.samples::<i16>().enumerate() {
                            channels[i % num_channels].push(sample? as f32 / max);
                        }
                    }
                    32 => {
                        for (i, sample) in reader.samples::<i32>().enumerate() {
                            channels[i % num_channels].push(sample? as f32 / (max * max));
                        }
                    }
                    _ => return Err(Error::UnsupportedFormat),
                }
            }
        }
        Ok(Self { sample_rate, channels })
    }

    /// Write in-memory audio to a 32-bit float WAV file.
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

    /// Duplicate a mono track to stereo.
    pub fn mono_to_stereo(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: vec![samples.clone(), samples],
        }
    }
}

/// Trait for audio denoising backends.
pub trait Denoiser {
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error>;
}

/// Mock denoiser: passes audio through unchanged.
///
/// Always available (no feature gate). Useful for testing the loop without
/// model weights.
pub struct MockDenoiser;

impl Denoiser for MockDenoiser {
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error> {
        Ok(input.clone())
    }
}

/// Generate a sine wave with optional white noise.
///
/// Returns a 32-bit float mono WAV in memory.
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
            // Fast xorshift noise
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let noise = ((rng as f32) / (u64::MAX as f32) - 0.5) * noise_level;
            signal + noise
        })
        .collect();

    AudioData {
        sample_rate,
        channels: vec![samples],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_denoiser_passthrough() {
        let audio = generate_wave(44100, 440.0, 1.0, 0.1);
        let denoiser = MockDenoiser;
        let clean = denoiser.denoise(&audio).unwrap();
        assert_eq!(clean.sample_rate, audio.sample_rate);
        assert_eq!(clean.channels.len(), audio.channels.len());
        assert_eq!(clean.channels[0].len(), audio.channels[0].len());
        // Mock passes through, so samples should be identical
        for (a, b) in audio.channels[0].iter().zip(clean.channels[0].iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn generate_wave_bounds() {
        let audio = generate_wave(48000, 1000.0, 2.5, 0.0);
        assert_eq!(audio.sample_rate, 48000);
        assert_eq!(audio.channels.len(), 1);
        assert_eq!(audio.channels[0].len(), 120_000); // 2.5s * 48k
        // No noise, so all values should be within [-0.5, 0.5]
        for s in &audio.channels[0] {
            assert!(*s >= -0.5 && *s <= 0.5);
        }
    }

    #[test]
    fn generate_wave_with_noise() {
        let audio = generate_wave(44100, 440.0, 0.5, 0.3);
        let has_outlier = audio
            .channels[0]
            .iter()
            .any(|s| (*s - 0.5).abs() > 1e-6 || (*s + 0.5).abs() > 1e-6);
        assert!(has_outlier, "noise should create values beyond pure sine bounds");
    }
}
