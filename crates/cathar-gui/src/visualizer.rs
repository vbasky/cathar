//! Classic spectrum-bar visualizer (WMP / iTunes “Bars” era).
//!
//! Pulls a short mono window around the playhead, runs a real FFT, and maps
//! log-spaced bands to bouncing bars with peak-hold chips.
//!
//! Magnitudes are **normalized + dB-mapped** so bars form a spectrum hill
//! instead of clipping to a solid green wall.

use egui::{Color32, FontId, Rect, Sense, Stroke, Ui, pos2};
use realfft::RealFftPlanner;

use crate::theme;

const FFT_SIZE: usize = 2048;
const N_BARS: usize = 48;
/// Hann coherent gain ≈ 0.5 — used with 1/N so unit-sine peaks near 0 dBFS.
const HANN_COHERENT: f32 = 0.5;
/// Display window: silence floor → full-scale.
const DB_FLOOR: f32 = -55.0;
const DB_CEIL: f32 = -2.0;

pub(crate) struct SpectrumViz {
    /// Smoothed bar heights 0…1.
    bars: Vec<f32>,
    /// Peak-hold chips (fall slowly).
    peaks: Vec<f32>,
    scratch: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex32>,
    planner: RealFftPlanner<f32>,
}

impl Default for SpectrumViz {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumViz {
    pub(crate) fn new() -> Self {
        Self {
            bars: vec![0.0; N_BARS],
            peaks: vec![0.0; N_BARS],
            scratch: vec![0.0; FFT_SIZE],
            spectrum: vec![realfft::num_complex::Complex32::default(); FFT_SIZE / 2 + 1],
            planner: RealFftPlanner::new(),
        }
    }

    /// Update bars from mono samples around the playhead.
    pub(crate) fn tick(&mut self, mono: &[f32], sample_rate: u32, pos_sec: f32, playing: bool) {
        if mono.is_empty() || sample_rate == 0 {
            self.decay_only();
            return;
        }

        let sr = sample_rate as f32;
        let center = (pos_sec * sr).round() as isize;
        let half = (FFT_SIZE / 2) as isize;
        let start = (center - half).max(0) as usize;

        // Hann window into scratch
        for i in 0..FFT_SIZE {
            let idx = start + i;
            let s = mono.get(idx).copied().unwrap_or(0.0);
            let w = 0.5
                * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE as f32 - 1.0)).cos());
            self.scratch[i] = s * w;
        }

        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        if fft.process(&mut self.scratch, &mut self.spectrum).is_err() {
            self.decay_only();
            return;
        }

        let n_bins = self.spectrum.len();
        let nyquist = sr * 0.5;
        // Log frequency bands from ~40 Hz to Nyquist
        let f_lo = 40.0_f32;
        let f_hi = nyquist.max(f_lo * 2.0);
        // Amplitude scale: unwindowed DFT peak ~ N/2 for full-scale sine; Hann ≈ N*0.5/2.
        let amp_scale = (FFT_SIZE as f32) * HANN_COHERENT;

        let attack = if playing { 0.45 } else { 0.22 };
        let release = if playing { 0.28 } else { 0.12 };
        let peak_fall = if playing { 0.018 } else { 0.05 };

        for b in 0..N_BARS {
            let t0 = b as f32 / N_BARS as f32;
            let t1 = (b + 1) as f32 / N_BARS as f32;
            let f0 = f_lo * (f_hi / f_lo).powf(t0);
            let f1 = f_lo * (f_hi / f_lo).powf(t1);
            let i0 = (((f0 / nyquist) * (n_bins as f32 - 1.0))
                .floor()
                .clamp(0.0, (n_bins - 1) as f32)) as usize;
            let i1 = ((((f1 / nyquist) * (n_bins as f32 - 1.0))
                .ceil()
                .clamp(0.0, (n_bins - 1) as f32)) as usize)
                .max(i0);

            // Mean power in band, then amplitude (more stable than max bin).
            let mut power = 0.0_f32;
            let mut count = 0usize;
            for i in i0..=i1 {
                let c = self.spectrum[i];
                // realfft: DC/Nyquist once; others often 2× energy in one-sided — use |X|/scale.
                let re = c.re / amp_scale;
                let im = c.im / amp_scale;
                power += re * re + im * im;
                count += 1;
            }
            if count > 0 {
                power /= count as f32;
            }
            let mag = power.sqrt();
            // dB relative to full-scale sine ≈ 0 dBFS
            let db = 20.0 * (mag.max(1e-12)).log10();
            let mut level = ((db - DB_FLOOR) / (DB_CEIL - DB_FLOOR)).clamp(0.0, 1.0);
            // Mild gamma so quiet mid-range still moves without pegging loud bins.
            level = level.powf(0.85);

            let cur = self.bars[b];
            let next = if level > cur {
                cur + (level - cur) * attack
            } else {
                cur + (level - cur) * release
            };
            self.bars[b] = next.clamp(0.0, 1.0);

            if self.bars[b] > self.peaks[b] {
                self.peaks[b] = self.bars[b];
            } else {
                self.peaks[b] = (self.peaks[b] - peak_fall).max(self.bars[b]);
            }
        }
    }

    fn decay_only(&mut self) {
        for b in 0..N_BARS {
            self.bars[b] *= 0.88;
            self.peaks[b] = (self.peaks[b] - 0.03).max(self.bars[b]);
        }
    }

    /// Paint bars + peak chips using the active Cathar palette (dark/light).
    pub(crate) fn show(&self, ui: &mut Ui, track_title: &str) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
        let rect = resp.rect;
        // Same well as spectro empty/chrome — not pure WMP black/lime.
        painter.rect_filled(rect, 0.0, theme::well_bg());

        if !track_title.is_empty() {
            painter.text(
                pos2(rect.left() + 16.0, rect.top() + 14.0),
                egui::Align2::LEFT_TOP,
                track_title,
                FontId::proportional(18.0),
                theme::text(),
            );
        }
        painter.text(
            pos2(rect.left() + 16.0, rect.top() + 40.0),
            egui::Align2::LEFT_TOP,
            "Bars",
            FontId::proportional(12.0),
            theme::text_muted(),
        );

        let plot = Rect::from_min_max(
            pos2(rect.left() + 12.0, rect.top() + 64.0),
            pos2(rect.right() - 12.0, rect.bottom() - 28.0),
        );
        if plot.width() < 8.0 || plot.height() < 8.0 {
            return;
        }

        let gap = 2.0_f32;
        let bar_w = ((plot.width() - gap * (N_BARS as f32 - 1.0)) / N_BARS as f32).max(1.0);
        let accent = theme::accent();
        let peak_c = theme::wave_r(); // warm ember chip — reads on teal bars
        let base_line = theme::hairline();

        for (i, (&h, &pk)) in self.bars.iter().zip(self.peaks.iter()).enumerate() {
            let x = plot.left() + i as f32 * (bar_w + gap);
            let h_px = (h * plot.height()).max(if h > 0.01 { 2.0 } else { 0.0 });
            let y1 = plot.bottom();
            let y0 = y1 - h_px;
            let bar = Rect::from_min_max(pos2(x, y0), pos2(x + bar_w, y1));

            // Height → Cathar spectro ramp (ink/teal → gold → ember at hot peaks).
            let fill = bar_color(h);
            painter.rect_filled(bar, 1.5, fill);
            if h_px > 5.0 {
                let tip_h = (h_px * 0.18).clamp(3.0, 8.0);
                let tip = Rect::from_min_max(pos2(x, y0), pos2(x + bar_w, y0 + tip_h));
                painter.rect_filled(tip, 1.5, lighten(fill, 0.35));
            }
            // Peak chip
            if pk > 0.02 {
                let py = plot.bottom() - pk * plot.height();
                let chip = Rect::from_center_size(
                    pos2(x + bar_w * 0.5, py),
                    egui::vec2(bar_w.max(2.0), 3.0),
                );
                painter.rect_filled(chip, 0.5, peak_c.gamma_multiply(0.95));
                painter.rect_stroke(chip, 0.5, Stroke::new(0.5, accent.gamma_multiply(0.5)));
            }
        }

        painter.line_segment(
            [pos2(plot.left(), plot.bottom()), pos2(plot.right(), plot.bottom())],
            Stroke::new(1.0, base_line),
        );
    }
}

/// Map bar height to Cathar spectrogram-adjacent colour (teal body → gold/ember tip).
fn bar_color(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    // Reuse spectro identity without pure lime.
    crate::colormap::cathar(0.28 + t * 0.62)
}

fn lighten(c: Color32, amount: f32) -> Color32 {
    let a = amount.clamp(0.0, 1.0);
    let lerp = |x: u8| -> u8 {
        let v = x as f32 + (255.0 - x as f32) * a;
        v.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(lerp(c.r()), lerp(c.g()), lerp(c.b()), c.a())
}
