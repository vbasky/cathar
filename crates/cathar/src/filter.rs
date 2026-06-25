//! Biquad filters and dynamics processing (compressor, limiter, gate).

/// Coefficients for a Direct Form I biquad filter.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn design_lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn design_highpass(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn design_bandpass(sample_rate: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn design_peaking(sample_rate: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let a = 10.0f32.powf(gain_db / 40.0);
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha / a) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn design_lowshelf(sample_rate: f32, cutoff: f32, q: f32, gain_db: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let a = 10.0f32.powf(gain_db / 40.0);
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let sqrt_a = a.sqrt();
        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn design_highshelf(sample_rate: f32, cutoff: f32, q: f32, gain_db: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let a = 10.0f32.powf(gain_db / 40.0);
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let sqrt_a = a.sqrt();
        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

// ── public API ─────────────────────────────────────────────────────────────

/// Apply a low-pass biquad filter (12 dB/oct) to a mono signal.
pub fn lowpass(signal: &[f32], sample_rate: u32, cutoff: f32) -> Vec<f32> {
    let mut filter =
        Biquad::design_lowpass(sample_rate as f32, cutoff, std::f32::consts::FRAC_1_SQRT_2);
    signal.iter().map(|&x| filter.process(x)).collect()
}

/// Apply a high-pass biquad filter (12 dB/oct) to a mono signal.
pub fn highpass(signal: &[f32], sample_rate: u32, cutoff: f32) -> Vec<f32> {
    let mut filter =
        Biquad::design_highpass(sample_rate as f32, cutoff, std::f32::consts::FRAC_1_SQRT_2);
    signal.iter().map(|&x| filter.process(x)).collect()
}

/// Apply a band-pass biquad filter to a mono signal.
pub fn bandpass(signal: &[f32], sample_rate: u32, freq: f32, q: f32) -> Vec<f32> {
    let mut filter = Biquad::design_bandpass(sample_rate as f32, freq, q);
    signal.iter().map(|&x| filter.process(x)).collect()
}

/// Apply a peaking (bell) EQ filter at `freq` Hz with `gain_db` boost/cut and
/// bandwidth `q`.
pub fn equalizer(signal: &[f32], sample_rate: u32, freq: f32, q: f32, gain_db: f32) -> Vec<f32> {
    let mut filter = Biquad::design_peaking(sample_rate as f32, freq, q, gain_db);
    signal.iter().map(|&x| filter.process(x)).collect()
}

/// Apply a low-shelf filter: boost/cut `gain_db` below `cutoff` Hz.
pub fn bass(signal: &[f32], sample_rate: u32, cutoff: f32, gain_db: f32) -> Vec<f32> {
    let mut filter = Biquad::design_lowshelf(sample_rate as f32, cutoff, 1.0, gain_db);
    signal.iter().map(|&x| filter.process(x)).collect()
}

/// Apply a high-shelf filter: boost/cut `gain_db` above `cutoff` Hz.
pub fn treble(signal: &[f32], sample_rate: u32, cutoff: f32, gain_db: f32) -> Vec<f32> {
    let mut filter = Biquad::design_highshelf(sample_rate as f32, cutoff, 1.0, gain_db);
    signal.iter().map(|&x| filter.process(x)).collect()
}

// ── dynamics ────────────────────────────────────────────────────────────────

/// Downward compressor with soft-knee. Reduces gain when the envelope exceeds
/// `threshold` dBFS, by a `ratio`:1 slope. `attack` and `release` in seconds.
pub fn compressor(
    signal: &[f32],
    sample_rate: u32,
    threshold_dbfs: f32,
    ratio: f32,
    attack_sec: f32,
    release_sec: f32,
) -> Vec<f32> {
    let thresh_linear = 10.0f32.powf(threshold_dbfs / 20.0);
    let alpha_attack = (-1.0 / (attack_sec * sample_rate as f32)).exp();
    let alpha_release = (-1.0 / (release_sec * sample_rate as f32)).exp();
    let slope = 1.0 - 1.0 / ratio;
    let mut env = 0.0f32;
    signal
        .iter()
        .map(|&x| {
            let level = x.abs();
            let alpha = if level > env { alpha_attack } else { alpha_release };
            env = alpha * env + (1.0 - alpha) * level;
            let gain = if env > thresh_linear {
                let over = env / thresh_linear;
                let reduction = slope * over.log10() * 20.0;
                10.0f32.powf(-reduction / 20.0)
            } else {
                1.0
            };
            x * gain
        })
        .collect()
}

/// Brickwall limiter: hard-clips gain so output never exceeds `ceiling_dbfs`.
pub fn limiter(signal: &[f32], sample_rate: u32, ceiling_dbfs: f32) -> Vec<f32> {
    let ceil = 10.0f32.powf(ceiling_dbfs / 20.0);
    let alpha = (-1.0 / (0.001 * sample_rate as f32)).exp(); // 1 ms attack
    let mut env = 0.0f32;
    signal
        .iter()
        .map(|&x| {
            let level = x.abs();
            env = alpha * env + (1.0 - alpha) * level;
            let gain = if env > ceil { ceil / env } else { 1.0 };
            (x * gain).clamp(-ceil, ceil)
        })
        .collect()
}

/// Noise gate: attenuates the signal when the envelope falls below
/// `threshold_dbfs`. `attack` and `release` in seconds.
pub fn gate(
    signal: &[f32],
    sample_rate: u32,
    threshold_dbfs: f32,
    attack_sec: f32,
    release_sec: f32,
) -> Vec<f32> {
    let thresh_linear = 10.0f32.powf(threshold_dbfs / 20.0);
    let alpha_attack = (-1.0 / (attack_sec * sample_rate as f32)).exp();
    let alpha_release = (-1.0 / (release_sec * sample_rate as f32)).exp();
    let mut env = 0.0f32;
    signal
        .iter()
        .map(|&x| {
            let level = x.abs();
            let alpha = if level > env { alpha_attack } else { alpha_release };
            env = alpha * env + (1.0 - alpha) * level;
            let gain = if env < thresh_linear { 0.0 } else { 1.0 };
            x * gain
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_wave;

    #[test]
    fn lowpass_attenuates_high() {
        let audio = generate_wave(48_000, 5000.0, 0.1, 0.0);
        let filtered = lowpass(&audio.channels[0], 48_000, 1000.0);
        // 5 kHz should be strongly attenuated by a 1 kHz lowpass
        let orig_rms = (audio.channels[0].iter().map(|x| x * x).sum::<f32>()
            / audio.channels[0].len() as f32)
            .sqrt();
        let filt_rms = (filtered.iter().map(|x| x * x).sum::<f32>() / filtered.len() as f32).sqrt();
        assert!(filt_rms < orig_rms * 0.3, "high freq not attenuated: {orig_rms} -> {filt_rms}");
    }

    #[test]
    fn highpass_attenuates_low() {
        let audio = generate_wave(48_000, 100.0, 0.1, 0.0);
        let filtered = highpass(&audio.channels[0], 48_000, 1000.0);
        let orig_rms = (audio.channels[0].iter().map(|x| x * x).sum::<f32>()
            / audio.channels[0].len() as f32)
            .sqrt();
        let filt_rms = (filtered.iter().map(|x| x * x).sum::<f32>() / filtered.len() as f32).sqrt();
        assert!(filt_rms < orig_rms * 0.3, "low freq not attenuated: {orig_rms} -> {filt_rms}");
    }

    #[test]
    fn equalizer_boosts() {
        let sig: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.1).sin()).collect();
        let boosted = equalizer(&sig, 48_000, 1000.0, 1.0, 6.0);
        let orig_rms = (sig.iter().map(|x| x * x).sum::<f32>() / sig.len() as f32).sqrt();
        let boost_rms = (boosted.iter().map(|x| x * x).sum::<f32>() / boosted.len() as f32).sqrt();
        assert!(boost_rms > orig_rms, "peaking boost should increase RMS");
    }

    #[test]
    fn compressor_reduces_rms() {
        let mut sig = vec![0.1f32; 96000];
        sig[48000..96000].fill(0.8);
        let compressed = compressor(&sig, 48_000, -12.0, 3.0, 0.005, 0.05);
        // RMS of the loud section should drop measurably
        let loud_rms: f32 =
            (compressed[49000..96000].iter().map(|x| x * x).sum::<f32>() / 47000.0).sqrt();
        // Bypass the first 1000 samples of loud to let envelope settle.
        assert!(loud_rms < 0.55, "compressor should reduce RMS below 0.55, got {:.3}", loud_rms);
    }

    #[test]
    fn limiter_caps_peak() {
        let sig = vec![2.0f32; 1000];
        let limited = limiter(&sig, 48_000, -3.0);
        let peak = limited.iter().fold(0.0f32, |a, &x| a.max(x.abs()));
        let ceil = 10.0f32.powf(-3.0 / 20.0);
        assert!(peak <= ceil * 1.01, "limiter peak {} exceeds ceiling {}", peak, ceil);
    }

    #[test]
    fn gate_silences_quiet() {
        let mut sig = vec![0.001f32; 1000];
        sig[500..510].copy_from_slice([0.5f32; 10].as_slice());
        let gated = gate(&sig, 48_000, -40.0, 0.001, 0.01);
        // Quiet parts should be silenced (near zero)
        let pre_rms: f32 = (gated[0..400].iter().map(|x| x * x).sum::<f32>() / 400.0).sqrt();
        assert!(pre_rms < 0.001, "gate should silence quiet parts, got {}", pre_rms);
    }
}
