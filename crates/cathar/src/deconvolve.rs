//! Regularised frequency-domain measured-impulse-response deconvolution.

use rustfft::{FftPlanner, num_complex::Complex};

/// Recover a signal from a measured impulse response using Wiener-style
/// regularised inversion. The result has the same length as `signal`.
///
/// `regularization` is a non-negative fraction of the maximum IR power. A
/// small value such as `1e-4` is a useful starting point; zero performs an
/// unregularised inverse and can strongly amplify spectral nulls.
pub fn deconvolve(signal: &[f32], impulse_response: &[f32], regularization: f32) -> Vec<f32> {
    if signal.is_empty() || impulse_response.is_empty() {
        return signal.to_vec();
    }
    let convolution_len = signal.len().saturating_add(impulse_response.len()).saturating_sub(1);
    let size = convolution_len.next_power_of_two();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let ifft = planner.plan_fft_inverse(size);
    let mut x = vec![Complex::new(0.0, 0.0); size];
    let mut h = vec![Complex::new(0.0, 0.0); size];
    for (dst, &v) in x.iter_mut().zip(signal) {
        dst.re = v;
    }
    for (dst, &v) in h.iter_mut().zip(impulse_response) {
        dst.re = v;
    }
    fft.process(&mut x);
    fft.process(&mut h);
    let max_power = h.iter().map(|v| v.norm_sqr()).fold(0.0, f32::max);
    let floor = regularization.max(0.0) * max_power;
    for (xk, hk) in x.iter_mut().zip(&h) {
        let denom = hk.norm_sqr() + floor;
        *xk = if denom > f32::EPSILON { *xk * hk.conj() / denom } else { Complex::new(0.0, 0.0) };
    }
    ifft.process(&mut x);
    let scale = 1.0 / size as f32;
    x.into_iter().take(signal.len()).map(|v| v.re * scale).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn impulse_response_identity_is_near_identity() {
        let x: Vec<f32> = (0..257).map(|i| (i as f32 * 0.17).sin()).collect();
        let y = deconvolve(&x, &[1.0], 1e-6);
        let err = x.iter().zip(y).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-4, "identity error {err}");
    }
    #[test]
    fn recovers_delayed_impulse() {
        let mut x = vec![0.0; 64];
        x[0] = 1.0;
        let ir = [0.5, 0.25];
        let y = deconvolve(&x, &ir, 1e-6);
        assert!(y[0] > 0.9, "recovered impulse {}", y[0]);
    }
}
