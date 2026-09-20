//! Tape / VHS restoration chain — a named preset of gated stages.
//!
//! Analog pre-conditioning, gated physical repair, per-harmonic hum
//! cancellation ahead of the noise probe, event-gated plosives, then spectral
//! subtraction from the quietest 4 s. Neural denoisers stay behind
//! `--features ml` and are not part of this chain.

use crate::align::azimuth_correct_with_method;
use crate::analysis::compute_stats;
use crate::declip::declip;
use crate::decrackle::decrackle;
use crate::dehum_adaptive;
use crate::denoise::{Denoiser, SpectralDenoiser, learn_noise_print_quietest};
use crate::edit::remove_dc;
use crate::enhance::deess_multiband;
use crate::inpaint::inpaint_auto;
use crate::restore::{declick, deplosive, dewind};
use crate::stereo::mono_below;
use crate::{AudioData, Error, LagMethod};

/// Seconds of the quietest stretch used to learn the noise profile.
const NOISE_PROBE_S: f32 = 4.0;

/// Knobs for [`vhs_restore`]. Defaults match the measured tape chain.
#[derive(Debug, Clone)]
pub struct VhsOptions {
    /// Spectral-subtraction over-subtraction factor (1 = gentle, 6 = aggressive).
    pub alpha: f32,
    /// Spectral floor as a fraction of the input magnitude.
    pub beta: f32,
    /// High-pass cutoff for rumble / wind (Hz).
    pub dewind_cutoff: f32,
    /// How many mains harmonics to evaluate (each is gated on standing out).
    pub harmonics: usize,
    /// Optional EBU R128 target (LUFS). `None` leaves loudness alone.
    pub normalize_lufs: Option<f32>,
}

impl Default for VhsOptions {
    fn default() -> Self {
        Self { alpha: 3.0, beta: 0.01, dewind_cutoff: 80.0, harmonics: 8, normalize_lufs: None }
    }
}

/// Restore a tape / VHS capture with a gated cascade of inspectable stages.
///
/// Order: DC block → rumble high-pass → stereo azimuth + bass-mono → declip
/// (only if flat-top runs are present) → dropout inpaint → declick →
/// decrackle → adaptive dehum (auto 50/60) → event-gated deplosive →
/// coherent spectral subtraction from the quietest 4 s → multiband de-ess →
/// optional loudness normalise.
///
/// Hum cancellation and plosive control are internally gated: a file that
/// carries neither is not carved by those stages.
pub fn vhs_restore(audio: &AudioData, opts: &VhsOptions) -> Result<AudioData, Error> {
    let sr = audio.sample_rate;
    let mut audio = audio.map_channels(remove_dc);
    audio = audio.map_channels(|c| dewind(c, sr, opts.dewind_cutoff));

    if audio.channels.len() >= 2 {
        let (l, r) = azimuth_correct_with_method(
            &audio.channels[0],
            &audio.channels[1],
            sr,
            5.0,
            LagMethod::GccPhat,
        );
        let (l, r) = mono_below(&l, &r, sr, 100.0);
        audio.channels[0] = l;
        audio.channels[1] = r;
    }

    if let Some(stats) = compute_stats(&audio.channels, sr) {
        if stats.clipped_runs > 0 {
            audio = audio.map_channels(|c| declip(c, 0.95));
        }
    }

    audio = audio.map_channels(|c| inpaint_auto(c, sr, 50.0));
    audio = audio.map_channels(|c| declick(c, 8.0, 64));
    audio = audio.map_channels(|c| decrackle(c, sr, 6.0));
    audio = audio.map_channels(|c| dehum_adaptive(c, sr, 0.0, opts.harmonics));
    audio = audio.map_channels(|c| deplosive(c, sr, 4.0));

    let denoiser = match learn_noise_print_quietest(&audio, NOISE_PROBE_S) {
        Ok(np) => SpectralDenoiser::with_noise_print(np, opts.alpha, opts.beta),
        Err(_) => SpectralDenoiser { alpha: opts.alpha, beta: opts.beta, ..Default::default() },
    };
    audio = if audio.channels.len() > 1 {
        denoiser.denoise_coherent(&audio)?
    } else {
        denoiser.denoise(&audio)?
    };

    audio = audio.map_channels(|c| deess_multiband(c, sr, 4000.0, -24.0, 4.0, 3));

    if let Some(lufs) = opts.normalize_lufs {
        audio = audio.normalize_r128(lufs, -1.0);
    }
    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::generate_wave;

    #[test]
    fn vhs_restore_preserves_shape() {
        let audio = generate_wave(48_000, 440.0, 2.0, 0.15);
        let out = vhs_restore(&audio, &VhsOptions::default()).unwrap();
        assert_eq!(out.sample_rate, audio.sample_rate);
        assert_eq!(out.channels.len(), audio.channels.len());
        assert_eq!(out.channels[0].len(), audio.channels[0].len());
    }

    #[test]
    fn vhs_restore_reduces_hissy_hum() {
        let sr = 48_000u32;
        let n = sr as usize * 2;
        let two_pi = 2.0 * std::f32::consts::PI;
        let mut rng = 1u64;
        let noise = |rng: &mut u64| {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 7;
            *rng ^= *rng << 17;
            (*rng as f32 / u64::MAX as f32 - 0.5) * 0.2
        };
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                0.3 * (two_pi * 1000.0 * t).sin()
                    + 0.4 * (two_pi * 60.0 * t).sin()
                    + noise(&mut rng)
            })
            .collect();
        let audio = AudioData { sample_rate: sr, channels: vec![x.clone()] };
        let out = vhs_restore(&audio, &VhsOptions::default()).unwrap();
        let power = |v: &[f32]| v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32;
        assert!(
            power(&out.channels[0]) < power(&x) * 0.95,
            "vhs chain should reduce noisy+hum power"
        );
    }
}
