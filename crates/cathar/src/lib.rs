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

#![deny(missing_docs)]

mod analysis;
mod audio;
mod denoise;
mod dequant;
mod digitize;
mod edit;
mod enhance;
mod error;
mod filter;
mod hpss;
mod inpaint;
mod loudness;
#[cfg(feature = "ml")]
mod ml;
mod pitch;
mod resample;
mod restore;
mod spectrum;
mod timestretch;
mod util;

pub use analysis::{Stats, compute_stats};
pub use audio::AudioData;
pub use denoise::{Denoiser, NoisePrint, SpectralDenoiser, learn_noise_print, wiener_denoise};
pub use dequant::dequantize;
pub use digitize::{elliptical_mono, riaa_deemphasis, vinyl_restore};
pub use edit::{
    dither, fade, gain_db, pad, remix, reverse, select_channels, silence_strip, trim, vad,
};
pub use enhance::{
    EnhanceMethod, bandwidth_extend, bandwidth_extend_with_method, breath_remove, deess_multiband,
    deesser, voice_isolate,
};
pub use error::Error;
pub use filter::{bandpass, bass, compressor, equalizer, gate, highpass, limiter, lowpass, treble};
pub use hpss::hpss;
pub use inpaint::{inpaint_auto, inpaint_gap};
pub use loudness::{integrated_loudness, normalize_peak, true_peak_dbtp};
#[cfg(feature = "ml")]
pub use ml::{NeuralConfig, NeuralDenoiser};
pub use pitch::{detect_pitch, fundamental_hz};
pub use resample::resample;
pub use restore::{declick, declip, dehum, deplosive, dereverb, derustle, dewind, spectral_repair};
pub use spectrum::{Spectrogram, spectrogram};
pub use timestretch::{StretchMode, pitch_shift, time_stretch};
pub use util::{generate_wave, variance};

#[cfg(test)]
mod tests {
    use crate::audio::ieee754_extended;
    use crate::*;

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

    /// A clean signal with no clipping passes straight through (early return).
    #[test]
    fn declip_passthrough_when_clean() {
        let fs = 44_100.0;
        let signal: Vec<f32> = (0..4096)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / fs).sin())
            .collect();
        let out = declip(&signal, 0.95);
        assert_eq!(out, signal, "no clipped samples → no change");
    }

    /// A-SPADE rebuilds a hard-clipped sine back toward its true peak and tracks
    /// the original closely (the sparse Gabor reconstruction, not a flat fill).
    #[test]
    fn declip_restores_clipped_sine() {
        let fs = 44_100.0;
        let clip = 0.7f32;
        let n = 4096;
        let truth: Vec<f32> =
            (0..n).map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / fs).sin()).collect();
        let clipped: Vec<f32> = truth.iter().map(|&v| v.clamp(-clip, clip)).collect();
        assert!(clipped.iter().filter(|&&v| v.abs() >= clip).count() > 100);

        let restored = declip(&clipped, clip);

        let peak = restored.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
        assert!(peak > 0.9, "peak should climb back toward 1.0, got {peak}");
        // Track the true sine in the interior (edges have one-sided frame cover).
        let mse: f32 = (256..n - 256).map(|i| (restored[i] - truth[i]).powi(2)).sum::<f32>()
            / (n - 512) as f32;
        assert!(mse.sqrt() < 0.05, "RMS error vs true sine too high: {}", mse.sqrt());
    }

    /// Negative clipping (troughs chopped flat) is reconstructed too.
    #[test]
    fn declip_handles_negative_clipping() {
        let fs = 44_100.0;
        let n = 4096;
        let truth: Vec<f32> =
            (0..n).map(|i| (2.0 * std::f32::consts::PI * 300.0 * i as f32 / fs).sin()).collect();
        let clipped: Vec<f32> = truth.iter().map(|&v| v.max(-0.6)).collect();
        let restored = declip(&clipped, 0.6);
        let min = (256..n - 256).map(|i| restored[i]).fold(0.0f32, f32::min);
        assert!(min < -0.85, "negative peak should be rebuilt toward -1.0, got {min}");
    }

    /// The spectrogram's loudest bin sits at the tone's frequency.
    #[test]
    fn spectrogram_peaks_at_tone_frequency() {
        let fs = 44_100u32;
        let sig: Vec<f32> = (0..fs)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
            .collect();
        let spec = spectrogram(&sig, fs, 2048, 512);
        assert!(spec.frames() > 0 && spec.bins == 1025);
        let f = spec.frames() / 2;
        let mut peak_bin = 0;
        let mut peak_db = f32::MIN;
        for b in 0..spec.bins {
            if spec.get(f, b) > peak_db {
                peak_db = spec.get(f, b);
                peak_bin = b;
            }
        }
        assert!(
            (spec.bin_hz(peak_bin) - 1000.0).abs() < 50.0,
            "peak at {} Hz, want ~1000",
            spec.bin_hz(peak_bin)
        );
    }

    /// Debug harness: run a clipped sine through A-SPADE and print the result
    /// (peak / RMS). Run manually with `--ignored --nocapture`.
    #[ignore = "debug trace, run manually"]
    #[test]
    fn spade_trace() {
        let fs = 44_100.0;
        let clip = 0.7f32;
        let n = 4096;
        let freq = 220.0;
        let truth: Vec<f32> =
            (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / fs).sin()).collect();
        let clipped: Vec<f32> = truth.iter().map(|&v| v.clamp(-clip, clip)).collect();
        let out = declip(&clipped, clip);
        let peak = out.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
        let mse: f32 =
            (256..n - 256).map(|i| (out[i] - truth[i]).powi(2)).sum::<f32>() / (n - 512) as f32;
        eprintln!("RESULT peak={peak:.3} rms_err={:.4} (true peak 1.0)", mse.sqrt());
    }

    #[test]
    fn normalize_peak_target() {
        let signal = vec![0.5f32, -0.5, 0.25, -0.25, 0.1];
        let normalized = normalize_peak(&signal, -3.0); // target -3 dBFS ≈ 0.707
        let peak = normalized.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        assert!((peak - 0.707).abs() < 0.01, "peak should be ~0.707, got {peak}");
    }

    /// A full-scale 1 kHz mono sine measures ≈ -3.01 LUFS — the BS.1770
    /// absolute-calibration anchor (K-weight gain at 1 kHz ≈ +0.69 dB cancels
    /// the -0.691 offset, leaving the -3.01 dB of a full-scale sine's RMS).
    #[test]
    fn integrated_loudness_calibration() {
        let fs = 48_000u32;
        let sine: Vec<f32> = (0..fs * 3)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
            .collect();
        let lufs = integrated_loudness(&[sine], fs);
        assert!(
            (lufs - (-3.01)).abs() < 0.5,
            "full-scale 1 kHz sine should read ~-3.0 LUFS, got {lufs}"
        );
    }

    /// Louder input must measure higher loudness.
    #[test]
    fn integrated_loudness_monotonic() {
        let fs = 48_000u32;
        let tone = |amp: f32| -> Vec<f32> {
            (0..fs * 2)
                .map(|i| amp * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
                .collect()
        };
        let loud = integrated_loudness(&[tone(0.5)], fs);
        let quiet = integrated_loudness(&[tone(0.05)], fs);
        assert!(loud > quiet + 15.0, "0.5 vs 0.05 amp should differ ~20 LU: {loud} vs {quiet}");
    }

    /// Normalising to a target and re-measuring round-trips to that target
    /// (when the true-peak guard does not engage).
    #[test]
    fn normalize_r128_round_trip() {
        let fs = 48_000u32;
        let sine: Vec<f32> = (0..fs * 3)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
            .collect();
        let audio = AudioData { sample_rate: fs, channels: vec![sine.clone(), sine] };
        // Ceiling well above the signal's peak so only loudness drives the gain.
        let normalized = audio.normalize_r128(-23.0, 12.0);
        let after = integrated_loudness(&normalized.channels, fs);
        assert!((after - (-23.0)).abs() < 0.5, "should hit -23 LUFS, got {after}");
    }

    /// The true-peak ceiling caps inter-sample peaks instead of clipping.
    #[test]
    fn normalize_r128_respects_true_peak() {
        let fs = 48_000u32;
        let sine: Vec<f32> = (0..fs * 2)
            .map(|i| 0.1 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
            .collect();
        let audio = AudioData { sample_rate: fs, channels: vec![sine] };
        // Aggressive target would boost ~+20 dB; the -1 dBTP ceiling must hold.
        let normalized = audio.normalize_r128(0.0, -1.0);
        let tp = true_peak_dbtp(&normalized.channels, fs);
        assert!(tp <= -1.0 + 0.2, "true peak should be capped near -1 dBTP, got {tp}");
    }

    #[test]
    fn resample_identity_on_same_rate() {
        let sig: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin()).collect();
        assert_eq!(resample(&sig, 48_000, 48_000), sig);
    }

    #[test]
    fn resample_scales_length_by_ratio() {
        let sig = vec![0.0f32; 48_000];
        assert_eq!(resample(&sig, 48_000, 44_100).len(), 44_100);
        assert_eq!(resample(&sig, 48_000, 96_000).len(), 96_000);
    }

    /// Resampling preserves a tone's frequency: positive-going zero crossings
    /// per second equal the tone frequency regardless of sample rate.
    #[test]
    fn resample_preserves_tone_frequency() {
        let fs = 48_000u32;
        let f = 1000.0f32;
        let sig: Vec<f32> = (0..fs)
            .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin())
            .collect();
        let out = resample(&sig, fs, 32_000);
        let crossings = |s: &[f32]| s.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let (a, b) = (crossings(&sig), crossings(&out));
        assert!((a as i32 - b as i32).abs() <= 3, "frequency drifted: {a} vs {b}");
    }

    /// Downsampling anti-aliases: a tone above the new Nyquist is rejected, not
    /// folded back into the band.
    #[test]
    fn resample_downsample_antialiases() {
        let fs = 48_000u32;
        let f = 15_000.0f32; // above the 8 kHz Nyquist of the 16 kHz target
        let sig: Vec<f32> = (0..fs)
            .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin())
            .collect();
        let out = resample(&sig, fs, 16_000);
        let power = |s: &[f32]| s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32;
        assert!(power(&out) < power(&sig) * 0.1, "alias not suppressed: {}", power(&out));
    }

    #[test]
    fn audio_resample_sets_rate_and_all_channels() {
        let audio = generate_wave(44_100, 440.0, 0.5, 0.0);
        let out = audio.resample(48_000);
        assert_eq!(out.sample_rate, 48_000);
        assert_eq!(out.channels.len(), audio.channels.len());
        assert_eq!(
            out.channels[0].len(),
            (audio.channels[0].len() as f64 * 48_000.0 / 44_100.0).round() as usize
        );
    }

    /// 44100 Hz encoded as an 80-bit IEEE 754 extended float (AIFF COMM).
    #[test]
    fn ieee754_extended_encodes_44100() {
        assert_eq!(
            ieee754_extended(44_100.0),
            [0x40, 0x0e, 0xac, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    /// FLAC is lossless: encode then decode (via symphonia) round-trips to
    /// within 24-bit quantisation.
    #[test]
    fn flac_round_trips() {
        let audio = generate_wave(44_100, 440.0, 0.5, 0.0);
        let path = std::env::temp_dir().join("cathar_rt.flac");
        let p = path.to_str().unwrap();
        audio.to_file(p).unwrap();
        let back = AudioData::from_file(p).unwrap();
        std::fs::remove_file(p).ok();
        assert_eq!(back.sample_rate, 44_100);
        assert_eq!(back.channels.len(), 1);
        assert_eq!(back.channels[0].len(), audio.channels[0].len());
        let err = audio.channels[0]
            .iter()
            .zip(&back.channels[0])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-4, "FLAC 24-bit round-trip error {err}");
    }

    /// AIFF round-trips through symphonia to within 24-bit quantisation.
    #[test]
    fn aiff_round_trips() {
        let audio = generate_wave(48_000, 440.0, 0.5, 0.0);
        let path = std::env::temp_dir().join("cathar_rt.aiff");
        let p = path.to_str().unwrap();
        audio.to_file(p).unwrap();
        let back = AudioData::from_file(p).unwrap();
        std::fs::remove_file(p).ok();
        assert_eq!(back.sample_rate, 48_000);
        assert_eq!(back.channels[0].len(), audio.channels[0].len());
        let err = audio.channels[0]
            .iter()
            .zip(&back.channels[0])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-4, "AIFF 24-bit round-trip error {err}");
    }

    /// Spectral repair removes a brief transient artifact while leaving a
    /// sustained tone intact.
    #[test]
    fn spectral_repair_removes_transient_keeps_tone() {
        let fs = 48_000usize;
        let n = fs; // 1 s
        let two_pi = 2.0 * std::f32::consts::PI;
        // sustained 2 kHz tone (legitimate content)
        let mut sig: Vec<f32> =
            (0..n).map(|i| 0.3 * (two_pi * 2000.0 * i as f32 / fs as f32).sin()).collect();
        // 30 ms 7 kHz burst at 0.5 s (the transient artifact)
        let (start, len) = (fs / 2, fs * 30 / 1000);
        for (i, s) in sig.iter_mut().enumerate().skip(start).take(len) {
            *s += 0.5 * (two_pi * 7000.0 * i as f32 / fs as f32).sin();
        }

        let repaired = spectral_repair(&sig, 6.0);
        assert_eq!(repaired.len(), sig.len());

        // single-frequency magnitude over a sample range
        let mag_at = |x: &[f32], f: f32, lo: usize, hi: usize| -> f64 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate().take(hi).skip(lo) {
                let p = two_pi as f64 * f as f64 * i as f64 / fs as f64;
                re += v as f64 * p.cos();
                im -= v as f64 * p.sin();
            }
            (re * re + im * im).sqrt() / (hi - lo) as f64
        };

        // 7 kHz transient strongly attenuated in its window
        let burst_before = mag_at(&sig, 7000.0, start, start + len);
        let burst_after = mag_at(&repaired, 7000.0, start, start + len);
        assert!(
            burst_after < burst_before * 0.5,
            "transient not removed: {burst_before} -> {burst_after}"
        );
        // 2 kHz sustained tone preserved (steady region away from the burst)
        let tone_before = mag_at(&sig, 2000.0, 2000, fs / 4);
        let tone_after = mag_at(&repaired, 2000.0, 2000, fs / 4);
        assert!(
            tone_after > tone_before * 0.8,
            "sustained tone not preserved: {tone_before} -> {tone_after}"
        );
    }

    // Single-frequency magnitude over a sample range (a one-bin DFT).
    fn mag_at(x: &[f32], f: f32, fs: usize, lo: usize, hi: usize) -> f64 {
        let two_pi = 2.0 * std::f64::consts::PI;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate().take(hi).skip(lo) {
            let p = two_pi * f as f64 * i as f64 / fs as f64;
            re += v as f64 * p.cos();
            im -= v as f64 * p.sin();
        }
        (re * re + im * im).sqrt() / (hi - lo) as f64
    }

    #[test]
    fn dewind_attenuates_low_passes_high() {
        let fs = 48_000u32;
        let tone = |f: f32| -> Vec<f32> {
            (0..fs).map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin()).collect()
        };
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let low = dewind(&tone(40.0), fs, 80.0); // octave below the corner
        let high = dewind(&tone(1000.0), fs, 80.0); // well above
        assert!(rms(&low) < 0.2, "40 Hz wind should be cut, rms {}", rms(&low));
        assert!(rms(&high) > 0.6, "1 kHz should pass, rms {}", rms(&high));
    }

    #[test]
    fn deplosive_reduces_low_transient_keeps_tone() {
        let fs = 48_000usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        let mut sig: Vec<f32> =
            (0..fs).map(|i| 0.3 * (two_pi * 2000.0 * i as f32 / fs as f32).sin()).collect();
        let (start, len) = (fs / 2, fs * 40 / 1000); // 40 ms 100 Hz pop
        for (i, s) in sig.iter_mut().enumerate().skip(start).take(len) {
            *s += 0.6 * (two_pi * 100.0 * i as f32 / fs as f32).sin();
        }
        let out = deplosive(&sig, fs as u32, 6.0);
        let (b, a) = (
            mag_at(&sig, 100.0, fs, start, start + len),
            mag_at(&out, 100.0, fs, start, start + len),
        );
        assert!(a < b * 0.6, "plosive not reduced: {b} -> {a}");
        let (tb, ta) =
            (mag_at(&sig, 2000.0, fs, 2000, fs / 4), mag_at(&out, 2000.0, fs, 2000, fs / 4));
        assert!(ta > tb * 0.8, "tone not preserved: {tb} -> {ta}");
    }

    #[test]
    fn derustle_reduces_midband_transient_keeps_low_tone() {
        let fs = 48_000usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        let mut sig: Vec<f32> =
            (0..fs).map(|i| 0.3 * (two_pi * 500.0 * i as f32 / fs as f32).sin()).collect();
        let (start, len) = (fs / 2, fs * 40 / 1000); // 40 ms 3 kHz rustle burst
        for (i, s) in sig.iter_mut().enumerate().skip(start).take(len) {
            *s += 0.5 * (two_pi * 3000.0 * i as f32 / fs as f32).sin();
        }
        let out = derustle(&sig, fs as u32, 6.0);
        let (b, a) = (
            mag_at(&sig, 3000.0, fs, start, start + len),
            mag_at(&out, 3000.0, fs, start, start + len),
        );
        assert!(a < b * 0.6, "rustle not reduced: {b} -> {a}");
        let (tb, ta) =
            (mag_at(&sig, 500.0, fs, 2000, fs / 4), mag_at(&out, 500.0, fs, 2000, fs / 4));
        assert!(ta > tb * 0.8, "low tone not preserved: {tb} -> {ta}");
    }

    #[test]
    fn denoise_coherent_preserves_stereo_balance() {
        let fs = 44_100u32;
        let two_pi = 2.0 * std::f32::consts::PI;
        let n = fs as usize;
        let mut rng_l = 1u64;
        let mut rng_r = 2u64;
        let noise = |rng: &mut u64| -> f32 {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 7;
            *rng ^= *rng << 17;
            ((*rng as f32) / (u64::MAX as f32) - 0.5) * 0.1
        };
        // 1 kHz tone panned 2:1 (L louder), plus independent noise per channel.
        let l: Vec<f32> = (0..n)
            .map(|i| 0.4 * (two_pi * 1000.0 * i as f32 / fs as f32).sin() + noise(&mut rng_l))
            .collect();
        let r: Vec<f32> = (0..n)
            .map(|i| 0.2 * (two_pi * 1000.0 * i as f32 / fs as f32).sin() + noise(&mut rng_r))
            .collect();
        let audio = AudioData { sample_rate: fs, channels: vec![l, r] };
        let out = SpectralDenoiser::default().denoise_coherent(&audio).unwrap();
        assert_eq!(out.channels.len(), 2);
        let rl = mag_at(&out.channels[0], 1000.0, n, 0, n);
        let rr = mag_at(&out.channels[1], 1000.0, n, 0, n);
        let ratio = rl / rr;
        assert!((ratio - 2.0).abs() < 0.4, "stereo balance shifted from 2.0 to {ratio}");
    }

    #[test]
    fn deess_multiband_reduces_sibilance_keeps_tone() {
        let fs = 48_000usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        let mut sig: Vec<f32> =
            (0..fs).map(|i| 0.3 * (two_pi * 500.0 * i as f32 / fs as f32).sin()).collect();
        // 8 kHz sibilance bursts at 0.25 s and 0.75 s
        for &start in &[fs / 4, fs * 3 / 4] {
            for (i, s) in sig.iter_mut().enumerate().skip(start).take(fs * 50 / 1000) {
                *s += 0.4 * (two_pi * 8000.0 * i as f32 / fs as f32).sin();
            }
        }
        let out = deess_multiband(&sig, fs as u32, 4000.0, 3.0, 6.0, 4);
        let (b, a) = (
            mag_at(&sig, 8000.0, fs, fs / 4, fs / 4 + fs * 50 / 1000),
            mag_at(&out, 8000.0, fs, fs / 4, fs / 4 + fs * 50 / 1000),
        );
        assert!(a < b * 0.85, "sibilance not reduced: {b} -> {a}");
        let (tb, ta) = (mag_at(&sig, 500.0, fs, 0, fs / 8), mag_at(&out, 500.0, fs, 0, fs / 8));
        assert!(ta > tb * 0.8, "tone not preserved: {tb} -> {ta}");
    }

    /// A mono WAV must be tagged FRONT_CENTER, not FRONT_LEFT, so layout-aware
    /// players route it to both speakers.
    #[test]
    fn mono_wav_is_centered_not_front_left() {
        let audio = generate_wave(44_100, 440.0, 0.2, 0.0);
        let path = std::env::temp_dir().join("cathar_mask_test.wav");
        let p = path.to_str().unwrap();
        audio.to_file(p).unwrap();
        let bytes = std::fs::read(p).unwrap();
        std::fs::remove_file(p).ok();
        // WAVE_FORMAT_EXTENSIBLE dwChannelMask is at byte offset 40.
        let mask = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        assert_eq!(mask, 0x4, "mono WAV should be FRONT_CENTER (0x4), got {mask:#x}");
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
