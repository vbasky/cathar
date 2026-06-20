//! STFT magnitude spectrogram — for analysis and the optional terminal viewer.

use crate::util::hann_window;
use realfft::RealFftPlanner;

/// A magnitude spectrogram: a column per time frame, each holding `bins`
/// frequency rows (row 0 = DC, row `bins-1` = Nyquist), in decibels.
#[derive(Debug, Clone)]
pub struct Spectrogram {
    /// Source sample rate, Hz.
    pub sample_rate: u32,
    /// FFT size used (frequency resolution = `sample_rate / fft_size`).
    pub fft_size: usize,
    /// Hop between successive frames, in samples.
    pub hop: usize,
    /// Frequency bins per frame (`fft_size / 2 + 1`).
    pub bins: usize,
    /// Row-major dB magnitudes laid out as `data[frame * bins + bin]`.
    pub data: Vec<f32>,
}

impl Spectrogram {
    /// Number of time frames.
    pub fn frames(&self) -> usize {
        self.data.len().checked_div(self.bins).unwrap_or(0)
    }

    /// dB magnitude at `(frame, bin)`.
    pub fn get(&self, frame: usize, bin: usize) -> f32 {
        self.data[frame * self.bins + bin]
    }

    /// Centre frequency (Hz) of a bin.
    pub fn bin_hz(&self, bin: usize) -> f32 {
        bin as f32 * self.sample_rate as f32 / self.fft_size as f32
    }

    /// Start time (seconds) of a frame.
    pub fn frame_time(&self, frame: usize) -> f32 {
        (frame * self.hop) as f32 / self.sample_rate as f32
    }
}

/// Compute the magnitude spectrogram of `signal` via a Hann-windowed STFT.
///
/// Magnitudes are returned in decibels (`20·log10`, amplitude-normalised by the
/// window and floored at −120 dB). `hop` is the step between frames in samples
/// (e.g. `fft_size / 4` for 75 % overlap). A signal shorter than one frame yields
/// an empty spectrogram.
pub fn spectrogram(signal: &[f32], sample_rate: u32, fft_size: usize, hop: usize) -> Spectrogram {
    let bins = fft_size / 2 + 1;
    let hop = hop.max(1);
    if signal.len() < fft_size {
        return Spectrogram { sample_rate, fft_size, hop, bins, data: Vec::new() };
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft_size);
    let win = hann_window(fft_size);
    let win_sum: f32 = win.iter().sum::<f32>().max(1e-9);
    let mut in_buf = r2c.make_input_vec();
    let mut out_buf = r2c.make_output_vec();

    let mut data = Vec::with_capacity((signal.len() / hop + 1) * bins);
    let mut pos = 0;
    while pos + fft_size <= signal.len() {
        for (i, s) in in_buf.iter_mut().enumerate() {
            *s = signal[pos + i] * win[i];
        }
        r2c.process(&mut in_buf, &mut out_buf).expect("realfft forward");
        for c in &out_buf {
            // One-sided amplitude: ×2 / window sum (DC/Nyquist slightly overstated,
            // immaterial for display).
            let mag = (c.re * c.re + c.im * c.im).sqrt() * 2.0 / win_sum;
            data.push((20.0 * mag.max(1e-6).log10()).max(-120.0));
        }
        pos += hop;
    }
    Spectrogram { sample_rate, fft_size, hop, bins, data }
}
