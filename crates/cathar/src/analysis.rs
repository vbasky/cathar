//! Analysis and reporting: stats, spectrum measurement.

use crate::loudness::{integrated_loudness, true_peak_dbtp};

/// Linear amplitude at/above which a sample counts as clipped (~ −0.09 dBFS).
const CLIP_AMPLITUDE: f32 = 0.99;
/// Minimum consecutive samples at [`CLIP_AMPLITUDE`] that count as a flat-top run.
const CLIP_RUN_MIN: usize = 2;
/// Frame size / hop for noise-floor min-statistics (matches `SpectralDenoiser` defaults).
const NOISE_FRAME: usize = 2048;
const NOISE_HOP: usize = 512;
/// Quietest fraction of frames taken as the noise estimate.
const NOISE_QUIET_RATIO: f32 = 0.15;
/// Absolute noise-floor (dBFS) above which gaps sound noisy.
const NOISE_FLOOR_HIGH_DB: f32 = -45.0;
/// Peak-to-floor gap (dB) required before a high floor is treated as noise
/// rather than a continuous tone with no gaps.
const NOISE_GAP_MIN_DB: f32 = 12.0;

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
    /// Estimated noise floor in dBFS — mean RMS of the quietest 15 % of
    /// 2048-sample frames (minimum-statistics). Equals [`Self::rms_dbfs`] when
    /// the file is too short to form a frame.
    pub noise_floor_dbfs: f32,
    /// Signal-to-noise estimate: [`Self::rms_dbfs`] − [`Self::noise_floor_dbfs`].
    pub snr_db: f32,
    /// Samples with `|x| ≥ 0.99` (near digital full scale).
    pub clipped_samples: usize,
    /// Contiguous runs of ≥ 2 samples at/above 0.99 (flat-topped peaks).
    pub clipped_runs: usize,
    /// Per-channel peak in dBFS.
    pub channel_peaks: Vec<f32>,
    /// Per-channel RMS in dBFS.
    pub channel_rms: Vec<f32>,
    /// Zero-lag L/R phase correlation in `[-1, +1]` (stereo only; `None` for mono).
    pub phase_correlation: Option<f32>,
}

/// A restoration hint derived from [`Stats`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Short issue name (`"clipping"`, `"noise"`, `"true-peak"`).
    pub issue: &'static str,
    /// What was measured and why it matters.
    pub detail: String,
    /// CLI subcommand that addresses it (`"declip"`, `"denoise"`, `"normalize"`).
    pub command: &'static str,
    /// Extra flags after the input path (empty for most commands).
    pub args: &'static str,
}

impl Suggestion {
    /// `cathar <command> <input> [args]`
    pub fn invocation(&self, input: &str) -> String {
        if self.args.is_empty() {
            format!("cathar {} {input}", self.command)
        } else {
            format!("cathar {} {input} {}", self.command, self.args)
        }
    }
}

impl Stats {
    /// Restoration-oriented hints for failing checks. Empty if nothing looks wrong.
    ///
    /// Thresholds (inspectable, not a mix-quality score):
    /// - **clipping** — at least one flat-top run, or sample peak above 0 dBFS
    ///   → `declip` (turning gain down does not restore flattened peaks)
    /// - **true-peak** — intersample overs with no sample-peak clip
    ///   → `normalize --true-peak -1`
    /// - **noise** — quiet-frame floor above −45 dBFS *and* at least 12 dB below
    ///   the peak (so a continuous tone is not flagged) → `denoise`
    pub fn suggestions(&self) -> Vec<Suggestion> {
        let mut out = Vec::new();

        if self.clipped_runs > 0 || self.peak_dbfs > 0.0 {
            out.push(Suggestion {
                issue: "clipping",
                detail: format!(
                    "{} run(s), {} sample(s), peak {:.1} dBFS — reducing gain will not restore flattened peaks",
                    self.clipped_runs, self.clipped_samples, self.peak_dbfs
                ),
                command: "declip",
                args: "",
            });
        } else if self.true_peak_dbtp > 0.0 {
            out.push(Suggestion {
                issue: "true-peak",
                detail: format!(
                    "true peak {:.1} dBTP (intersample overs; sample peak {:.1} dBFS)",
                    self.true_peak_dbtp, self.peak_dbfs
                ),
                command: "normalize",
                args: "--true-peak -1",
            });
        }

        let gap = self.peak_dbfs - self.noise_floor_dbfs;
        if self.samples >= NOISE_FRAME * 2
            && self.noise_floor_dbfs > NOISE_FLOOR_HIGH_DB
            && gap >= NOISE_GAP_MIN_DB
        {
            out.push(Suggestion {
                issue: "noise",
                detail: format!(
                    "floor {:.1} dBFS, SNR {:.1} dB — denoise under the signal; `gate` only mutes the gaps",
                    self.noise_floor_dbfs, self.snr_db
                ),
                command: "denoise",
                args: "",
            });
        }

        out
    }
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

    let noise_floor_dbfs = noise_floor_dbfs(channels, rms_dbfs);
    let snr_db = rms_dbfs - noise_floor_dbfs;
    let (clipped_samples, clipped_runs) = clip_runs(channels);

    let phase_correlation = if channels.len() >= 2 {
        Some(crate::stereo::phase_correlation(&channels[0], &channels[1]))
    } else {
        None
    };

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
        noise_floor_dbfs,
        snr_db,
        clipped_samples,
        clipped_runs,
        channel_peaks,
        channel_rms,
        phase_correlation,
    })
}

/// Mean RMS of the quietest [`NOISE_QUIET_RATIO`] of frames. Falls back to
/// `rms_dbfs` when the buffer is shorter than one frame.
fn noise_floor_dbfs(channels: &[Vec<f32>], rms_dbfs: f32) -> f32 {
    let n = channels[0].len();
    if n < NOISE_FRAME {
        return rms_dbfs;
    }
    let mut frame_rms = Vec::new();
    let mut offset = 0;
    while offset + NOISE_FRAME <= n {
        let mut acc = 0.0f64;
        let mut count = 0usize;
        for ch in channels {
            for &s in &ch[offset..offset + NOISE_FRAME] {
                acc += (s as f64) * (s as f64);
                count += 1;
            }
        }
        frame_rms.push((acc / count as f64).sqrt() as f32);
        offset += NOISE_HOP;
    }
    if frame_rms.is_empty() {
        return rms_dbfs;
    }
    frame_rms.sort_by(|a, b| a.total_cmp(b));
    let k = ((frame_rms.len() as f32) * NOISE_QUIET_RATIO).ceil() as usize;
    let k = k.clamp(1, frame_rms.len());
    let mean = frame_rms[..k].iter().sum::<f32>() / k as f32;
    20.0 * mean.max(1e-10).log10()
}

/// Count near-full-scale samples and contiguous flat-top runs (same sign).
fn clip_runs(channels: &[Vec<f32>]) -> (usize, usize) {
    let mut samples = 0usize;
    let mut runs = 0usize;
    for ch in channels {
        let mut i = 0;
        while i < ch.len() {
            if ch[i].abs() >= CLIP_AMPLITUDE {
                let sign = ch[i].signum();
                let start = i;
                i += 1;
                while i < ch.len() && ch[i].abs() >= CLIP_AMPLITUDE && ch[i].signum() == sign {
                    i += 1;
                }
                let len = i - start;
                samples += len;
                if len >= CLIP_RUN_MIN {
                    runs += 1;
                }
            } else {
                i += 1;
            }
        }
    }
    (samples, runs)
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

    #[test]
    fn clean_sine_has_no_clip_runs_or_suggestions() {
        let audio = generate_wave(48_000, 1000.0, 1.0, 0.0);
        let s = compute_stats(&audio.channels, audio.sample_rate).unwrap();
        assert_eq!(s.clipped_samples, 0);
        assert_eq!(s.clipped_runs, 0);
        // Continuous tone: quietest frames ≈ RMS, so the 12 dB-gap guard holds.
        assert!(
            (s.noise_floor_dbfs - s.rms_dbfs).abs() < 1.0,
            "floor {} vs rms {}",
            s.noise_floor_dbfs,
            s.rms_dbfs
        );
        assert!(s.suggestions().is_empty(), "{:?}", s.suggestions());
    }

    #[test]
    fn hard_clipped_sine_flags_declip() {
        let mut audio = generate_wave(48_000, 1000.0, 1.0, 0.0);
        for s in &mut audio.channels[0] {
            *s = (*s * 4.0).clamp(-1.0, 1.0);
        }
        let s = compute_stats(&audio.channels, audio.sample_rate).unwrap();
        assert!(s.clipped_runs > 0, "runs {}", s.clipped_runs);
        assert!(s.clipped_samples > 100, "samples {}", s.clipped_samples);
        let hints = s.suggestions();
        assert!(hints.iter().any(|h| h.command == "declip"), "{hints:?}");
        assert_eq!(hints[0].invocation("mix.wav"), "cathar declip mix.wav");
    }

    #[test]
    fn noisy_gaps_flag_denoise() {
        let sr = 48_000u32;
        let n = sr as usize;
        let mut sig = vec![0.0f32; n];
        // Broadband floor at ~ −30 dBFS in the gaps, plus a −6 dBFS burst.
        let mut rng: u64 = 7;
        for s in &mut sig {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            *s = ((rng as f32) / (u64::MAX as f32) - 0.5) * 0.08;
        }
        for s in sig.iter_mut().skip(n / 3).take(n / 8) {
            *s = 0.5;
        }
        let s = compute_stats(&[sig], sr).unwrap();
        assert!(s.noise_floor_dbfs > -45.0, "floor {}", s.noise_floor_dbfs);
        assert!(
            s.peak_dbfs - s.noise_floor_dbfs >= 12.0,
            "gap {}",
            s.peak_dbfs - s.noise_floor_dbfs
        );
        let hints = s.suggestions();
        assert!(
            hints.iter().any(|h| h.command == "denoise"),
            "floor {} peak {} hints {hints:?}",
            s.noise_floor_dbfs,
            s.peak_dbfs
        );
    }

    #[test]
    fn isolated_full_scale_sample_is_not_a_run() {
        let mut audio = generate_wave(48_000, 440.0, 1.0, 0.0);
        audio.channels[0][1000] = 1.0;
        let s = compute_stats(&audio.channels, audio.sample_rate).unwrap();
        assert_eq!(s.clipped_samples, 1);
        assert_eq!(s.clipped_runs, 0);
        assert!(!s.suggestions().iter().any(|h| h.issue == "clipping"), "{:?}", s.suggestions());
    }
}
