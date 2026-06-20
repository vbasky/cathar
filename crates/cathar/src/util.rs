//! Small shared helpers (STFT window, test-tone generator, variance).

use crate::AudioData;

pub(crate) fn hann_window(size: usize) -> Vec<f32> {
    let n = size as f32 - 1.0;
    (0..size)
        .map(|i| {
            let x = i as f32 / n;
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * x).cos()
        })
        .collect()
}

/// Generate a mono test tone: a `frequency`-Hz sine at 0.5 amplitude plus
/// uniform white noise scaled by `noise_level`, `duration_secs` long. Uses a
/// fixed seed, so the output is deterministic.
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

/// Population variance of a sample buffer (mean of squared deviations).
pub fn variance(samples: &[f32]) -> f32 {
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    samples.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / samples.len() as f32
}
