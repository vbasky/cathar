//! Loudness (ITU-R BS.1770-4 / EBU R128) and peak normalisation.

/// Scale to target peak level in dBFS (0 dBFS = ±1.0, -3 dBFS = ~±0.707).
pub fn normalize_peak(signal: &[f32], target_dbfs: f32) -> Vec<f32> {
    let peak = signal.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
    if peak < 1e-10 {
        return signal.to_vec();
    }
    let target_linear = 10.0f32.powf(target_dbfs / 20.0);
    let gain = target_linear / peak;
    signal.iter().map(|s| s * gain).collect()
}

// ── Loudness (ITU-R BS.1770-4 / EBU R128) ────────────────────────────────────

/// One stage of the K-weighting filter: a normalised biquad (a0 = 1).
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    /// Direct-form II transposed, with f64 state for numerical accuracy.
    fn apply(&self, x: &[f64]) -> Vec<f64> {
        let mut s1 = 0.0f64;
        let mut s2 = 0.0f64;
        let mut out = Vec::with_capacity(x.len());
        for &xn in x {
            let yn = self.b0 * xn + s1;
            s1 = self.b1 * xn - self.a1 * yn + s2;
            s2 = self.b2 * xn - self.a2 * yn;
            out.push(yn);
        }
        out
    }
}

/// K-weighting stage 1 — the high-shelf "pre-filter", recomputed for `fs`.
fn kweight_stage1(fs: f64) -> Biquad {
    let f0 = 1681.974450955533;
    let g = 3.999843853973347;
    let q = 0.7071752369554196;
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// K-weighting stage 2 — the RLB high-pass, recomputed for `fs`.
fn kweight_stage2(fs: f64) -> Biquad {
    let f0 = 38.13547087602444;
    let q = 0.5003270373238773;
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// Measure integrated loudness in LUFS per ITU-R BS.1770-4 / EBU R128:
/// K-weighting, 400 ms blocks at 75 % overlap, the -70 LUFS absolute gate,
/// then the -10 LU relative gate.
///
/// Loudness is summed across all channels jointly (channel weight 1.0 — exact
/// for mono and stereo; surround channels are not up-weighted because channel
/// layout is not tracked). Returns [`f32::NEG_INFINITY`] for silence or empty
/// input.
pub fn integrated_loudness(channels: &[Vec<f32>], sample_rate: u32) -> f32 {
    let fs = sample_rate as f64;
    let n = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    if n == 0 || fs <= 0.0 {
        return f32::NEG_INFINITY;
    }
    let s1 = kweight_stage1(fs);
    let s2 = kweight_stage2(fs);
    let weighted: Vec<Vec<f64>> = channels
        .iter()
        .map(|c| {
            let f64ch: Vec<f64> = c.iter().map(|&x| x as f64).collect();
            s2.apply(&s1.apply(&f64ch))
        })
        .collect();

    let block = ((0.4 * fs).round() as usize).clamp(1, n);
    let step = ((0.1 * fs).round() as usize).max(1);

    // Per-block: weighted mean-square summed across channels.
    let mut blocks: Vec<f64> = Vec::new();
    let mut start = 0;
    while start + block <= n {
        let mut sum = 0.0f64;
        for ch in &weighted {
            sum += ch[start..start + block].iter().map(|v| v * v).sum::<f64>() / block as f64;
        }
        blocks.push(sum);
        start += step;
    }
    if blocks.is_empty() {
        return f32::NEG_INFINITY;
    }

    let loudness = |ms: f64| -0.691 + 10.0 * ms.log10();

    // Absolute gate at -70 LUFS.
    let abs_gated: Vec<f64> =
        blocks.iter().copied().filter(|&ms| ms > 0.0 && loudness(ms) >= -70.0).collect();
    if abs_gated.is_empty() {
        return f32::NEG_INFINITY;
    }

    // Relative gate at -10 LU below the mean of the absolute-gated blocks.
    let mean_abs = abs_gated.iter().sum::<f64>() / abs_gated.len() as f64;
    let rel_threshold = loudness(mean_abs) - 10.0;
    let rel_gated: Vec<f64> =
        abs_gated.iter().copied().filter(|&ms| loudness(ms) >= rel_threshold).collect();
    let kept = if rel_gated.is_empty() { &abs_gated } else { &rel_gated };

    let mean = kept.iter().sum::<f64>() / kept.len() as f64;
    loudness(mean) as f32
}

/// Build `os` windowed-sinc sub-filters (one per fractional phase), each
/// normalised to unity DC gain.
fn polyphase_kernels(os: usize, half: usize) -> Vec<Vec<f64>> {
    let taps = 2 * half;
    (0..os)
        .map(|p| {
            let frac = p as f64 / os as f64;
            let mut ker = vec![0.0f64; taps];
            let mut sum = 0.0f64;
            for (k, slot) in ker.iter_mut().enumerate() {
                let arg = (k as f64 - half as f64 + 1.0) - frac;
                let sinc = if arg.abs() < 1e-9 {
                    1.0
                } else {
                    (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
                };
                // Hann window over the [-half, half) support.
                let w = 0.5 * (1.0 + (std::f64::consts::PI * arg / half as f64).cos());
                let v = sinc * w;
                *slot = v;
                sum += v;
            }
            if sum.abs() > 1e-12 {
                for v in &mut ker {
                    *v /= sum;
                }
            }
            ker
        })
        .collect()
}

/// Estimate true-peak level in dBTP via 4× polyphase oversampling (the
/// inter-sample-peak method of ITU-R BS.1770-4). Returns [`f32::NEG_INFINITY`]
/// for digital silence. Oversampling is fixed at 4×, independent of sample rate.
pub fn true_peak_dbtp(channels: &[Vec<f32>], _sample_rate: u32) -> f32 {
    const OS: usize = 4;
    const HALF: usize = 8;
    let kernels = polyphase_kernels(OS, HALF);

    let mut peak = 0.0f64;
    for ch in channels {
        let len = ch.len() as isize;
        for i in 0..len {
            for ker in &kernels {
                let mut acc = 0.0f64;
                for (k, w) in ker.iter().enumerate() {
                    let idx = i + k as isize - HALF as isize + 1;
                    if idx >= 0 && idx < len {
                        acc += ch[idx as usize] as f64 * w;
                    }
                }
                peak = peak.max(acc.abs());
            }
        }
    }
    if peak <= 0.0 { f32::NEG_INFINITY } else { 20.0 * (peak as f32).log10() }
}
