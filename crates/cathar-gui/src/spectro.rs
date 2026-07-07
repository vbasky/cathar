//! Turn a decoded signal into an egui image (spectrogram) and a waveform
//! peak envelope, reusing `cathar::spectrogram` for the STFT.

use crate::colormap::magma;
use cathar::{AudioData, Spectrogram};
use egui::ColorImage;

/// Mono mixdown of every channel (equal-weight average).
pub(crate) fn mono_mix(audio: &AudioData) -> Vec<f32> {
    let nch = audio.channels.len().max(1);
    let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
    let mut mono = vec![0.0f32; n];
    for ch in &audio.channels {
        for (i, &s) in ch.iter().enumerate() {
            mono[i] += s;
        }
    }
    for s in &mut mono {
        *s /= nch as f32;
    }
    mono
}

/// Compute the STFT magnitude spectrogram of a mono signal.
pub(crate) fn compute_spectrogram(
    mono: &[f32],
    sample_rate: u32,
    fft_size: usize,
    hop: usize,
) -> Spectrogram {
    cathar::spectrogram(mono, sample_rate, fft_size, hop)
}

/// Colour a spectrogram into an egui image using a `[db_floor, db_ceil]` display
/// window (the "gain"/contrast control). The image is `frames` wide × `bins`
/// tall, with row 0 = Nyquist and the last row = DC.
pub(crate) fn colorize(spec: &Spectrogram, db_floor: f32, db_ceil: f32) -> ColorImage {
    let frames = spec.frames();
    let bins = spec.bins;
    let w = frames.max(1);
    let h = bins.max(1);
    let mut pixels = vec![egui::Color32::BLACK; w * h];
    let range = (db_ceil - db_floor).max(1.0);
    for f in 0..frames {
        for bin in 0..bins {
            let db = spec.get(f, bin);
            let t = ((db - db_floor) / range).clamp(0.0, 1.0);
            // Image y=0 is the top → highest frequency. Flip the bin index.
            let y = bins - 1 - bin;
            pixels[y * w + f] = magma(t);
        }
    }
    ColorImage { size: [w, h], pixels }
}

/// Reduce a signal to `buckets` (min, max) pairs for waveform drawing.
pub(crate) fn waveform_envelope(mono: &[f32], buckets: usize) -> Vec<(f32, f32)> {
    let buckets = buckets.max(1);
    if mono.is_empty() {
        return vec![(0.0, 0.0); buckets];
    }
    let per = (mono.len() as f32 / buckets as f32).max(1.0);
    (0..buckets)
        .map(|b| {
            let start = (b as f32 * per) as usize;
            let end = (((b + 1) as f32 * per) as usize).min(mono.len()).max(start + 1);
            let slice = &mono[start..end.min(mono.len())];
            let mut lo = 0.0f32;
            let mut hi = 0.0f32;
            for &s in slice {
                lo = lo.min(s);
                hi = hi.max(s);
            }
            (lo, hi)
        })
        .collect()
}
