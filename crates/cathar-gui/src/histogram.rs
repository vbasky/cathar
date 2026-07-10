//! Amplitude histogram — level distribution of the current buffer
//! (or selection), drawn as a vertical bar graph in dBFS.

use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, pos2};

use crate::theme;

/// Number of dB bins from `floor_db` … 0 dBFS.
const BINS: usize = 48;
const FLOOR_DB: f32 = -96.0;

/// Histogram counts (normalised 0…1) for L and optional R.
#[derive(Clone, Default)]
pub(crate) struct LevelHistogram {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
}

impl LevelHistogram {
    pub(crate) fn compute(channels: &[Vec<f32>]) -> Self {
        let left = channels.first().map(Vec::as_slice).unwrap_or(&[]);
        let right = if channels.len() >= 2 { channels[1].as_slice() } else { &[] };
        let (hl, pl, rl) = hist_channel(left);
        let (hr, pr, rr) =
            if right.is_empty() { (Vec::new(), 0.0, 0.0) } else { hist_channel(right) };
        Self { left: hl, right: hr, peak_l: pl, peak_r: pr, rms_l: rl, rms_r: rr }
    }

    /// Draw into `ui` at the given height. Returns the allocated rect.
    pub(crate) fn show(&self, ui: &mut Ui, height: f32) -> Rect {
        let w = ui.available_width().max(80.0);
        let (resp, painter) = ui.allocate_painter(egui::vec2(w, height), Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, theme::RADIUS_MD, theme::well_bg());
        painter.rect_stroke(rect, theme::RADIUS_MD, theme::stroke_hairline());

        let pad = 4.0;
        let inner = rect.shrink(pad);
        let dual = !self.right.is_empty();
        let gap = if dual { 3.0 } else { 0.0 };
        let col_w = if dual { (inner.width() - gap) * 0.5 } else { inner.width() };

        draw_bars(
            &painter,
            Rect::from_min_size(inner.min, egui::vec2(col_w, inner.height())),
            &self.left,
            theme::wave_l(),
        );
        if dual {
            draw_bars(
                &painter,
                Rect::from_min_size(
                    pos2(inner.left() + col_w + gap, inner.top()),
                    egui::vec2(col_w, inner.height()),
                ),
                &self.right,
                theme::wave_r(),
            );
        }

        let font = FontId::proportional(9.0);
        painter.text(
            pos2(rect.left() + 4.0, rect.top() + 2.0),
            egui::Align2::LEFT_TOP,
            "0 dB",
            font.clone(),
            theme::text_muted(),
        );
        painter.text(
            pos2(rect.left() + 4.0, rect.bottom() - 2.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{FLOOR_DB:.0}"),
            font,
            theme::text_muted(),
        );

        let stats = if dual {
            format!(
                "L pk {:.1}  rms {:.1}   R pk {:.1}  rms {:.1}",
                self.peak_l, self.rms_l, self.peak_r, self.rms_r
            )
        } else {
            format!("pk {:.1} dBFS   rms {:.1} dBFS", self.peak_l, self.rms_l)
        };
        painter.text(
            pos2(rect.right() - 4.0, rect.top() + 2.0),
            egui::Align2::RIGHT_TOP,
            stats,
            FontId::proportional(10.0),
            theme::text().gamma_multiply(0.85),
        );

        rect
    }
}

fn hist_channel(samples: &[f32]) -> (Vec<f32>, f32, f32) {
    let mut counts = vec![0.0f32; BINS];
    if samples.is_empty() {
        return (counts, -120.0, -120.0);
    }
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    for &s in samples {
        let a = s.abs();
        peak = peak.max(a);
        sum_sq += (s as f64) * (s as f64);
        let db = if a < 1e-10 { FLOOR_DB } else { 20.0 * a.log10() };
        let t = ((db - FLOOR_DB) / (0.0 - FLOOR_DB)).clamp(0.0, 1.0);
        let bin = ((1.0 - t) * (BINS as f32 - 0.001)) as usize;
        counts[bin.min(BINS - 1)] += 1.0;
    }
    let max_c = counts.iter().copied().fold(0.0f32, f32::max).max(1.0);
    for c in &mut counts {
        *c /= max_c;
    }
    let peak_db = 20.0 * peak.max(1e-10).log10();
    let rms = (sum_sq / samples.len() as f64).sqrt() as f32;
    let rms_db = 20.0 * rms.max(1e-10).log10();
    (counts, peak_db, rms_db)
}

fn draw_bars(painter: &egui::Painter, rect: Rect, bins: &[f32], color: Color32) {
    if bins.is_empty() || rect.width() < 2.0 {
        return;
    }
    let n = bins.len();
    let bh = rect.height() / n as f32;
    for (i, &v) in bins.iter().enumerate() {
        if v <= 0.001 {
            continue;
        }
        let y0 = rect.top() + i as f32 * bh;
        let bar_w = (v * rect.width()).max(1.0);
        // Keep bars solid (high alpha) so they don’t read as pastel wash.
        let c = Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            (160.0 + 95.0 * v) as u8,
        );
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(rect.left(), y0), egui::vec2(bar_w, bh.max(1.0) - 0.5)),
            0.0,
            c,
        );
    }
    // Clip zone near 0 dB.
    painter.rect_filled(
        Rect::from_min_size(rect.min, egui::vec2(rect.width(), bh * 2.0)),
        0.0,
        Color32::from_rgba_unmultiplied(255, 72, 64, 36),
    );
}
