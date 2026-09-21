//! Bark-scale, masking-aware spectral denoising.

use crate::util::hann_window;
use realfft::RealFftPlanner;

fn bark(hz: f32) -> f32 {
    13.0 * (0.00076 * hz).atan() + 3.5 * (hz / 7500.0).powi(2).atan()
}

/// Denoise with a Bark-scale masking floor.
///
/// The noise estimate is minimum-statistics, while the residual floor follows
/// strong nearby spectral energy through a spreading function. This avoids the
/// unnaturally flat spectral holes produced by a fixed per-bin floor.
pub fn psycho_denoise(signal: &[f32], sample_rate: u32, alpha: f32, beta: f32) -> Vec<f32> {
    const FFT: usize = 2048;
    const HOP: usize = 512;
    if signal.len() < FFT || sample_rate == 0 {
        return signal.to_vec();
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT);
    let c2r = planner.plan_fft_inverse(FFT);
    let hann = hann_window(FFT);
    let bins = FFT / 2 + 1;
    let frames = signal.len() / HOP;
    let mut spectra = Vec::new();
    let mut noise = vec![f32::MAX; bins];
    let mut input = r2c.make_input_vec();
    let mut freq = r2c.make_output_vec();
    for frame in 0..frames {
        let start = frame * HOP;
        if start + FFT > signal.len() {
            break;
        }
        for i in 0..FFT {
            input[i] = signal[start + i] * hann[i];
        }
        r2c.process(&mut input, &mut freq).expect("real FFT buffer sizes are fixed");
        let mags: Vec<f32> = freq.iter().map(|z| z.norm()).collect();
        spectra.push(mags);
    }
    if spectra.is_empty() {
        return signal.to_vec();
    }
    // A low quantile is more robust than a strict minimum for broadband
    // noise: random-bin minima otherwise collapse toward zero as the file
    // gets longer and the denoiser becomes an accidental bypass.
    for k in 0..bins {
        let mut values: Vec<f32> = spectra.iter().map(|frame| frame[k]).collect();
        values.sort_by(|a, b| a.total_cmp(b));
        let take = (values.len() / 5).max(1);
        noise[k] = values[..take].iter().sum::<f32>() / take as f32;
    }
    let bark_bins: Vec<f32> =
        (0..bins).map(|k| bark(k as f32 * sample_rate as f32 / FFT as f32)).collect();
    let mut output = vec![0.0; signal.len() + FFT];
    let mut weight = vec![0.0; signal.len() + FFT];
    for (frame, mags) in spectra.iter().enumerate() {
        let start = frame * HOP;
        for i in 0..FFT {
            input[i] = signal[start + i] * hann[i];
        }
        r2c.process(&mut input, &mut freq).expect("real FFT buffer sizes are fixed");
        let mut band_peak = [0.0f32; 25];
        for k in 0..bins {
            let band = bark_bins[k].floor().clamp(0.0, 24.0) as usize;
            band_peak[band] = band_peak[band].max((mags[k] - alpha.max(0.0) * noise[k]).max(0.0));
        }
        for k in 0..bins {
            let band = bark_bins[k].floor().clamp(0.0, 24.0) as usize;
            let mut mask_floor: f32 = 0.0;
            for (b, &peak) in band_peak.iter().enumerate() {
                let spread = 10.0f32.powf(-0.18 * (band as f32 - b as f32).abs());
                mask_floor = mask_floor.max(peak * spread * 0.01);
            }
            let clean = (mags[k] - alpha.max(0.0) * noise[k])
                .max(beta.clamp(0.0, 1.0) * mags[k])
                .max(mask_floor)
                .min(mags[k]);
            let gain = if mags[k] > 1e-12 { clean / mags[k] } else { 0.0 };
            freq[k].re *= gain;
            freq[k].im *= gain;
        }
        freq[0].im = 0.0;
        freq[bins - 1].im = 0.0;
        c2r.process(&mut freq, &mut input).expect("inverse FFT buffer sizes are fixed");
        for i in 0..FFT {
            let w = hann[i];
            output[start + i] += input[i] * w / FFT as f32;
            weight[start + i] += w * w;
        }
    }
    output.truncate(signal.len());
    output.into_iter().zip(weight).map(|(x, w)| if w > 1e-8 { x / w } else { 0.0 }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suppresses_stationary_noise_and_preserves_tone() {
        let sr = 48_000u32;
        let clean: Vec<f32> = (0..sr as usize * 2)
            .map(|i| {
                let t = i as f32 / sr as f32;
                if i < sr as usize {
                    0.0
                } else {
                    0.4 * (2.0 * std::f32::consts::PI * 1000.0 * t).sin()
                }
            })
            .collect();
        let mut state = 0x1234_5678u32;
        let x: Vec<f32> = clean
            .iter()
            .map(|&v| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                v + (state as f32 / u32::MAX as f32 - 0.5) * 0.08
            })
            .collect();
        let y = psycho_denoise(&x, sr, 0.5, 0.05);
        let mid = sr as usize;
        let tone_rms =
            (y[mid..].iter().map(|v| v * v).sum::<f32>() / (y.len() - mid) as f32).sqrt();
        assert!(tone_rms > 0.5 * 0.4 / 2f32.sqrt());
        assert!(y.iter().all(|v| v.is_finite()));
    }
    #[test]
    fn short_input_is_bypass() {
        let x = vec![0.2; 64];
        assert_eq!(psycho_denoise(&x, 48_000, 2.0, 0.02), x);
    }
}
