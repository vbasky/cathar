//! Turn a decoded signal into an egui image (spectrogram) and a waveform
//! peak envelope, reusing `cathar::spectrogram` for the STFT.

use crate::colormap::cathar;
use cathar::{AudioData, Spectrogram};
use egui::ColorImage;

/// Which channel(s) the spectrogram / waveform represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ChannelView {
    /// Mid = (L+R)/2 — always available.
    #[default]
    Mid,
    Left,
    Right,
    /// Stacked L (top) + R (bottom) spectrograms.
    Split,
}

impl ChannelView {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Mid => "L+R",
            Self::Left => "L",
            Self::Right => "R",
            Self::Split => "L|R",
        }
    }
}

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

/// Samples for a display channel view. `Right` on mono falls back to the only channel.
pub(crate) fn channel_samples(audio: &AudioData, view: ChannelView) -> Vec<f32> {
    match view {
        ChannelView::Mid => mono_mix(audio),
        ChannelView::Left | ChannelView::Split => {
            audio.channels.first().cloned().unwrap_or_default()
        }
        ChannelView::Right => {
            if audio.channels.len() >= 2 {
                audio.channels[1].clone()
            } else {
                audio.channels.first().cloned().unwrap_or_default()
            }
        }
    }
}

/// True when the buffer has at least two channels.
pub(crate) fn is_stereo(audio: &AudioData) -> bool {
    audio.channels.len() >= 2
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
/// window. The image is `frames` wide × `bins` tall, with row 0 = Nyquist.
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
            let y = bins - 1 - bin;
            pixels[y * w + f] = cathar(t);
        }
    }
    ColorImage { size: [w, h], pixels }
}

/// Stack two spectrogram images vertically (L on top, R on bottom) with a 2px gap.
pub(crate) fn stack_vertical(top: &ColorImage, bottom: &ColorImage) -> ColorImage {
    let w = top.size[0].max(bottom.size[0]).max(1);
    let gap = 2usize;
    let h = top.size[1] + gap + bottom.size[1];
    let mut pixels = vec![egui::Color32::from_rgb(14, 13, 12); w * h];
    blit(top, &mut pixels, w, 0);
    blit(bottom, &mut pixels, w, top.size[1] + gap);
    ColorImage { size: [w, h], pixels }
}

fn blit(src: &ColorImage, dst: &mut [egui::Color32], dst_w: usize, y0: usize) {
    let sw = src.size[0];
    let sh = src.size[1];
    for y in 0..sh {
        for x in 0..sw.min(dst_w) {
            dst[(y0 + y) * dst_w + x] = src.pixels[y * sw + x];
        }
    }
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
