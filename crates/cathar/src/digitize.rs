//! Vinyl digitization: RIAA de-emphasis and elliptical mono summing.

use crate::filter::lowpass;

/// Second-order IIR (biquad) in Direct Form I.
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
}

/// RIAA playback (de-emphasis) biquad coefficients matched at common sample
/// rates (jtp / musicdsp reproduction filters, ±0.15 dB at 48 kHz).
fn riaa_biquad(sample_rate: u32) -> Biquad {
    let (b0, b1, b2, a1, a2) = match sample_rate {
        44_100 => (1.0, -0.721_892_2, -0.186_052_05, -1.700_724, 0.702_938_15),
        48_000 => (1.0, -0.755_552_1, -0.164_625_71, -1.732_765_5, 0.734_553_44),
        88_200 => (1.0, -0.847_957_7, -0.112_763_2, -1.855_464_8, 0.855_972_14),
        96_000 => (1.0, -0.853_533_1, -0.110_459_51, -1.866_608_3, 0.867_038_3),
        _ => unreachable!("fallback rates use `riaa_fallback`"),
    };
    Biquad { b0, b1, b2, a1, a2, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
}

/// First-order section `(1 + τₙ·s) / (1 + τₐ·s)`; `τₙ = None` → unity numerator.
#[derive(Debug, Clone, Copy)]
struct FirstOrder {
    b0: f32,
    b1: f32,
    a1: f32,
    x1: f32,
    y1: f32,
}

impl FirstOrder {
    fn design(num_tau: Option<f32>, den_tau: f32, sample_rate: f32) -> Self {
        let k = 2.0 * sample_rate;
        let (n0, n1) = match num_tau {
            Some(tau) => (1.0 + k * tau, 1.0 - k * tau),
            None => (1.0, 1.0),
        };
        let d0 = 1.0 + k * den_tau;
        let d1 = 1.0 - k * den_tau;
        Self { b0: n0 / d0, b1: n1 / d0, a1: d1 / d0, x1: 0.0, y1: 0.0 }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

const RIAA_T1: f32 = 3180e-6;
const RIAA_T2: f32 = 318e-6;
const RIAA_T3: f32 = 75e-6;

fn riaa_fallback(signal: &[f32], sample_rate: u32) -> Vec<f32> {
    let fs = sample_rate as f32;
    let mut s1 = FirstOrder::design(Some(RIAA_T1), RIAA_T2, fs);
    let mut s2 = FirstOrder::design(None, RIAA_T3, fs);
    signal.iter().map(|&x| s2.process(s1.process(x))).collect()
}

/// Apply the standard RIAA playback (de-emphasis) curve to a mono signal.
///
/// Digitized vinyl is captured with RIAA pre-emphasis on the cutter head; this
/// inverts that curve so the output is flat. Gain is normalised so 1 kHz is
/// 0 dB reference.
pub fn riaa_deemphasis(signal: &[f32], sample_rate: u32) -> Vec<f32> {
    if signal.is_empty() {
        return Vec::new();
    }
    let mut out = match sample_rate {
        44_100 | 48_000 | 88_200 | 96_000 => {
            let mut f = riaa_biquad(sample_rate);
            signal.iter().map(|&x| f.process(x)).collect()
        }
        _ => riaa_fallback(signal, sample_rate),
    };
    // jtp coefficients are ~+12 dB hot; normalise to 0 dB at 1 kHz.
    let gain = riaa_gain_at_1khz(sample_rate);
    if gain > 1e-6 {
        let scale = 1.0 / gain;
        for s in out.iter_mut() {
            *s *= scale;
        }
    }
    out
}

/// Sum low frequencies to mono below `crossover_hz` while preserving the stereo
/// image above the crossover. Tames out-of-phase vinyl rumble without collapsing
/// the full mix.
pub fn elliptical_mono(
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    crossover_hz: f32,
) -> (Vec<f32>, Vec<f32>) {
    let n = left.len().min(right.len());
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    let low_l = lowpass(&left[..n], sample_rate, crossover_hz);
    let low_r = lowpass(&right[..n], sample_rate, crossover_hz);
    let mut out_l = Vec::with_capacity(n);
    let mut out_r = Vec::with_capacity(n);
    for i in 0..n {
        let mono_low = (low_l[i] + low_r[i]) * 0.5;
        out_l.push(left[i] - low_l[i] + mono_low);
        out_r.push(right[i] - low_r[i] + mono_low);
    }
    (out_l, out_r)
}

/// RIAA de-emphasis with optional elliptical mono on a stereo pair.
pub fn vinyl_restore(
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    elliptical_hz: Option<f32>,
) -> (Vec<f32>, Vec<f32>) {
    let mut l = riaa_deemphasis(left, sample_rate);
    let mut r = riaa_deemphasis(right, sample_rate);
    if let Some(crossover) = elliptical_hz {
        (l, r) = elliptical_mono(&l, &r, sample_rate, crossover);
    }
    (l, r)
}

fn riaa_gain_at_1khz(sample_rate: u32) -> f32 {
    let fs = sample_rate;
    let n = fs as usize * 2;
    let tone: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
        .collect();
    let out = riaa_deemphasis_unscaled(&tone, fs);
    let peak = |s: &[f32]| s.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
    peak(&out[n / 2..]) / peak(&tone[n / 2..]).max(1e-10)
}

fn riaa_deemphasis_unscaled(signal: &[f32], sample_rate: u32) -> Vec<f32> {
    match sample_rate {
        44_100 | 48_000 | 88_200 | 96_000 => {
            let mut f = riaa_biquad(sample_rate);
            signal.iter().map(|&x| f.process(x)).collect()
        }
        _ => riaa_fallback(signal, sample_rate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riaa_is_unity_at_1khz() {
        let fs = 48_000u32;
        let n = fs as usize * 2;
        let tone: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
            .collect();
        let out = riaa_deemphasis(&tone, fs);
        let rms = |s: &[f32]| {
            let tail = &s[s.len() / 2..];
            (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let ratio = rms(&out) / rms(&tone);
        assert!((ratio - 1.0).abs() < 0.15, "1 kHz should be near unity, ratio={ratio}");
    }

    #[test]
    fn riaa_boosts_bass_relative_to_mids() {
        let fs = 48_000u32;
        let n = fs as usize * 4;
        let tone = |f: f32| {
            (0..n)
                .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin())
                .collect::<Vec<_>>()
        };
        let rms = |s: &[f32]| {
            let tail = &s[s.len() / 2..];
            (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let bass_out = riaa_deemphasis(&tone(80.0), fs);
        let mid_out = riaa_deemphasis(&tone(1000.0), fs);
        assert!(rms(&bass_out) > rms(&mid_out) * 1.2, "RIAA should lift bass vs 1 kHz");
    }

    #[test]
    fn elliptical_collapses_lows_to_mono() {
        let fs = 48_000u32;
        let n = fs as usize * 2;
        let t = |f: f32, i: usize| (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin();
        // In-phase lows should collapse to a shared mono component.
        let left: Vec<f32> = (0..n).map(|i| t(60.0, i)).collect();
        let right: Vec<f32> = (0..n).map(|i| t(60.0, i) * 0.8).collect();
        let (l, r) = elliptical_mono(&left, &right, fs, 200.0);
        let out_low_l = lowpass(&l, fs, 200.0);
        let out_low_r = lowpass(&r, fs, 200.0);
        let diff: f32 =
            out_low_l.iter().zip(&out_low_r).map(|(a, b)| (a - b).abs()).sum::<f32>() / n as f32;
        assert!(diff < 0.08, "low band should be mono, mean |L-R|={diff}");
    }
}
