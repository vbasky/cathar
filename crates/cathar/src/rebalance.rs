//! Deterministic long-term spectral matching against a reference recording.

use crate::util::hann_window;
use realfft::RealFftPlanner;

/// Match a signal's long-term spectral envelope to `reference`.
///
/// `strength` in `[0, 1]` controls how much of the measured correction is
/// applied. Gains are limited to ±12 dB to avoid turning reference silence or
/// unrelated narrow notches into unstable boosts.
pub fn spectral_rebalance(
    signal: &[f32],
    reference: &[f32],
    sample_rate: u32,
    strength: f32,
) -> Vec<f32> {
    const FFT: usize = 2048;
    const HOP: usize = 512;
    if signal.len() < FFT || reference.len() < FFT || sample_rate == 0 {
        return signal.to_vec();
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT);
    let c2r = planner.plan_fft_inverse(FFT);
    let win = hann_window(FFT);
    let bins = FFT / 2 + 1;
    let average = |input: &[f32]| {
        let mut sum = vec![0.0; bins];
        let mut count = 0usize;
        let mut x = r2c.make_input_vec();
        let mut f = r2c.make_output_vec();
        let mut pos = 0;
        while pos + FFT <= input.len() {
            for i in 0..FFT {
                x[i] = input[pos + i] * win[i];
            }
            r2c.process(&mut x, &mut f).expect("FFT buffer sizes are fixed");
            for (s, z) in sum.iter_mut().zip(&f) {
                *s += z.norm();
            }
            count += 1;
            pos += HOP;
        }
        sum.into_iter().map(|v| v / count.max(1) as f32).collect::<Vec<_>>()
    };
    let target = average(reference);
    let source = average(signal);
    let strength = strength.clamp(0.0, 1.0);
    let gains: Vec<f32> = source
        .iter()
        .zip(target)
        .map(|(&s, t)| ((t.max(1e-6) / s.max(1e-6)).ln() * strength).exp().clamp(0.25, 4.0))
        .collect();
    let mut output = vec![0.0; signal.len()];
    let mut weights = vec![0.0; signal.len()];
    let mut x = r2c.make_input_vec();
    let mut f = r2c.make_output_vec();
    let mut pos = 0;
    while pos + FFT <= signal.len() {
        for i in 0..FFT {
            x[i] = signal[pos + i] * win[i];
        }
        r2c.process(&mut x, &mut f).expect("FFT buffer sizes are fixed");
        for (z, &g) in f.iter_mut().zip(&gains) {
            z.re *= g;
            z.im *= g;
        }
        f[0].im = 0.0;
        f[bins - 1].im = 0.0;
        c2r.process(&mut f, &mut x).expect("inverse FFT buffer sizes are fixed");
        for i in 0..FFT {
            output[pos + i] += x[i] * win[i] / FFT as f32;
            weights[pos + i] += win[i] * win[i];
        }
        pos += HOP;
    }
    output
        .into_iter()
        .zip(weights)
        .enumerate()
        .map(|(i, (v, w))| if w > 1e-8 { v / w } else { signal[i] })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_reference_is_stable() {
        let sr = 48_000u32;
        let x: Vec<f32> = (0..sr as usize).map(|i| (i as f32 * 0.13).sin() * 0.2).collect();
        let y = spectral_rebalance(&x, &x, sr, 1.0);
        assert!(y.iter().all(|v| v.is_finite()));
        assert!(
            y.iter().zip(&x).map(|(a, b)| (a - b).abs()).sum::<f32>() / (x.len() as f32) < 1e-3
        );
    }
}
