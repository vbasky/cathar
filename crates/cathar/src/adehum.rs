//! Adaptive de-hum — track a drifting mains fundamental and per-harmonic
//! amplitude, instead of the fixed notches in `dehum`.
//!
//! The recording's own quietest stretch decides 50 vs 60 Hz and where each
//! harmonic actually sits (real tape lines land 1–8 Hz off the exact series).
//! Harmonics that do not stand out above their spectral neighbourhood are
//! left alone, so a file without hum is returned unchanged. Each kept line is
//! cancelled by an I/Q heterodyne canceller: demodulate → zero-phase low-pass
//! (bandwidth widens with harmonic number to follow transport wow) → 6 dB cap
//! over the running envelope so a voiced partial parked on a mains line is
//! not taken with it → remodulate and subtract. Deterministic, pure Rust.

use realfft::RealFftPlanner;

/// Minimum harmonic excess (dB of line vs neighbourhood) to treat a recording
/// as carrying mains hum.
const HUM_MIN_EXCESS_DB: f32 = 6.0;
/// A gated harmonic must stand this far above its neighbourhood.
const HARMONIC_MIN_EXCESS_DB: f32 = 3.0;
/// Harmonics past the mains series proper need a higher bar (more likely programme).
const EXTENDED_MIN_EXCESS_DB: f32 = 6.0;
/// Envelope bandwidth at the fundamental (Hz). Widens with harmonic number.
const BANDWIDTH_BASE_HZ: f32 = 2.0;
const BANDWIDTH_SLOPE_HZ: f32 = 0.5;
const BANDWIDTH_MAX_HZ: f32 = 5.0;
/// A series whose lines place the fundamental further off than this is a chord.
const MAX_REFINE_HZ: f32 = 0.5;
/// Cap a line's envelope this far above its own running level (dB).
const CAP_DB: f32 = 6.0;
/// Time constant of the running-level estimator used by the cap (seconds).
const CAP_WINDOW_S: f32 = 2.0;
/// How many core harmonics vote on 50 vs 60 and on the refined fundamental.
const MAINS_HARMONICS: usize = 8;
/// First-pass search around each nominal harmonic (Hz). Real tape lines sit
/// 1–8 Hz off the exact series; a tighter window misses them.
const LINE_SEARCH_HZ: f32 = 8.0;

struct Line {
    harmonic: usize,
    freq: f64,
    bandwidth: f32,
}

/// Envelope bandwidth of harmonic `h` (1-based), widening with wow.
fn bandwidth_hz(h: usize) -> f32 {
    (BANDWIDTH_BASE_HZ + BANDWIDTH_SLOPE_HZ * (h.saturating_sub(1) as f32)).min(BANDWIDTH_MAX_HZ)
}

/// How far off `h * f0` a line may sit and still be that harmonic.
fn series_tolerance_hz(h: usize) -> f32 {
    0.5 * bandwidth_hz(h)
}

/// Pick 50 or 60 Hz from the recording's quiet spectrum, or `None` when neither
/// series stands out as mains hum.
///
/// Used by [`dehum_adaptive`] when `base_freq` is `0`, and by the `vhs` chain.
/// Anti-phase hum that cancels in a mono downmix is missed.
pub fn detect_mains_hz(signal: &[f32], sample_rate: u32) -> Option<f32> {
    let spec = quiet_spectrum(signal, sample_rate)?;
    let e50 = series_excess(&spec, 50.0, MAINS_HARMONICS);
    let e60 = series_excess(&spec, 60.0, MAINS_HARMONICS);
    if e50 < HUM_MIN_EXCESS_DB && e60 < HUM_MIN_EXCESS_DB {
        return None;
    }
    if e50 >= e60 { Some(50.0) } else { Some(60.0) }
}

/// Adaptive mains-hum removal. `base_freq` is the nominal (50 or 60); pass `0`
/// to take 50 vs 60 from the recording. Harmonics that do not stand out as
/// lines of their own are skipped, so undamaged material is returned unchanged.
///
/// When `base_freq` is 50 or 60 but the other series is clearly stronger, the
/// stronger series is used — a 50 Hz tape is not left humming because the
/// caller defaulted to 60.
pub fn dehum_adaptive(
    signal: &[f32],
    sample_rate: u32,
    base_freq: f32,
    num_harmonics: usize,
) -> Vec<f32> {
    let n = signal.len();
    if n < 64 || sample_rate == 0 || num_harmonics == 0 {
        return signal.to_vec();
    }
    let detected = detect_mains_hz(signal, sample_rate);
    let nominal = if base_freq <= 0.0 {
        match detected {
            Some(f) => f,
            None => return signal.to_vec(),
        }
    } else if let Some(f) = detected {
        // Honour the recording when the other mains frequency is the one present.
        if (f - base_freq).abs() > 5.0 { f } else { base_freq }
    } else {
        base_freq
    };

    let lines = plan_harmonics(signal, sample_rate, nominal, num_harmonics);
    if lines.is_empty() || !lines.iter().any(|l| l.harmonic == 1) {
        return signal.to_vec();
    }

    let mut out = signal.to_vec();
    for line in lines {
        if line.freq >= sample_rate as f64 * 0.45 {
            break;
        }
        cancel_line(&mut out, sample_rate, line.freq, line.bandwidth);
    }
    out
}

fn plan_harmonics(signal: &[f32], sample_rate: u32, f0: f32, count: usize) -> Vec<Line> {
    let Some(spec) = quiet_spectrum(signal, sample_rate) else {
        return Vec::new();
    };
    let mut core = Vec::new();
    for h in 1..=count.min(MAINS_HARMONICS) {
        if let Some((_hz, excess)) = line_near(&spec, h as f32 * f0) {
            if excess >= HARMONIC_MIN_EXCESS_DB {
                core.push(h);
            }
        }
    }
    let Some(refined) = refine_f0(&spec, f0, &core) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for h in 1..=count {
        let target = h as f32 * refined;
        if target >= sample_rate as f32 * 0.45 {
            break;
        }
        let bar =
            if h <= MAINS_HARMONICS { HARMONIC_MIN_EXCESS_DB } else { EXTENDED_MIN_EXCESS_DB };
        let Some((hz, excess)) = line_near(&spec, target) else {
            continue;
        };
        if excess < bar {
            continue;
        }
        if (hz - target).abs() > series_tolerance_hz(h) {
            continue;
        }
        lines.push(Line { harmonic: h, freq: hz as f64, bandwidth: bandwidth_hz(h) });
    }
    lines
}

fn refine_f0(spec: &Spectrum, f0: f32, harmonics: &[usize]) -> Option<f32> {
    if harmonics.is_empty() {
        return Some(f0);
    }
    let search = MAX_REFINE_HZ * 1.5;
    let mut offsets = Vec::with_capacity(harmonics.len());
    for &h in harmonics {
        let target = h as f32 * f0;
        let Some(hz) = peak_hz(spec, target, search * h as f32) else {
            continue;
        };
        offsets.push((hz - target) / h as f32);
    }
    if offsets.is_empty() {
        return Some(f0);
    }
    offsets.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let offset = offsets[offsets.len() / 2];
    if offset.abs() > MAX_REFINE_HZ {
        return None;
    }
    Some(f0 + offset)
}

fn series_excess(spec: &Spectrum, f0: f32, count: usize) -> f32 {
    let mut acc = 0.0;
    let mut n = 0usize;
    for h in 1..=count {
        if let Some((_hz, excess)) = line_near(spec, h as f32 * f0) {
            acc += excess;
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { acc / n as f32 }
}

fn line_near(spec: &Spectrum, hz: f32) -> Option<(f32, f32)> {
    let lo = spec.bin((hz - LINE_SEARCH_HZ).max(spec.hz_per_bin));
    let hi = spec.bin(hz + LINE_SEARCH_HZ).min(spec.power.len());
    if hi <= lo + 2 {
        return None;
    }
    let local = lo
        + spec.power[lo..hi]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
    // A peak at the edge of the search belongs to a neighbour, not this line.
    if local <= lo || local + 1 >= hi || local + 1 >= spec.power.len() {
        return None;
    }
    let peak = spec.power[local];
    let floor = neighbourhood_floor(spec, local);
    if floor <= 0.0 {
        return None;
    }
    let excess = 10.0 * (peak / floor).log10();
    let found = interpolated_hz(spec, local);
    Some((found, excess))
}

fn peak_hz(spec: &Spectrum, target: f32, search_hz: f32) -> Option<f32> {
    let lo = spec.bin((target - search_hz).max(spec.hz_per_bin));
    let hi = spec.bin(target + search_hz).min(spec.power.len() - 2);
    if hi <= lo {
        return None;
    }
    let peak = (lo..=hi).max_by(|&a, &b| {
        spec.power[a].partial_cmp(&spec.power[b]).unwrap_or(std::cmp::Ordering::Equal)
    })?;
    if peak == 0 || peak + 1 >= spec.power.len() {
        return None;
    }
    Some(interpolated_hz(spec, peak))
}

fn interpolated_hz(spec: &Spectrum, bin: usize) -> f32 {
    let left = (spec.power[bin - 1] + 1e-30).ln();
    let centre = (spec.power[bin] + 1e-30).ln();
    let right = (spec.power[bin + 1] + 1e-30).ln();
    let denom = left - 2.0 * centre + right;
    let shift = if denom >= 0.0 { 0.0 } else { 0.5 * (left - right) / denom };
    (bin as f32 + shift) * spec.hz_per_bin
}

fn neighbourhood_floor(spec: &Spectrum, peak: usize) -> f32 {
    let lo = peak.saturating_sub(16).max(1);
    let hi = (peak + 17).min(spec.power.len());
    let mut nb: Vec<f32> =
        (lo..hi).filter(|&b| b.abs_diff(peak) > 2).map(|b| spec.power[b]).collect();
    if nb.is_empty() {
        return 0.0;
    }
    nb.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    nb[nb.len() / 2]
}

struct Spectrum {
    power: Vec<f32>,
    hz_per_bin: f32,
}

impl Spectrum {
    fn bin(&self, hz: f32) -> usize {
        ((hz / self.hz_per_bin).round() as usize).min(self.power.len().saturating_sub(1))
    }
}

/// Power spectrum of the quietest ~1 s (or the whole file if shorter).
fn quiet_spectrum(signal: &[f32], sample_rate: u32) -> Option<Spectrum> {
    let n = signal.len();
    if n < 64 || sample_rate == 0 {
        return None;
    }
    let sr = sample_rate as usize;
    let win = sr.min(n).max(64);
    let hop = 2048.min(win);
    let mut best_start = 0;
    let mut best_e = f32::MAX;
    let mut start = 0;
    while start + win <= n {
        let e: f32 = signal[start..start + win].iter().map(|v| v * v).sum();
        if e < best_e {
            best_e = e;
            best_start = start;
        }
        if start + hop + win > n {
            break;
        }
        start += hop;
    }
    let tail = n - win;
    if tail > best_start {
        let e: f32 = signal[tail..].iter().map(|v| v * v).sum();
        if e < best_e {
            best_start = tail;
        }
    }
    let seg = &signal[best_start..best_start + win];
    let mut fft_len = win.next_power_of_two().clamp(4096, 1 << 17);
    if fft_len % 2 == 1 {
        fft_len += 1;
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_len);
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();
    in_buf[..win].copy_from_slice(seg);
    // Hann the occupied stretch so a rectangular cut doesn't smear the lines.
    if win > 1 {
        let scale = std::f32::consts::PI / (win - 1) as f32;
        for (i, v) in in_buf[..win].iter_mut().enumerate() {
            let w = 0.5 - 0.5 * (scale * i as f32).cos();
            *v *= w;
        }
    }
    r2c.process(&mut in_buf, &mut out_buf).ok()?;
    let power: Vec<f32> = out_buf.iter().map(|c| c.re * c.re + c.im * c.im).collect();
    Some(Spectrum { power, hz_per_bin: sample_rate as f32 / fft_len as f32 })
}

fn cancel_line(out: &mut [f32], sample_rate: u32, freq: f64, bandwidth_hz: f32) {
    let n = out.len();
    let sr = sample_rate as f64;
    let two_pi = 2.0 * std::f64::consts::PI;
    let w = two_pi * freq / sr;
    let lam = (1.0 - (-two_pi * bandwidth_hz as f64 / sr).exp()) as f32;

    let mut pc = vec![0.0f32; n];
    let mut ps = vec![0.0f32; n];
    let mut phase = 0.0f64;
    for i in 0..n {
        let (s, c) = phase.sin_cos();
        pc[i] = out[i] * c as f32;
        ps[i] = out[i] * s as f32;
        phase += w;
        if phase > two_pi {
            phase -= two_pi;
        }
    }
    lpf_zero_phase(&mut pc, lam);
    lpf_zero_phase(&mut ps, lam);

    // Cap: a voiced partial sharing the line may not exceed 6 dB over the
    // running envelope (2 s one-pole).
    let cap_lam = (1.0 - (-two_pi / (CAP_WINDOW_S as f64 * sr)).exp()) as f32;
    let cap_ratio = 10.0f32.powf(CAP_DB / 20.0);
    let mut run = (pc[0] * pc[0] + ps[0] * ps[0]).sqrt();
    for i in 0..n {
        let mag = (pc[i] * pc[i] + ps[i] * ps[i]).sqrt();
        run += cap_lam * (mag - run);
        let bound = run * cap_ratio;
        if mag > bound && mag > 1e-12 {
            let s = bound / mag;
            pc[i] *= s;
            ps[i] *= s;
        }
    }

    let mut phase = 0.0f64;
    for i in 0..n {
        let (s, c) = phase.sin_cos();
        out[i] -= 2.0 * (pc[i] * c as f32 + ps[i] * s as f32);
        phase += w;
        if phase > two_pi {
            phase -= two_pi;
        }
    }
}

/// Forward–backward one-pole low-pass (zero phase), seeded at the edges to tame
/// start/end transients.
fn lpf_zero_phase(x: &mut [f32], lam: f32) {
    if x.is_empty() {
        return;
    }
    let mut y = x[0];
    for v in x.iter_mut() {
        y += lam * (*v - y);
        *v = y;
    }
    y = *x.last().unwrap();
    for v in x.iter_mut().rev() {
        y += lam * (*v - y);
        *v = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hum_band_energy(x: &[f32], sr: usize, f: f32) -> f64 {
        let two_pi = 2.0 * std::f64::consts::PI;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate() {
            let p = two_pi * f as f64 * i as f64 / sr as f64;
            re += v as f64 * p.cos();
            im -= v as f64 * p.sin();
        }
        (re * re + im * im).sqrt() / x.len() as f64
    }

    fn tone(sr: usize, secs: usize, freq: f32, amp: f32) -> Vec<f32> {
        let two_pi = 2.0 * std::f32::consts::PI;
        (0..sr * secs).map(|i| amp * (two_pi * freq * i as f32 / sr as f32).sin()).collect()
    }

    #[test]
    fn cancels_drifting_hum_keeps_tone() {
        let sr = 48_000usize;
        let two_pi = 2.0 * std::f32::consts::PI;
        // 60 Hz hum whose frequency drifts ±1.5 Hz, plus a 1 kHz tone.
        let mut phase = 0.0f32;
        let mut x = vec![0.0f32; sr * 2];
        for (i, xi) in x.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            let f = 60.0 + 1.5 * (two_pi * 0.5 * t).sin();
            phase += two_pi * f / sr as f32;
            *xi = 0.5 * phase.sin() + 0.3 * (two_pi * 1000.0 * t).sin();
        }
        let out = dehum_adaptive(&x, sr as u32, 60.0, 3);
        let before = hum_band_energy(&x, sr, 60.0);
        let after = hum_band_energy(&out, sr, 60.0);
        assert!(after < before * 0.3, "hum not cancelled: {before} -> {after}");
        let tone_b = hum_band_energy(&x, sr, 1000.0);
        let tone_a = hum_band_energy(&out, sr, 1000.0);
        assert!(tone_a > tone_b * 0.8, "tone not preserved: {tone_b} -> {tone_a}");
    }

    #[test]
    fn auto_picks_50_when_asked_for_60() {
        let sr = 48_000usize;
        let mut x = tone(sr, 2, 50.0, 0.5);
        for (i, s) in x.iter_mut().enumerate() {
            *s += 0.25 * (2.0 * std::f32::consts::PI * 100.0 * i as f32 / sr as f32).sin();
            *s += 0.3 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin();
        }
        assert_eq!(detect_mains_hz(&x, sr as u32), Some(50.0));
        let out = dehum_adaptive(&x, sr as u32, 60.0, 5);
        let before = hum_band_energy(&x, sr, 50.0);
        let after = hum_band_energy(&out, sr, 50.0);
        assert!(after < before * 0.3, "50 Hz hum not cancelled at --freq 60: {before} -> {after}");
    }

    #[test]
    fn clean_tone_is_identity() {
        let sr = 48_000usize;
        let x = tone(sr, 2, 1000.0, 0.4);
        let out = dehum_adaptive(&x, sr as u32, 60.0, 5);
        assert_eq!(out, x, "no-hum material must be returned unchanged");
    }

    #[test]
    fn chord_partials_are_not_hum() {
        // G major at 98 / 147 / 196 Hz looks like harmonics 2–4 of 49 Hz.
        let sr = 48_000usize;
        let mut x = tone(sr, 2, 98.0, 0.4);
        for (i, s) in x.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            *s += 0.3 * (2.0 * std::f32::consts::PI * 147.0 * t).sin();
            *s += 0.3 * (2.0 * std::f32::consts::PI * 196.0 * t).sin();
        }
        let out = dehum_adaptive(&x, sr as u32, 50.0, 8);
        let before = hum_band_energy(&x, sr, 98.0);
        let after = hum_band_energy(&out, sr, 98.0);
        assert!(after > before * 0.9, "chord partial taken as hum: {before} -> {after}");
    }
}
