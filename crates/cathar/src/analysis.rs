//! Analysis and reporting: stats, spectrum measurement.

use crate::loudness::{integrated_loudness, true_peak_dbtp};

/// Per-channel and aggregate audio statistics.
#[derive(Debug, Clone)]
pub struct Stats {
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: usize,
    /// Total duration in seconds.
    pub duration_sec: f32,
    /// Sample count per channel.
    pub samples: usize,
    /// Peak amplitude in dBFS (0 = full scale).
    pub peak_dbfs: f32,
    /// RMS level in dBFS.
    pub rms_dbfs: f32,
    /// Crest factor = peak / RMS in dB (higher = more dynamic).
    pub crest_factor_db: f32,
    /// Integrated loudness (LUFS, BS.1770-4).
    pub integrated_lufs: f32,
    /// True-peak level in dBTP.
    pub true_peak_dbtp: f32,
    /// DC offset (mean of all samples × channel).
    pub dc_offset: f32,
    /// Per-channel peak in dBFS.
    pub channel_peaks: Vec<f32>,
    /// Per-channel RMS in dBFS.
    pub channel_rms: Vec<f32>,
}

/// Compute comprehensive statistics for an audio buffer. Returns `None` if the
/// signal is empty.
pub fn compute_stats(channels: &[Vec<f32>], sample_rate: u32) -> Option<Stats> {
    if channels.is_empty() || channels[0].is_empty() {
        return None;
    }
    let n = channels[0].len();
    let samples = n;
    let duration_sec = n as f32 / sample_rate as f32;

    // Aggregate peak and RMS across all channels (joint)
    let mut peak_sq = 0.0f32;
    let mut rms_acc = 0.0f64;
    let mut dc_sum = 0.0f64;
    let total = (n * channels.len()) as f64;

    let mut channel_peaks = Vec::with_capacity(channels.len());
    let mut channel_rms = Vec::with_capacity(channels.len());

    for ch in channels {
        let mut ch_peak = 0.0f32;
        let mut ch_sum_sq = 0.0f64;
        for &s in ch {
            let abs = s.abs();
            if abs > ch_peak {
                ch_peak = abs;
            }
            ch_sum_sq += (s as f64) * (s as f64);
            dc_sum += s as f64;
        }
        if ch_peak > peak_sq {
            peak_sq = ch_peak;
        }
        rms_acc += ch_sum_sq;
        channel_peaks.push(20.0 * ch_peak.max(1e-10).log10());
        let ch_rms = (ch_sum_sq / n as f64).sqrt() as f32;
        channel_rms.push(20.0 * ch_rms.max(1e-10).log10());
    }

    let rms = (rms_acc / total).sqrt() as f32;
    let peak_dbfs = 20.0 * peak_sq.max(1e-10).log10();
    let rms_dbfs = 20.0 * rms.max(1e-10).log10();
    let crest_factor_db = peak_dbfs - rms_dbfs;
    let dc_offset = (dc_sum / total) as f32;

    let integrated_lufs = integrated_loudness(channels, sample_rate);
    let true_peak_dbtp = true_peak_dbtp(channels, sample_rate);

    Some(Stats {
        sample_rate,
        channels: channels.len(),
        duration_sec,
        samples,
        peak_dbfs,
        rms_dbfs,
        crest_factor_db,
        integrated_lufs,
        true_peak_dbtp,
        dc_offset,
        channel_peaks,
        channel_rms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_wave;

    #[test]
    fn stats_returns_none_for_empty() {
        assert!(compute_stats(&[], 44100).is_none());
        assert!(compute_stats(&[vec![]], 44100).is_none());
    }

    #[test]
    fn stats_sine_values() {
        let audio = generate_wave(48_000, 1000.0, 1.0, 0.0);
        let s = compute_stats(&audio.channels, audio.sample_rate).unwrap();
        assert_eq!(s.sample_rate, 48_000);
        assert_eq!(s.channels, 1);
        assert!(s.duration_sec > 0.99 && s.duration_sec < 1.01);
        // Full-scale sine at 0.5 amplitude: peak ≈ -6 dBFS
        assert!((s.peak_dbfs - (-6.02)).abs() < 0.1, "peak: {}", s.peak_dbfs);
        // Sine RMS = peak / sqrt(2) ≈ -9 dBFS
        assert!((s.rms_dbfs - (-9.03)).abs() < 0.2, "rms: {}", s.rms_dbfs);
    }

    #[test]
    fn stats_mono_peak() {
        let audio = generate_wave(44_100, 440.0, 2.0, 0.0);
        let s = compute_stats(&audio.channels, audio.sample_rate).unwrap();
        assert!((s.channel_peaks[0] - (-6.02)).abs() < 0.1);
    }
}
