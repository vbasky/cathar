//! Spectral-subtraction / Wiener denoiser.

use crate::util::hann_window;
use crate::{AudioData, Error};
use realfft::RealFftPlanner;

/// A denoising strategy: turn noisy [`AudioData`] into a cleaner copy.
pub trait Denoiser {
    /// Denoise every channel of `input`, returning a new [`AudioData`].
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error>;
}

// ── NoisePrint ───────────────────────────────────────────────────────────────

/// Pre-computed noise profile from a silence segment.
/// Feed into `SpectralDenoiser::with_noise_print` instead of auto-detection.
#[derive(Debug, Clone)]
pub struct NoisePrint {
    /// FFT size the spectrum was measured at (must match the denoiser's).
    pub fft_size: usize,
    /// Per-bin noise magnitude spectrum (`fft_size / 2 + 1` bins).
    pub spectrum: Vec<f32>,
}

/// Learn a noise profile from an audio segment (should be silence/noise-only).
pub fn learn_noise_print(audio: &AudioData) -> Result<NoisePrint, Error> {
    let fft_size = 2048;
    let signal = &audio.channels[0];
    if signal.len() < fft_size {
        return Err(Error::TooShort);
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let hann = hann_window(fft_size);
    let hop = fft_size / 4;
    let frames = signal.len() / hop;
    let n_bins = fft_size / 2 + 1;
    let mut spectrum = vec![0.0f32; n_bins];
    let mut count = 0usize;
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop;
        if offset + fft_size > signal.len() {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();
        for k in 0..n_bins {
            spectrum[k] += (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
        }
        count += 1;
    }
    if count == 0 {
        return Err(Error::TooShort);
    }
    for v in &mut spectrum {
        *v /= count as f32;
    }
    Ok(NoisePrint { fft_size, spectrum })
}

/// Learn a noise profile from the quietest `duration_s` seconds of `audio`.
///
/// Falls back to the whole file when it is shorter than `duration_s`. The
/// `vhs` chain uses [`learn_noise_print_quiet_windows`] instead: a contiguous
/// stretch on dialogue includes speech.
pub fn learn_noise_print_quietest(audio: &AudioData, duration_s: f32) -> Result<NoisePrint, Error> {
    if audio.channels.is_empty() || audio.channels[0].is_empty() {
        return Err(Error::TooShort);
    }
    let n = audio.channels[0].len();
    let win = ((duration_s * audio.sample_rate as f32).round() as usize).clamp(2048, n);
    if n < 2048 {
        return Err(Error::TooShort);
    }
    let hop = 2048.min(win);
    let mut best_start = 0;
    let mut best_e = f32::MAX;
    let mut start = 0;
    while start + win <= n {
        let mut e = 0.0f32;
        let mut count = 0usize;
        for ch in &audio.channels {
            for v in &ch[start..start + win] {
                e += v * v;
                count += 1;
            }
        }
        e /= count.max(1) as f32;
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
    if tail != best_start {
        let mut e = 0.0f32;
        let mut count = 0usize;
        for ch in &audio.channels {
            for v in &ch[tail..] {
                e += v * v;
                count += 1;
            }
        }
        e /= count.max(1) as f32;
        if e < best_e {
            best_start = tail;
        }
    }
    let slice = AudioData {
        sample_rate: audio.sample_rate,
        channels: audio.channels.iter().map(|c| c[best_start..best_start + win].to_vec()).collect(),
    };
    learn_noise_print(&slice)
}

/// Mean-square below this is digital silence (a dropout), not tape hiss.
const SILENCE_MS: f32 = 1e-12;

#[derive(Clone, Copy)]
struct QuietWindow {
    start: usize,
    energy: f32,
}

fn mean_square(audio: &AudioData, start: usize, len: usize) -> f32 {
    let mut e = 0.0f32;
    let mut count = 0usize;
    for ch in &audio.channels {
        for &v in &ch[start..start + len] {
            e += v * v;
            count += 1;
        }
    }
    e / count.max(1) as f32
}

/// Non-overlapping `win`-sample tiles, skipping digital silence.
fn tile_windows(audio: &AudioData, win: usize) -> Vec<QuietWindow> {
    let n = audio.channels[0].len();
    let mut out = Vec::new();
    let mut start = 0;
    while start + win <= n {
        let energy = mean_square(audio, start, win);
        if energy > SILENCE_MS {
            out.push(QuietWindow { start, energy });
        }
        start += win;
    }
    out
}

/// Quietest `quiet_fraction` of `windows`, then `n_pick` of those spread evenly by level.
fn pick_even_by_level(
    mut windows: Vec<QuietWindow>,
    quiet_fraction: f32,
    n_pick: usize,
) -> Vec<QuietWindow> {
    if windows.is_empty() || n_pick == 0 {
        return Vec::new();
    }
    windows.sort_by(|a, b| a.energy.total_cmp(&b.energy).then(a.start.cmp(&b.start)));
    let n_quiet = ((windows.len() as f32) * quiet_fraction.clamp(0.0, 1.0)).ceil() as usize;
    let n_quiet = n_quiet.clamp(1, windows.len());
    let quiet = &windows[..n_quiet];
    let n_pick = n_pick.min(quiet.len());
    let mut picked = Vec::with_capacity(n_pick);
    if n_pick == 1 {
        picked.push(quiet[0]);
    } else {
        for k in 0..n_pick {
            let idx = k * (quiet.len() - 1) / (n_pick - 1);
            picked.push(quiet[idx]);
        }
    }
    picked.sort_by_key(|w| w.start);
    picked.dedup_by_key(|w| w.start);
    picked
}

fn stitch_windows(audio: &AudioData, starts: &[usize], win: usize, fade: usize) -> AudioData {
    let fade = fade.min(win / 2);
    let channels = audio
        .channels
        .iter()
        .map(|ch| {
            let mut out = Vec::new();
            for (i, &start) in starts.iter().enumerate() {
                let src = &ch[start..start + win];
                if i == 0 || fade == 0 {
                    out.extend_from_slice(src);
                    continue;
                }
                let n = out.len();
                for (k, &incoming) in src.iter().take(fade).enumerate() {
                    let t = (k + 1) as f32 / fade as f32;
                    let dst = n - fade + k;
                    out[dst] = out[dst] * (1.0 - t) + incoming * t;
                }
                out.extend_from_slice(&src[fade..]);
            }
            out
        })
        .collect();
    AudioData { sample_rate: audio.sample_rate, channels }
}

/// Starts of the windows [`learn_noise_print_quiet_windows`] would stitch.
pub(crate) fn quiet_window_starts(
    audio: &AudioData,
    window_s: f32,
    n_windows: usize,
    quiet_fraction: f32,
) -> Vec<usize> {
    if audio.channels.is_empty() || audio.channels[0].is_empty() {
        return Vec::new();
    }
    let n = audio.channels[0].len();
    if n < 2048 {
        return Vec::new();
    }
    let win = ((window_s * audio.sample_rate as f32).round() as usize).clamp(2048, n);
    pick_even_by_level(tile_windows(audio, win), quiet_fraction, n_windows.max(1))
        .into_iter()
        .map(|w| w.start)
        .collect()
}

/// Learn a noise print from quiet windows spread across the file.
///
/// Non-overlapping `window_s` tiles are ranked by mean-square level. The
/// quietest `quiet_fraction` of them are the pool; `n_windows` of those are
/// taken evenly by level, concatenated with `crossfade_s` linear joins, and
/// passed to [`learn_noise_print`].
///
/// A single contiguous stretch (see [`learn_noise_print_quietest`]) is used
/// when the file is too short to tile. Digital-silent tiles are skipped so a
/// dropout is not learned as the noise floor.
///
/// The `vhs` chain uses 8 × 0.75 s over the quietest 20 %, 10 ms crossfades:
/// a contiguous 4 s on dialogue sits well above the true pauses and the print
/// carries sibilance into the subtraction.
pub fn learn_noise_print_quiet_windows(
    audio: &AudioData,
    window_s: f32,
    n_windows: usize,
    quiet_fraction: f32,
    crossfade_s: f32,
) -> Result<NoisePrint, Error> {
    if audio.channels.is_empty() || audio.channels[0].is_empty() {
        return Err(Error::TooShort);
    }
    let n = audio.channels[0].len();
    if n < 2048 {
        return Err(Error::TooShort);
    }
    let sr = audio.sample_rate as f32;
    let win = ((window_s * sr).round() as usize).clamp(2048, n);
    if n / win < 2 {
        return learn_noise_print_quietest(audio, window_s);
    }
    let starts = quiet_window_starts(audio, window_s, n_windows, quiet_fraction);
    if starts.is_empty() {
        return learn_noise_print_quietest(audio, window_s);
    }
    let fade = ((crossfade_s.max(0.0) * sr).round() as usize).min(win / 2);
    learn_noise_print(&stitch_windows(audio, &starts, win, fade))
}

// ── SpectralDenoiser ─────────────────────────────────────────────────────────

/// STFT spectral-subtraction / Wiener denoiser (see [`Denoiser`]).
pub struct SpectralDenoiser {
    /// Analysis/synthesis FFT size in samples.
    pub fft_size: usize,
    /// Hop between successive frames in samples (overlap = `fft_size - hop_size`).
    pub hop_size: usize,
    /// Over-subtraction factor: how many times the noise estimate to subtract
    /// (1.0 = gentle, 6.0 = aggressive).
    pub alpha: f32,
    /// Spectral floor as a fraction of the input magnitude, limiting "musical
    /// noise" artifacts (0.0–0.1).
    pub beta: f32,
    /// Fraction of the quietest frames taken as the noise estimate when no
    /// `noise_print` is supplied (minimum-statistics).
    pub noise_frame_ratio: f32,
    /// Optional pre-computed noise print. Takes priority over auto-detection.
    pub noise_print: Option<NoisePrint>,
}

impl Default for SpectralDenoiser {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            hop_size: 512,
            alpha: 3.0,
            beta: 0.01,
            noise_frame_ratio: 0.15,
            noise_print: None,
        }
    }
}

impl SpectralDenoiser {
    /// Build a denoiser driven by a pre-learned [`NoisePrint`] (FFT size and hop
    /// follow the print; minimum-statistics auto-detection is disabled).
    pub fn with_noise_print(noise_print: NoisePrint, alpha: f32, beta: f32) -> Self {
        Self {
            fft_size: noise_print.fft_size,
            hop_size: noise_print.fft_size / 4,
            alpha,
            beta,
            noise_frame_ratio: 0.0,
            noise_print: Some(noise_print),
        }
    }
}

impl Denoiser for SpectralDenoiser {
    fn denoise(&self, input: &AudioData) -> Result<AudioData, Error> {
        let mut output_channels = Vec::with_capacity(input.channels.len());
        for channel in &input.channels {
            output_channels.push(self.denoise_channel(channel)?);
        }
        Ok(AudioData { sample_rate: input.sample_rate, channels: output_channels })
    }
}

impl SpectralDenoiser {
    fn noise_spectrum(&self, signal: &[f32]) -> Result<Vec<f32>, Error> {
        if let Some(ref np) = self.noise_print {
            if np.fft_size != self.fft_size {
                return Err(Error::NoisePrintMismatch);
            }
            return Ok(np.spectrum.clone());
        }
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(self.fft_size);
        let hann = hann_window(self.fft_size);
        let noise_frames = signal.len() / self.hop_size;
        let n_bins = self.fft_size / 2 + 1;
        let mut spectrum = vec![f32::MAX; n_bins];
        let mut in_buf = r2c.make_input_vec();
        let mut out_buf = r2c.make_output_vec();
        for fi in 0..noise_frames {
            let offset = fi * self.hop_size;
            if offset + self.fft_size > signal.len() {
                break;
            }
            for i in 0..self.fft_size {
                in_buf[i] = signal[offset + i] * hann[i];
            }
            r2c.process(&mut in_buf, &mut out_buf).unwrap();
            for (k, item) in spectrum.iter_mut().enumerate() {
                let mag = (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
                *item = (*item).min(mag);
            }
        }
        for v in &mut spectrum {
            *v *= 2.0;
        }
        Ok(spectrum)
    }

    fn denoise_channel(&self, signal: &[f32]) -> Result<Vec<f32>, Error> {
        if signal.len() < self.fft_size {
            return Err(Error::TooShort);
        }
        let n = signal.len();
        let noise_spectrum = self.noise_spectrum(signal)?;
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(self.fft_size);
        let c2r = planner.plan_fft_inverse(self.fft_size);
        let hann = hann_window(self.fft_size);
        let scale = 1.0f32 / (self.fft_size as f32);
        let frames = n / self.hop_size;
        let mut output = vec![0.0f32; n + self.fft_size];
        let mut in_buf = r2c.make_input_vec();
        let mut out_buf = r2c.make_output_vec();

        for fi in 0..frames {
            let offset = fi * self.hop_size;
            if offset + self.fft_size > n {
                break;
            }
            for i in 0..self.fft_size {
                in_buf[i] = signal[offset + i] * hann[i];
            }
            r2c.process(&mut in_buf, &mut out_buf).unwrap();
            for (k, ns) in noise_spectrum.iter().enumerate() {
                let mag = (out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im).sqrt();
                let phase = out_buf[k].im.atan2(out_buf[k].re);
                let clean_mag = (mag - self.alpha * ns).max(self.beta * mag).max(0.0);
                if k == 0 || k == noise_spectrum.len() - 1 {
                    out_buf[k].re = clean_mag;
                    out_buf[k].im = 0.0;
                } else {
                    out_buf[k].re = clean_mag * phase.cos();
                    out_buf[k].im = clean_mag * phase.sin();
                }
            }
            c2r.process(&mut out_buf, &mut in_buf).unwrap();
            for i in 0..self.fft_size {
                output[offset + i] += in_buf[i] * hann[i] * scale;
            }
        }
        output.truncate(n);
        Ok(output)
    }

    /// Denoise **phase-coherently** across channels: the per-bin suppression gain
    /// is computed once from the mid signal (the channel average) and applied
    /// identically to every channel. Independent per-channel denoising gates bins
    /// differently in L and R, which makes the residual noise floor pan; one
    /// shared gain keeps the stereo image stable while each channel keeps its own
    /// phase. Equivalent to [`denoise`](Self::denoise) for mono input.
    pub fn denoise_coherent(&self, input: &AudioData) -> Result<AudioData, Error> {
        let n_ch = input.channels.len();
        if n_ch <= 1 {
            return self.denoise(input);
        }
        let len = input.channels.iter().map(|c| c.len()).min().unwrap_or(0);
        if len < self.fft_size {
            return Err(Error::TooShort);
        }
        let mid: Vec<f32> = (0..len)
            .map(|i| input.channels.iter().map(|c| c[i]).sum::<f32>() / n_ch as f32)
            .collect();
        let noise_spectrum = self.noise_spectrum(&mid)?;

        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(self.fft_size);
        let c2r = planner.plan_fft_inverse(self.fft_size);
        let hann = hann_window(self.fft_size);
        let scale = 1.0f32 / self.fft_size as f32;
        let frames = len / self.hop_size;
        let n_bins = self.fft_size / 2 + 1;

        let mut outputs = vec![vec![0.0f32; len + self.fft_size]; n_ch];
        let mut mid_in = r2c.make_input_vec();
        let mut mid_out = r2c.make_output_vec();
        let mut ch_in = r2c.make_input_vec();
        let mut ch_out = r2c.make_output_vec();
        let mut inv = c2r.make_output_vec();
        let mut gains = vec![1.0f32; n_bins];

        for fi in 0..frames {
            let offset = fi * self.hop_size;
            if offset + self.fft_size > len {
                break;
            }
            // One shared gain mask from the mid signal.
            for i in 0..self.fft_size {
                mid_in[i] = mid[offset + i] * hann[i];
            }
            r2c.process(&mut mid_in, &mut mid_out).unwrap();
            for (k, ns) in noise_spectrum.iter().enumerate() {
                let mag = (mid_out[k].re * mid_out[k].re + mid_out[k].im * mid_out[k].im).sqrt();
                let clean = (mag - self.alpha * ns).max(self.beta * mag).max(0.0);
                gains[k] = if mag > 1e-10 { clean / mag } else { 0.0 };
            }
            // Apply it to every channel, preserving each channel's phase.
            for (ch, out) in outputs.iter_mut().enumerate() {
                for i in 0..self.fft_size {
                    ch_in[i] = input.channels[ch][offset + i] * hann[i];
                }
                r2c.process(&mut ch_in, &mut ch_out).unwrap();
                for (c, &g) in ch_out.iter_mut().zip(gains.iter()) {
                    c.re *= g;
                    c.im *= g;
                }
                c2r.process(&mut ch_out, &mut inv).unwrap();
                for i in 0..self.fft_size {
                    out[offset + i] += inv[i] * hann[i] * scale;
                }
            }
        }
        let channels = outputs
            .into_iter()
            .map(|mut o| {
                o.truncate(len);
                o
            })
            .collect();
        Ok(AudioData { sample_rate: input.sample_rate, channels })
    }
}

/// Wiener-filter denoiser — statistically optimal, better transients.
pub fn wiener_denoise(
    signal: &[f32],
    noise_print: &NoisePrint,
    alpha: f32,
) -> Result<Vec<f32>, Error> {
    let fft_size = noise_print.fft_size;
    let hop_size = fft_size / 4;
    let n = signal.len();
    if n < fft_size {
        return Err(Error::TooShort);
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let c2r = planner.plan_fft_inverse(fft_size);
    let hann = hann_window(fft_size);
    let scale = 1.0f32 / (fft_size as f32);
    let frames = n / hop_size;
    let n_bins = fft_size / 2 + 1;

    // Smooth the noise spectrum
    let noise: Vec<f32> = noise_print.spectrum.iter().map(|&v| v * alpha).collect();

    let mut output = vec![0.0f32; n + fft_size];
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    for fi in 0..frames {
        let offset = fi * hop_size;
        if offset + fft_size > n {
            break;
        }
        for i in 0..fft_size {
            in_buf[i] = signal[offset + i] * hann[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).unwrap();

        for k in 0..n_bins {
            let signal_power = out_buf[k].re * out_buf[k].re + out_buf[k].im * out_buf[k].im;
            let noise_power = noise[k] * noise[k];
            // Wiener gain: signal / (signal + noise)
            let gain = signal_power / (signal_power + noise_power).max(1e-10);
            out_buf[k].re *= gain;
            out_buf[k].im *= gain;
        }
        c2r.process(&mut out_buf, &mut in_buf).unwrap();
        for i in 0..fft_size {
            output[offset + i] += in_buf[i] * hann[i] * scale;
        }
    }
    output.truncate(n);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioData;

    fn xorshift(rng: &mut u64) -> f32 {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng as f32 / u64::MAX as f32 - 0.5
    }

    /// 16 × 0.75 s tiles: odd tiles are hiss, even tiles are 1 kHz + 8 kHz speech.
    fn dialogue_with_pauses(sr: u32) -> AudioData {
        let win = ((0.75 * sr as f32).round() as usize).max(2048);
        let n_tiles = 16;
        let n = win * n_tiles;
        let two_pi = 2.0 * std::f32::consts::PI;
        let mut rng = 1u64;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                let hiss = xorshift(&mut rng) * 0.08;
                if (i / win) % 2 == 1 {
                    hiss
                } else {
                    hiss + 0.4 * (two_pi * 1000.0 * t).sin() + 0.4 * (two_pi * 8000.0 * t).sin()
                }
            })
            .collect();
        AudioData { sample_rate: sr, channels: vec![x] }
    }

    fn bin_at(np: &NoisePrint, hz: f32, sr: u32) -> f32 {
        let k = (hz * np.fft_size as f32 / sr as f32).round() as usize;
        np.spectrum[k.min(np.spectrum.len() - 1)]
    }

    #[test]
    fn quiet_windows_land_on_pauses_not_speech() {
        let sr = 48_000u32;
        let win = ((0.75 * sr as f32).round() as usize).max(2048);
        let audio = dialogue_with_pauses(sr);
        let starts = quiet_window_starts(&audio, 0.75, 8, 0.20);
        assert!(!starts.is_empty(), "expected quiet windows");
        for &s in &starts {
            assert_eq!(s % win, 0, "window {s} is not on the 0.75 s grid");
            let tile = s / win;
            assert_eq!(tile % 2, 1, "window at tile {tile} is speech, not a pause");
        }
        for i in 1..starts.len() {
            assert!(starts[i] >= starts[i - 1] + win, "windows overlap");
        }
    }

    #[test]
    fn stitched_print_drops_sibilance_contiguous_4s_keeps() {
        // The quietest 4 s on dialogue includes speech, so the print carries
        // 8 kHz and the subtraction eats the presence band. Short windows in
        // the pauses do not.
        let sr = 48_000u32;
        let audio = dialogue_with_pauses(sr);
        let stitched = learn_noise_print_quiet_windows(&audio, 0.75, 8, 0.20, 0.010).unwrap();
        let contiguous = learn_noise_print_quietest(&audio, 4.0).unwrap();
        let s8 = bin_at(&stitched, 8000.0, sr);
        let c8 = bin_at(&contiguous, 8000.0, sr);
        assert!(
            s8 < c8 * 0.5,
            "stitched 8 kHz {s8:.4} should be well below contiguous 4 s {c8:.4}"
        );
        assert!(s8 > 1e-4, "stitched print still captured hiss at 8 kHz");
    }

    #[test]
    fn quiet_windows_falls_back_when_too_short_to_tile() {
        let audio = crate::util::generate_wave(48_000, 440.0, 0.8, 0.1);
        let a = learn_noise_print_quiet_windows(&audio, 0.75, 8, 0.20, 0.010).unwrap();
        let b = learn_noise_print_quietest(&audio, 0.75).unwrap();
        assert_eq!(a.fft_size, b.fft_size);
        assert_eq!(a.spectrum.len(), b.spectrum.len());
    }
}
