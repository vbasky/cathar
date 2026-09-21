//! Dequantization: reduce grain from low-bit-depth sources.

/// Reduce quantization grain from a signal assumed to have been recorded at
/// `bits` bits per sample.
///
/// Uses neighbour linear prediction: each sample is nudged toward the local
/// midpoint of its neighbours while staying within one quantisation step of its
/// lattice position. `strength` in `[0, 1]` controls correction (0 = bypass).
pub fn dequantize(signal: &[f32], _sample_rate: u32, bits: u32, strength: f32) -> Vec<f32> {
    let strength = strength.clamp(0.0, 1.0);
    let n = signal.len();
    if strength <= 0.0 || n < 3 {
        return signal.to_vec();
    }
    let bits = bits.clamp(4, 24);
    let step = 2.0f32 / (1u32 << bits) as f32;
    let half = step * 0.5;

    let mut out = signal.to_vec();
    for i in 1..n - 1 {
        let q = (out[i] / step).round() * step;
        let pred = (out[i - 1] + out[i + 1]) * 0.5;
        let target = pred.clamp(q - half, q + half);
        out[i] += strength * (target - out[i]);
    }

    // Second pass with a wider window for low-frequency content.
    if strength > 0.3 && n >= 5 {
        let src = out.clone();
        for i in 2..n - 2 {
            let q = (src[i] / step).round() * step;
            let pred = (src[i - 2] + src[i - 1] + src[i + 1] + src[i + 2]) * 0.25;
            let target = pred.clamp(q - half, q + half);
            out[i] += strength * 0.5 * (target - out[i]);
        }
    }
    out
}

/// Co-sparse depth pass for quantized audio.
///
/// Alternates projections onto the quantizer cells with first- and
/// second-order analysis priors. The cubic local predictor handles slowly
/// varying tones better than the lattice-only midpoint predictor, while the
/// cell constraint prevents invented samples from crossing quantization bins.
/// This is a deterministic, dependency-free approximation of co-sparse
/// recovery; `iterations` in the range 2–6 is usually sufficient.
pub fn dequantize_cosparse(
    signal: &[f32],
    _sample_rate: u32,
    bits: u32,
    strength: f32,
    iterations: u32,
) -> Vec<f32> {
    let strength = strength.clamp(0.0, 1.0);
    if signal.len() < 5 || strength <= 0.0 || iterations == 0 {
        return signal.to_vec();
    }
    let bits = bits.clamp(4, 24);
    let step = 2.0f32 / (1u32 << bits) as f32;
    let half = step * 0.5;
    let mut out = signal.to_vec();
    for _ in 0..iterations.min(16) {
        let src = out.clone();
        for i in 2..src.len() - 2 {
            let q = (src[i] / step).round() * step;
            let linear = (src[i - 1] + src[i + 1]) * 0.5;
            let cubic = (-src[i - 2] + 4.0 * src[i - 1] + 4.0 * src[i + 1] - src[i + 2]) / 6.0;
            let d1 = (src[i + 1] - src[i - 1]).abs();
            let d2 = (src[i + 2] - 2.0 * src[i + 1] + 2.0 * src[i - 1] - src[i - 2]).abs();
            let cubic_weight = (1.0 / (1.0 + 32.0 * d2)).clamp(0.1, 1.0);
            let candidate = (linear + cubic * cubic_weight) / (1.0 + cubic_weight);
            let target = candidate.clamp(q - half, q + half);
            let local_strength = strength * (0.5 + 0.5 / (1.0 + 16.0 * d1));
            out[i] = src[i] + local_strength * (target - src[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quantize(signal: &[f32], bits: u32) -> Vec<f32> {
        let step = 2.0f32 / (1u32 << bits) as f32;
        signal.iter().map(|s| (s / step).round() * step).collect()
    }

    #[test]
    fn dequantize_reduces_quantization_error() {
        let fs = 48_000u32;
        let n = fs as usize * 2;
        let clean: Vec<f32> = (0..n)
            .map(|i| {
                0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / fs as f32).sin()
                    + 0.3 * (2.0 * std::f32::consts::PI * 880.0 * i as f32 / fs as f32).sin()
            })
            .collect();
        let noisy = quantize(&clean, 6);
        let restored = dequantize(&noisy, fs, 6, 1.0);
        let err = |a: &[f32], b: &[f32]| {
            a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len() as f32
        };
        assert!(
            err(&restored, &clean) < err(&noisy, &clean),
            "dequantize should move closer to the clean signal"
        );
    }

    #[test]
    fn dequantize_zero_strength_is_bypass() {
        let sig = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(dequantize(&sig, 48_000, 16, 0.0), sig);
    }

    #[test]
    fn cosparse_reduces_quantization_error() {
        let sr = 48_000u32;
        let clean: Vec<f32> = (0..sr as usize)
            .map(|i| {
                let t = i as f32 / sr as f32;
                0.6 * (2.0 * std::f32::consts::PI * 330.0 * t).sin()
            })
            .collect();
        let noisy = quantize(&clean, 6);
        let restored = dequantize_cosparse(&noisy, sr, 6, 1.0, 4);
        let err = |a: &[f32]| a.iter().zip(&clean).map(|(x, y)| (x - y).powi(2)).sum::<f32>();
        assert!(err(&restored) < err(&noisy));
    }
}
