//! Editing utilities: trim, pad, fade, silence strip, gain, remix, reverse,
//! dither — the swiss-army foundation for `0.7`.

use crate::AudioData;

/// Extract a time slice from a mono signal. `start` and `duration` are in
/// seconds; samples outside the original range are clamped to the boundary.
/// Returns `None` if the requested slice is empty.
pub fn trim(
    signal: &[f32],
    sample_rate: u32,
    start_sec: f32,
    duration_sec: f32,
) -> Option<Vec<f32>> {
    let n = signal.len();
    let start = ((start_sec * sample_rate as f32) as usize).min(n);
    let end = (start + (duration_sec * sample_rate as f32) as usize).min(n);
    if end <= start {
        return None;
    }
    Some(signal[start..end].to_vec())
}

/// Pad a mono signal with silence at the start and/or end. `start_sec` and
/// `end_sec` specify how many seconds of silence to prepend/append.
pub fn pad(signal: &[f32], sample_rate: u32, start_sec: f32, end_sec: f32) -> Vec<f32> {
    let prepend = (start_sec * sample_rate as f32) as usize;
    let append = (end_sec * sample_rate as f32) as usize;
    let mut out = Vec::with_capacity(prepend + signal.len() + append);
    out.resize(prepend, 0.0);
    out.extend_from_slice(signal);
    out.resize(prepend + signal.len() + append, 0.0);
    out
}

/// Apply a linear fade-in and/or fade-out to a mono signal. `in_sec` and
/// `out_sec` specify the fade durations in seconds. Non-overlapping.
pub fn fade(signal: &[f32], sample_rate: u32, in_sec: f32, out_sec: f32) -> Vec<f32> {
    let n = signal.len();
    let mut out = signal.to_vec();
    let fi = ((in_sec * sample_rate as f32) as usize).min(n);
    if fi > 0 {
        for i in 0..fi {
            let gain = i as f32 / fi as f32;
            out[i] *= gain;
        }
    }
    let fo = ((out_sec * sample_rate as f32) as usize).min(n);
    if fo > 0 {
        let start = n - fo;
        for i in start..n {
            let gain = (n - 1 - i) as f32 / fo as f32;
            out[i] *= gain;
        }
    }
    out
}

/// Strip leading and trailing silence. A sample is "silent" if its magnitude
/// is below `threshold_amplitude`. Runs shorter than `min_duration_sec` at
/// the boundary are discarded; gaps shorter than `min_duration_sec` within
/// non-silent audio are kept.
pub fn silence_strip(
    signal: &[f32],
    sample_rate: u32,
    threshold_amplitude: f32,
    min_duration_sec: f32,
) -> Vec<f32> {
    let n = signal.len();
    if n == 0 {
        return vec![];
    }
    let min_silent = ((min_duration_sec * sample_rate as f32) as usize).max(1);

    // Find the first sample above threshold (skip leading silence)
    let mut start = 0usize;
    for (i, &s) in signal.iter().enumerate() {
        if s.abs() > threshold_amplitude {
            start = i.saturating_sub(min_silent);
            break;
        }
        start = n; // all silent
    }
    if start >= n {
        return vec![0.0f32; signal.len()];
    }

    // Find the last sample above threshold (skip trailing silence)
    let mut end = n;
    for (i, &s) in signal.iter().enumerate().rev() {
        if s.abs() > threshold_amplitude {
            end = (i + min_silent + 1).min(n);
            break;
        }
        end = 0; // all silent
    }
    if end <= start {
        end = n;
        start = 0;
    }

    signal[start..end].to_vec()
}

/// Simple energy-based voice activity detection: returns `(start_sec,
/// end_sec)`, where the signal rises above `threshold_amplitude` and stays for
/// at least `min_duration_sec`. Returns `None` if no voice segment is found.
pub fn vad(
    signal: &[f32],
    sample_rate: u32,
    threshold_amplitude: f32,
    min_duration_sec: f32,
) -> Option<(f32, f32)> {
    let min_samples = ((min_duration_sec * sample_rate as f32) as usize).max(1);
    let mut run_start = None;
    for (i, &s) in signal.iter().enumerate() {
        if s.abs() > threshold_amplitude {
            if run_start.is_none() {
                run_start = Some(i);
            }
            if let Some(rs) = run_start {
                if i - rs >= min_samples {
                    let start = rs as f32 / sample_rate as f32;
                    // Find where it drops below threshold again
                    let mut end = i;
                    for (j, &s) in signal.iter().enumerate().skip(i) {
                        if s.abs() <= threshold_amplitude {
                            break;
                        }
                        end = j;
                    }
                    return Some((start, end as f32 / sample_rate as f32));
                }
            }
        } else {
            run_start = None;
        }
    }
    None
}

/// Multiply every sample by `10^(db/20)`. Positive dB boosts, negative cuts.
pub fn gain_db(signal: &[f32], db: f32) -> Vec<f32> {
    let factor = 10.0f32.powf(db / 20.0);
    signal.iter().map(|s| s * factor).collect()
}

/// Remix channels: `spec` is a list of channel indices or mappings.
/// `input_channels` are the original channels; output channels are built from
/// `spec`. Each entry in `spec` is a list of `(channel_index, gain)` tuples.
/// The output channel is the sum of `input_channel[idx] * gain` for each tuple.
///
/// # Examples
///
/// - `remix(&ch, &[vec![(0, 1.0), (1, 1.0)]])` → mono (L+R)
/// - `remix(&ch, &[vec![(1, 1.0)], vec![(0, 1.0)]])` → swap L/R
/// - `remix(&ch, &[vec![(0, 0.5), (1, 0.5)]])` → mono (L+R)/2
pub fn remix(channels: &[Vec<f32>], spec: &[Vec<(usize, f32)>]) -> Vec<Vec<f32>> {
    let n = channels.first().map_or(0, |c| c.len());
    spec.iter()
        .map(|out_spec| {
            let mut out = vec![0.0f32; n];
            for &(idx, gain) in out_spec {
                if let Some(ch) = channels.get(idx) {
                    for (i, &s) in ch.iter().enumerate() {
                        out[i] += s * gain;
                    }
                }
            }
            out
        })
        .collect()
}

/// Select a subset of channels by index.
pub fn select_channels(channels: &[Vec<f32>], indices: &[usize]) -> Vec<Vec<f32>> {
    indices.iter().filter_map(|&i| channels.get(i).cloned()).collect()
}

/// Reverse a mono signal in time.
pub fn reverse(signal: &[f32]) -> Vec<f32> {
    let mut out = signal.to_vec();
    out.reverse();
    out
}

/// Apply triangular-probability-density-function (TPDF) dither at `bits`
/// resolution. Adds two independent rectangular dithers of amplitude ±0.5 LSB
/// to decorrelate quantisation noise from the signal. Intended for use before
/// truncation to a lower bit depth.
///
/// peak-to-peak amplitude = 2 × quantisation step.
pub fn dither(signal: &[f32], bits: u32) -> Vec<f32> {
    let step = 2.0f32.powi(1 - bits as i32); // 2 / (2^bits) = max peak-to-peak
    let mut rng: u64 = 0xDEADBEEF;
    let next_f32 = |rng: &mut u64| -> f32 {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        (*rng as f32) / (u64::MAX as f32) - 0.5 // uniform in [-0.5, 0.5)
    };
    signal
        .iter()
        .map(|&s| {
            let d1 = next_f32(&mut rng) * step;
            let d2 = next_f32(&mut rng) * step;
            s + d1 + d2
        })
        .collect()
}

impl AudioData {
    /// Trim all channels to `[start_sec, start_sec + duration_sec]`.
    pub fn trim(&self, start_sec: f32, duration_sec: f32) -> AudioData {
        let ch: Vec<Vec<f32>> = self
            .channels
            .iter()
            .filter_map(|c| trim(c, self.sample_rate, start_sec, duration_sec))
            .collect();
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }

    /// Pad all channels with silence at start and end.
    pub fn pad_extend(&self, start_sec: f32, end_sec: f32) -> AudioData {
        let ch: Vec<Vec<f32>> =
            self.channels.iter().map(|c| pad(c, self.sample_rate, start_sec, end_sec)).collect();
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }

    /// Apply fade in/out to all channels.
    pub fn fade(&self, in_sec: f32, out_sec: f32) -> AudioData {
        let ch: Vec<Vec<f32>> =
            self.channels.iter().map(|c| fade(c, self.sample_rate, in_sec, out_sec)).collect();
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }

    /// Strip silence from all channel ends.
    pub fn silence_strip(&self, threshold_amplitude: f32, min_duration_sec: f32) -> AudioData {
        let ch: Vec<Vec<f32>> = self
            .channels
            .iter()
            .map(|c| silence_strip(c, self.sample_rate, threshold_amplitude, min_duration_sec))
            .collect();
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }

    /// Apply gain in dB to all channels.
    pub fn gain_db(&self, db: f32) -> AudioData {
        let ch: Vec<Vec<f32>> = self.channels.iter().map(|c| gain_db(c, db)).collect();
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }

    /// Remix channels with the given spec.
    pub fn remix(&self, spec: &[Vec<(usize, f32)>]) -> AudioData {
        let ch = remix(&self.channels, spec);
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }

    /// Reverse all channels.
    pub fn reverse(&self) -> AudioData {
        let ch: Vec<Vec<f32>> = self.channels.iter().map(|c| reverse(c)).collect();
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }

    /// Apply TPDF dither at the given bit depth.
    pub fn dither(&self, bits: u32) -> AudioData {
        let ch: Vec<Vec<f32>> = self.channels.iter().map(|c| dither(c, bits)).collect();
        AudioData { sample_rate: self.sample_rate, channels: ch }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_cuts_range() {
        let sig: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let out = trim(&sig, 1000, 0.1, 0.2).unwrap();
        assert_eq!(out.len(), 200);
        assert_eq!(out[0], 100.0);
    }

    #[test]
    fn pad_adds_silence() {
        let sig = vec![1.0f32; 100];
        let out = pad(&sig, 1000, 0.05, 0.1);
        // 0.05 * 1000 = 50 samples of prepend, 0.1 * 1000 = 100 samples of append
        assert_eq!(out.len(), 250);
        assert_eq!(out[0..50].iter().sum::<f32>(), 0.0);
        assert_eq!(out[150..250].iter().sum::<f32>(), 0.0);
    }

    #[test]
    fn fade_ramps() {
        let sig = vec![1.0f32; 1000];
        let out = fade(&sig, 1000, 0.1, 0.2);
        assert!(out[0] < 0.05); // near zero at start
        assert!(out[50] > 0.4 && out[50] < 0.6); // halfway through fade-in
        assert!(out[99] > 0.95); // end of fade-in
        assert!(out[999] < 0.05); // very end of fade-out
        assert!(out[500] > 0.99); // middle untouched
    }

    #[test]
    fn silence_strip_strips_ends() {
        let mut sig = vec![0.001f32; 1000];
        sig[200..300].copy_from_slice(&vec![1.0f32; 100]);
        let out = silence_strip(&sig, 1000, 0.01, 0.05);
        // Non-silent region is 200..300, plus 50-sample margin each side (0.05s).
        // Result should be roughly 150..351, i.e. ~200 samples (much less than 1000).
        assert!(out.len() > 100 && out.len() < 400, "got {}", out.len());
    }

    #[test]
    fn vad_finds_speech() {
        let mut sig = vec![0.001f32; 1000];
        sig[300..500].copy_from_slice(&vec![0.5f32; 200]);
        let result = vad(&sig, 1000, 0.1, 0.05);
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        assert!((0.29..=0.31).contains(&start));
        assert!((0.49..=0.51).contains(&end));
    }

    #[test]
    fn gain_db_multiplies() {
        let sig = vec![1.0f32; 100];
        let out = gain_db(&sig, -6.0);
        let expected = 10.0f32.powf(-6.0 / 20.0);
        assert!((out[0] - expected).abs() < 0.001);
    }

    #[test]
    fn remix_to_mono() {
        let l = vec![1.0f32; 100];
        let r = vec![2.0f32; 100];
        let out = remix(&[l, r], &[vec![(0, 1.0), (1, 1.0)]]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0][0], 3.0);
    }

    #[test]
    fn reverse_flips() {
        let sig: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let out = reverse(&sig);
        assert_eq!(out[0], 99.0);
        assert_eq!(out[99], 0.0);
    }

    #[test]
    fn dither_adds_noise() {
        let sig = vec![1.0f32; 1000];
        let out = dither(&sig, 16);
        // Nearly all samples should differ from the original
        let diff_count = sig.iter().zip(&out).filter(|(a, b)| (**a - **b).abs() > 1e-6).count();
        assert!(diff_count > 900);
    }

    #[test]
    fn audio_trim_trait() {
        let audio = crate::generate_wave(44_100, 440.0, 1.0, 0.0);
        let trimmed = audio.trim(0.1, 0.3);
        assert_eq!(trimmed.channels[0].len(), (0.3 * 44_100.0) as usize);
    }

    #[test]
    fn audio_gain_trait() {
        let audio = crate::generate_wave(44_100, 440.0, 1.0, 0.0);
        let boosted = audio.gain_db(6.0);
        // Avoid sample 0 (sin(0) = 0); use sample 100 where amplitude is nonzero.
        assert!(boosted.channels[0][100].abs() > audio.channels[0][100].abs());
    }

    #[test]
    fn audio_reverse_trait() {
        let audio = crate::generate_wave(44_100, 440.0, 1.0, 0.0);
        let rev = audio.reverse();
        assert_eq!(rev.channels[0][0], audio.channels[0][audio.channels[0].len() - 1]);
    }
}
