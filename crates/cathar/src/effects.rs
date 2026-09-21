//! Deterministic creative effects: delay, modulation, tremolo and drive.

fn delay_samples(ms: f32, sr: u32) -> usize {
    (ms.max(0.0) * sr as f32 / 1000.0).round() as usize
}

/// Add a delayed copy with feedback. `mix` is the first-echo level.
pub fn echo(signal: &[f32], sample_rate: u32, delay_ms: f32, feedback: f32, mix: f32) -> Vec<f32> {
    let d = delay_samples(delay_ms, sample_rate);
    if signal.is_empty() || d == 0 {
        return signal.to_vec();
    }
    let fb = feedback.clamp(-0.99, 0.99);
    let mix = mix.clamp(0.0, 1.0);
    let mut out = signal.to_vec();
    for i in d..out.len() {
        out[i] += signal[i - d] * mix + (out[i - d] - signal[i - d]) * fb;
    }
    out
}

/// Add a compact Schroeder-style synthetic room response.
pub fn reverb(signal: &[f32], sample_rate: u32, room: f32, mix: f32) -> Vec<f32> {
    if signal.is_empty() || sample_rate == 0 {
        return signal.to_vec();
    }
    let room = room.clamp(0.0, 1.0);
    let mix = mix.clamp(0.0, 1.0);
    let taps = [(29.7, 0.72), (37.1, 0.63), (41.1, 0.56), (43.7, 0.49)];
    let mut wet = vec![0.0; signal.len()];
    for (ms, gain) in taps {
        let d = delay_samples(ms, sample_rate).max(1);
        for i in d..signal.len() {
            wet[i] += signal[i - d] * gain * (0.25 + 0.7 * room);
        }
    }
    signal.iter().zip(wet).map(|(&dry, wet)| dry * (1.0 - mix) + wet * mix).collect()
}

/// Apply a sinusoidally modulated delay suitable for chorus.
pub fn chorus(signal: &[f32], sample_rate: u32, rate_hz: f32, depth_ms: f32, mix: f32) -> Vec<f32> {
    mod_delay(signal, sample_rate, rate_hz, depth_ms, 0.0, mix)
}

/// Apply a short modulated delay with regenerative feedback.
pub fn flanger(
    signal: &[f32],
    sample_rate: u32,
    rate_hz: f32,
    depth_ms: f32,
    feedback: f32,
    mix: f32,
) -> Vec<f32> {
    mod_delay(signal, sample_rate, rate_hz, depth_ms, feedback, mix)
}

fn mod_delay(
    signal: &[f32],
    sr: u32,
    rate: f32,
    depth_ms: f32,
    feedback: f32,
    mix: f32,
) -> Vec<f32> {
    if signal.is_empty() || sr == 0 || rate <= 0.0 || depth_ms <= 0.0 {
        return signal.to_vec();
    }
    let base = (depth_ms * sr as f32 / 2000.0).max(1.0);
    let fb = feedback.clamp(-0.99, 0.99);
    let mix = mix.clamp(0.0, 1.0);
    let mut history = vec![0.0; signal.len()];
    let mut out = Vec::with_capacity(signal.len());
    for (i, &x) in signal.iter().enumerate() {
        let lfo = (2.0 * std::f32::consts::PI * rate * i as f32 / sr as f32).sin();
        let d = base * (1.0 + lfo);
        let delayed = if i as f32 > d {
            let p = i as f32 - d;
            let k = p.floor() as usize;
            let f = p - k as f32;
            history[k] * (1.0 - f) + history[(k + 1).min(i - 1)] * f
        } else {
            0.0
        };
        let y = x * (1.0 - mix) + delayed * mix;
        history[i] = x + delayed * fb;
        out.push(y);
    }
    out
}

/// Apply four first-order all-pass stages modulated by a low-frequency oscillator.
pub fn phaser(
    signal: &[f32],
    sample_rate: u32,
    rate_hz: f32,
    depth: f32,
    feedback: f32,
    mix: f32,
) -> Vec<f32> {
    if signal.is_empty() || sample_rate == 0 {
        return signal.to_vec();
    }
    let mut state = [0.0; 4];
    let mut out = Vec::with_capacity(signal.len());
    let mix = mix.clamp(0.0, 1.0);
    let fb = feedback.clamp(-0.99, 0.99);
    for (i, &x) in signal.iter().enumerate() {
        let lfo =
            (2.0 * std::f32::consts::PI * rate_hz.max(0.01) * i as f32 / sample_rate as f32).sin();
        let a = (0.1 + depth.clamp(0.0, 1.0) * 0.85 * (lfo + 1.0) * 0.5).clamp(0.01, 0.97);
        let mut y = x + state[3] * fb;
        for s in &mut state {
            let z = -a * y + *s;
            *s = y + a * z;
            y = z;
        }
        out.push(x * (1.0 - mix) + y * mix);
    }
    out
}

/// Apply sinusoidal amplitude modulation. `depth` is in `[0, 1]`.
pub fn tremolo(signal: &[f32], sample_rate: u32, rate_hz: f32, depth: f32) -> Vec<f32> {
    if signal.is_empty() || sample_rate == 0 || rate_hz <= 0.0 {
        return signal.to_vec();
    }
    let depth = depth.clamp(0.0, 1.0);
    signal
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let lfo = (2.0 * std::f32::consts::PI * rate_hz * i as f32 / sample_rate as f32).sin();
            x * (1.0 - depth * 0.5 * (lfo + 1.0))
        })
        .collect()
}

/// Apply symmetric tanh soft clipping. `drive=0` is an exact bypass.
pub fn overdrive(signal: &[f32], drive: f32, mix: f32) -> Vec<f32> {
    if signal.is_empty() || drive <= 0.0 {
        return signal.to_vec();
    }
    let gain = 1.0 + drive * 8.0;
    let norm = gain.tanh();
    let mix = mix.clamp(0.0, 1.0);
    signal.iter().map(|&x| x * (1.0 - mix) + (gain * x).tanh() / norm * mix).collect()
}

/// Apply a static broadband compressor/expander curve in dB.
pub fn compand(signal: &[f32], threshold_db: f32, ratio: f32, makeup_db: f32) -> Vec<f32> {
    let ratio = ratio.max(1.0);
    let makeup = 10.0f32.powf(makeup_db / 20.0);
    signal
        .iter()
        .map(|&x| {
            let db = 20.0 * x.abs().max(1e-12).log10();
            let y = if db > threshold_db { threshold_db + (db - threshold_db) / ratio } else { db };
            x.signum() * 10.0f32.powf(y / 20.0) * makeup
        })
        .collect()
}

/// Apply a time-varying compressor with attack/release envelope following.
/// Unlike [`compand`], gain does not jump at sample boundaries. `attack` and
/// `release` are seconds; `ratio` must be at least 1.
pub fn compand_dynamic(
    signal: &[f32],
    sample_rate: u32,
    threshold_db: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    makeup_db: f32,
) -> Vec<f32> {
    if signal.is_empty() || sample_rate == 0 {
        return signal.to_vec();
    }
    let ratio = ratio.max(1.0);
    let attack_coeff = (-1.0 / (attack.max(1e-5) * sample_rate as f32)).exp();
    let release_coeff = (-1.0 / (release.max(1e-5) * sample_rate as f32)).exp();
    let makeup = 10.0f32.powf(makeup_db / 20.0);
    let mut envelope = 0.0;
    signal
        .iter()
        .map(|&x| {
            let level = x.abs();
            let coeff = if level > envelope { attack_coeff } else { release_coeff };
            envelope = coeff * envelope + (1.0 - coeff) * level;
            let db = 20.0 * envelope.max(1e-12).log10();
            let reduction =
                if db > threshold_db { (db - threshold_db) * (1.0 - 1.0 / ratio) } else { 0.0 };
            x * 10.0f32.powf(-reduction / 20.0) * makeup
        })
        .collect()
}

/// Change waveform contrast around zero while keeping the result bounded.
pub fn contrast(signal: &[f32], amount: f32) -> Vec<f32> {
    let amount = amount.clamp(-1.0, 1.0);
    let k = 1.0 + amount * 4.0;
    signal.iter().map(|&x| (x * k).tanh() / k.tanh()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bypasses_are_exact() {
        let x = vec![0.1, -0.2, 0.3];
        assert_eq!(echo(&x, 48_000, 20.0, 0.5, 0.0), x);
        assert_eq!(overdrive(&x, 0.0, 1.0), x);
    }
    #[test]
    fn effects_are_finite() {
        let x = vec![0.2; 2048];
        for y in [
            reverb(&x, 48_000, 0.8, 1.0),
            chorus(&x, 48_000, 0.4, 8.0, 1.0),
            flanger(&x, 48_000, 0.4, 3.0, 0.5, 1.0),
            phaser(&x, 48_000, 0.3, 0.8, 0.4, 1.0),
            tremolo(&x, 48_000, 4.0, 0.8),
            overdrive(&x, 2.0, 1.0),
            compand(&x, -20.0, 2.0, 0.0),
            compand_dynamic(&x, 48_000, -20.0, 2.0, 0.001, 0.1, 0.0),
            contrast(&x, 0.5),
        ] {
            assert!(y.iter().all(|v| v.is_finite()));
        }
    }
}
