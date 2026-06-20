//! Anti-aliased sample-rate conversion (Kaiser-windowed sinc).

/// Modified Bessel function of the first kind, order 0 — for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0f64;
    let mut term = 1.0f64;
    let half_sq = (x / 2.0) * (x / 2.0);
    for k in 1..=30 {
        term *= half_sq / (k as f64 * k as f64);
        sum += term;
        if term < 1e-13 * sum {
            break;
        }
    }
    sum
}

/// Resample one channel from `from_rate` to `to_rate` with a Kaiser-windowed
/// sinc (arbitrary ratio). The cutoff tracks the lower of the two Nyquist
/// limits, so downsampling is anti-aliased and upsampling adds no imaging;
/// the filter support widens at low cutoffs to keep the stopband sharp. Returns
/// the input unchanged when the rates already match.
pub fn resample(signal: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || from_rate == 0 || to_rate == 0 || signal.is_empty() {
        return signal.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (signal.len() as f64 * ratio).round().max(1.0) as usize;

    // Cutoff as a fraction of the source rate (≤ 0.5); downsampling lowers it to
    // the destination Nyquist.
    let cutoff = 0.5 * ratio.min(1.0);
    // Fixed number of sinc lobes per side; support (in source samples) grows as
    // the cutoff falls so the filter always spans the same zero crossings.
    const LOBES: f64 = 16.0;
    let half_width = LOBES / (2.0 * cutoff);
    let beta = 9.0;
    let i0_beta = bessel_i0(beta);

    let n = signal.len() as isize;
    let mut out = vec![0.0f32; out_len];
    for (i, o) in out.iter_mut().enumerate() {
        let center = i as f64 / ratio; // position in source samples
        let first = (center - half_width).ceil() as isize;
        let last = (center + half_width).floor() as isize;
        let mut acc = 0.0f64;
        let mut norm = 0.0f64;
        for idx in first..=last {
            if idx < 0 || idx >= n {
                continue;
            }
            let dx = center - idx as f64;
            // Low-pass sinc 2·fc·sinc(2·fc·dx) = sin(π·t)/(π·dx), t = 2·fc·dx.
            let t = 2.0 * cutoff * dx;
            let sinc = if dx.abs() < 1e-9 {
                2.0 * cutoff
            } else {
                (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * dx)
            };
            // Kaiser window over |dx| ≤ half_width.
            let r = dx / half_width;
            let w =
                if r.abs() < 1.0 { bessel_i0(beta * (1.0 - r * r).sqrt()) / i0_beta } else { 0.0 };
            let k = sinc * w;
            acc += signal[idx as usize] as f64 * k;
            norm += k;
        }
        *o = (acc / norm.max(1e-12)) as f32;
    }
    out
}
