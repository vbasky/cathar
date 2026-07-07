//! Analog pre-emphasis decode — apply the standard playback de-emphasis curve
//! to sources recorded with high-frequency pre-emphasis.
//!
//! - **FM broadcast** — 50 µs (ITU-R / Europe) or 75 µs (Americas/Japan), a
//!   single-pole treble roll-off.
//! - **CD / IEC** — the optional 50/15 µs pre-emphasis flagged in some early CDs;
//!   a first-order shelf (pole 50 µs, zero 15 µs).
//!
//! Each is an exact first-order bilinear section; unity gain at DC. Companded
//! systems (Dolby B/C, dbx) are a separate, later addition.

/// A standard de-emphasis curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// FM broadcast, 50 µs time constant.
    Fm50,
    /// FM broadcast, 75 µs time constant.
    Fm75,
    /// CD / IEC 60908 optional pre-emphasis (50 µs pole, 15 µs zero).
    CdIec,
}

/// Apply the playback de-emphasis `curve` to a mono signal.
pub fn deemphasis(signal: &[f32], sample_rate: u32, curve: Emphasis) -> Vec<f32> {
    if signal.is_empty() || sample_rate == 0 {
        return signal.to_vec();
    }
    let (num_tau, den_tau) = match curve {
        Emphasis::Fm50 => (None, 50e-6f32),
        Emphasis::Fm75 => (None, 75e-6f32),
        Emphasis::CdIec => (Some(15e-6f32), 50e-6f32),
    };
    let k = 2.0 * sample_rate as f32;
    let (n0, n1) = match num_tau {
        Some(tau) => (1.0 + k * tau, 1.0 - k * tau),
        None => (1.0, 1.0),
    };
    let d0 = 1.0 + k * den_tau;
    let d1 = 1.0 - k * den_tau;
    let (b0, b1, a1) = (n0 / d0, n1 / d0, d1 / d0);

    let mut out = Vec::with_capacity(signal.len());
    let (mut x1, mut y1) = (0.0f32, 0.0f32);
    for &x in signal {
        let y = b0 * x + b1 * x1 - a1 * y1;
        out.push(y);
        x1 = x;
        y1 = y;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin()).collect()
    }
    fn rms(x: &[f32]) -> f32 {
        (x[x.len() / 2..].iter().map(|v| v * v).sum::<f32>() / (x.len() / 2) as f32).sqrt()
    }

    #[test]
    fn fm_rolls_off_highs_keeps_lows() {
        let sr = 48_000u32;
        for curve in [Emphasis::Fm50, Emphasis::Fm75] {
            let low = deemphasis(&tone(100.0, sr, sr as usize), sr, curve);
            let high = deemphasis(&tone(15_000.0, sr, sr as usize), sr, curve);
            // Low frequencies pass ~unity; highs are strongly attenuated.
            assert!((rms(&low) - 0.707).abs() < 0.05, "low gain {}", rms(&low));
            assert!(rms(&high) < rms(&low) * 0.4, "high not rolled off: {}", rms(&high));
        }
    }

    #[test]
    fn cd_shelf_attenuates_highs() {
        let sr = 48_000u32;
        let low = deemphasis(&tone(100.0, sr, sr as usize), sr, Emphasis::CdIec);
        let high = deemphasis(&tone(15_000.0, sr, sr as usize), sr, Emphasis::CdIec);
        // 50/15 µs shelf → ~−10.5 dB (×0.3) plateau at high frequencies.
        assert!(rms(&high) < rms(&low) * 0.5, "shelf not applied: {} vs {}", rms(&high), rms(&low));
        assert!(rms(&high) > rms(&low) * 0.15, "shelf over-attenuated");
    }
}
