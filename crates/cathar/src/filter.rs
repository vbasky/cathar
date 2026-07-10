//! Biquad filters and dynamics processing (compressor, limiter, gate).

/// Biquad section — Transposed Direct Form II (better numeric behaviour than DF-I
/// at low cutoffs / high sample rates).
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    /// TDF-II state.
    s1: f32,
    s2: f32,
}

impl Biquad {
    fn process(&mut self, x: f32) -> f32 {
        // Transposed Direct Form II (RBJ / standard IIR cascade form):
        //   y  = b0·x + s1
        //   s1 = b1·x − a1·y + s2
        //   s2 = b2·x − a2·y
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
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
            s1: 0.0,
            s2: 0.0,
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
            s1: 0.0,
            s2: 0.0,
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
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Peaking / bell EQ — Audio EQ Cookbook (Robert Bristow-Johnson).
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
            s1: 0.0,
            s2: 0.0,
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
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, s1: 0.0, s2: 0.0 }
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
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, s1: 0.0, s2: 0.0 }
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
/// bandwidth `q` (RBJ Audio EQ Cookbook).
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

// ── Graphic EQ (RBJ / f64, no saturation) ───────────────────────────────────

/// One second-order section for the graphic-EQ cascade.
///
/// Coefficients and state are **f64** (industry practice for LF peaking / shelves
/// at audio rates — f32 coeff design is a common source of harsh LF mush).
#[derive(Debug, Clone)]
struct EqSection {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    s1: f64,
    s2: f64,
}

impl EqSection {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        // Transposed Direct Form II
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        // Flush denormals in the state (can sound like "grit" on long tails).
        if self.s1.abs() < 1e-20 {
            self.s1 = 0.0;
        }
        if self.s2.abs() < 1e-20 {
            self.s2 = 0.0;
        }
        y
    }

    /// RBJ peaking EQ using **bandwidth in octaves** (cookbook form with
    /// `alpha = sin(w0) * sinh(ln(2)/2 * BW * w0/sin(w0))`).
    /// This is what SoX / FFmpeg `equalizer` and most DAW peaking filters use.
    fn peaking(sr: f64, freq: f64, gain_db: f64, bw_oct: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let a = 10.0f64.powf(gain_db / 40.0);
        // Digital bandwidth → alpha (RBJ cookbook).
        let bw = bw_oct.clamp(0.25, 4.0);
        let alpha = sin_w0 * (std::f64::consts::LN_2 / 2.0 * bw * w0 / sin_w0.max(1e-12)).sinh();
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, s1: 0.0, s2: 0.0 }
    }

    /// RBJ low shelf, shelf slope `S = 1` (maximally flat transition when gain → 0).
    fn lowshelf(sr: f64, freq: f64, gain_db: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let a = 10.0f64.powf(gain_db / 40.0);
        // S = 1 → alpha = sin(w0)/2 * √((A + 1/A)·0 + 2) = sin(w0)/√2
        let alpha = sin_w0 * std::f64::consts::FRAC_1_SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, s1: 0.0, s2: 0.0 }
    }

    /// RBJ high shelf, shelf slope `S = 1`.
    fn highshelf(sr: f64, freq: f64, gain_db: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * freq / sr;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let a = 10.0f64.powf(gain_db / 40.0);
        let alpha = sin_w0 * std::f64::consts::FRAC_1_SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, s1: 0.0, s2: 0.0 }
    }
}

/// Bandwidth (octaves) for band `i` from geometric spacing of neighbouring centres.
///
/// Half-power edges sit at the geometric mean with the adjacent band — the usual
/// constant-Q construction for octave / ISO graphic equalisers.
fn graphic_band_bw_octaves(centres: &[f32], i: usize) -> f64 {
    let n = centres.len();
    debug_assert!(i < n);
    let f = centres[i] as f64;
    if n < 2 || f <= 0.0 {
        return 1.0;
    }
    let lo = if i == 0 { f * f / centres[1].max(1.0) as f64 } else { centres[i - 1] as f64 };
    let hi =
        if i + 1 >= n { f * f / centres[i - 1].max(1.0) as f64 } else { centres[i + 1] as f64 };
    let f_lo = (lo.max(1.0) * f).sqrt();
    let f_hi = (f * hi.max(f + 1.0)).sqrt();
    (f_hi / f_lo).log2().clamp(0.5, 2.5)
}

/// Proportional-Q bandwidth: **wide at small gains, tighter at large gains**.
///
/// Matches classic musical / console graphic behaviour — small boosts tilt the
/// spectrum gently; deep cuts become more selective. Avoids the resonant
/// “honk / grunge” of fixed high-Q peaking at every fader move.
fn proportional_bw_octaves(base_bw: f64, gain_db: f64) -> f64 {
    let g = gain_db.abs().clamp(0.0, 18.0);
    // g=0 → ~2.2× base; g=12 → ~1.0× base; g=18 → ~0.85× base
    let widen = 1.0 + 1.2 * (1.0 - (g / 12.0).min(1.0));
    let narrow = if g > 12.0 { 1.0 - 0.15 * ((g - 12.0) / 6.0).min(1.0) } else { 1.0 };
    (base_bw * widen * narrow).clamp(0.4, 3.0)
}

/// **Graphic equalizer** — RBJ cookbook cascade (SoX / FFmpeg / JUCE style).
///
/// Design choices (why this does not sound “grungy”):
/// * Coefficients and filter state in **f64**; process in f64, write f32.
/// * Mid bands: peaking with bandwidth from **geometric band spacing**, then
///   **proportional-Q** (wider for mild gains).
/// * Outer bands: **low / high shelf** (standard consumer 10-band practice —
///   cleaner bass/treble than a 32 Hz / 16 kHz peaker).
/// * Preamp first. **No soft-clip or peak-normalize** on this path (those add
///   harmonics and were a primary source of grunge). Use negative preamp for
///   headroom when stacking boosts.
///
/// # Panics
/// Panics if `centre_hz.len() != gains_db.len()`.
pub fn graphic_eq(
    signal: &[f32],
    sample_rate: u32,
    centre_hz: &[f32],
    gains_db: &[f32],
    preamp_db: f32,
) -> Vec<f32> {
    assert_eq!(
        centre_hz.len(),
        gains_db.len(),
        "graphic_eq: centre_hz and gains_db length mismatch"
    );
    let sr = sample_rate as f64;
    if sr < 1.0 {
        return signal.to_vec();
    }
    // Keep poles comfortably below Nyquist (cookbook recommendation).
    let nyq = sr * 0.45;

    let mut stages: Vec<EqSection> = Vec::with_capacity(centre_hz.len());
    let n = centre_hz.len();
    for (i, (&f_hz, &g_db)) in centre_hz.iter().zip(gains_db.iter()).enumerate() {
        if g_db.abs() < 0.05 {
            continue;
        }
        let gain = f64::from(g_db).clamp(-18.0, 18.0);
        let freq = f64::from(f_hz).clamp(20.0, nyq);
        if freq >= nyq {
            continue;
        }

        let section = if n >= 2 && i == 0 {
            // Bottom fader → low shelf at the band centre.
            EqSection::lowshelf(sr, freq, gain)
        } else if n >= 2 && i + 1 == n {
            // Top fader → high shelf.
            EqSection::highshelf(sr, freq, gain)
        } else {
            let base_bw = graphic_band_bw_octaves(centre_hz, i);
            let bw = proportional_bw_octaves(base_bw, gain);
            EqSection::peaking(sr, freq, gain, bw)
        };
        stages.push(section);
    }

    let pre = if preamp_db.abs() >= 0.05 { 10.0f64.powf(f64::from(preamp_db) / 20.0) } else { 1.0 };

    if stages.is_empty() && (pre - 1.0).abs() < 1e-12 {
        return signal.to_vec();
    }

    signal
        .iter()
        .map(|&x| {
            let mut y = f64::from(x) * pre;
            for st in &mut stages {
                y = st.process(y);
            }
            // Transparent path — no saturator. Clamp only pure NaN/Inf.
            if !y.is_finite() { 0.0 } else { y as f32 }
        })
        .collect()
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
    fn graphic_eq_flat_is_near_identity() {
        let sig: Vec<f32> = (0..4800)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let freqs = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16_000.0];
        let gains = [0.0f32; 10];
        let out = graphic_eq(&sig, 48_000, &freqs, &gains, 0.0);
        let err: f32 =
            sig.iter().zip(out.iter()).map(|(a, b)| (a - b).abs()).sum::<f32>() / sig.len() as f32;
        assert!(err < 1e-5, "flat graphic_eq should pass through, mean err={err}");
    }

    #[test]
    fn graphic_eq_boost_at_1k_raises_energy() {
        // 1 kHz tone should get louder when only the 1 kHz band is boosted.
        let sr = 48_000u32;
        let sig: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin() * 0.25)
            .collect();
        let freqs = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16_000.0];
        let mut gains = [0.0f32; 10];
        gains[5] = 6.0; // 1 kHz band
        let out = graphic_eq(&sig, sr, &freqs, &gains, 0.0);
        let skip = 2000; // settle filter state
        let rms = |s: &[f32]| {
            let n = s.len().saturating_sub(skip);
            (s[skip..].iter().map(|x| x * x).sum::<f32>() / n as f32).sqrt()
        };
        assert!(
            rms(&out) > rms(&sig) * 1.3,
            "6 dB @ 1 kHz should raise 1 kHz tone RMS ({} vs {})",
            rms(&out),
            rms(&sig)
        );
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
