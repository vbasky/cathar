//! Time and frequency axis rulers for the spectrogram and waveform.

use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, pos2};

pub(crate) const FREQ_AXIS_W: f32 = 52.0;
pub(crate) const TIME_AXIS_H: f32 = 22.0;

/// Image area inside the outer painter rect (freq gutter left, time gutter below).
pub(crate) fn spectro_image_rect(outer: Rect, image_w: f32, image_h: f32) -> Rect {
    Rect::from_min_size(pos2(outer.left() + FREQ_AXIS_W, outer.top()), egui::vec2(image_w, image_h))
}

/// Nice tick positions for 0..=max (at least two ticks when max > 0).
pub(crate) fn nice_ticks(max: f32, target_count: usize) -> Vec<f32> {
    if max <= 0.0 {
        return vec![0.0];
    }
    let raw = max / target_count.max(1) as f32;
    let mag = 10f32.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm < 1.5 {
        mag
    } else if norm < 3.5 {
        2.0 * mag
    } else if norm < 7.5 {
        5.0 * mag
    } else {
        10.0 * mag
    };
    let mut ticks = Vec::new();
    let mut v = 0.0;
    while v <= max + step * 0.01 {
        ticks.push(v);
        v += step;
    }
    if ticks.last().copied().unwrap_or(0.0) < max * 0.85 {
        ticks.push(max);
    }
    ticks
}

pub(crate) fn draw_freq_axis(
    painter: &Painter,
    outer: Rect,
    image: Rect,
    nyquist: f32,
    text: Color32,
) {
    let axis =
        Rect::from_min_max(pos2(outer.left(), image.top()), pos2(image.left(), image.bottom()));
    painter.rect_filled(axis, 0.0, Color32::from_black_alpha(12));
    painter.line_segment(
        [pos2(image.left(), image.top()), pos2(image.left(), image.bottom())],
        Stroke::new(1.0, text.gamma_multiply(0.35)),
    );

    let font = FontId::proportional(10.0);
    for f in nice_ticks(nyquist, 6) {
        let t = 1.0 - (f / nyquist).clamp(0.0, 1.0);
        let y = image.top() + t * image.height();
        painter.line_segment(
            [pos2(image.left() - 4.0, y), pos2(image.left(), y)],
            Stroke::new(1.0, text.gamma_multiply(0.45)),
        );
        let label = fmt_hz(f);
        painter.text(
            pos2(axis.right() - 3.0, y),
            Align2::RIGHT_CENTER,
            label,
            font.clone(),
            text.gamma_multiply(0.85),
        );
    }
    painter.text(
        pos2(axis.center().x, axis.top() - 2.0),
        Align2::CENTER_BOTTOM,
        "Hz",
        FontId::proportional(9.0),
        text.gamma_multiply(0.55),
    );
}

pub(crate) fn draw_time_axis(
    painter: &Painter,
    outer: Rect,
    image: Rect,
    duration: f32,
    text: Color32,
) {
    let axis =
        Rect::from_min_max(pos2(image.left(), image.bottom()), pos2(image.right(), outer.bottom()));
    painter.rect_filled(axis, 0.0, Color32::from_black_alpha(12));
    painter.line_segment(
        [pos2(image.left(), image.bottom()), pos2(image.right(), image.bottom())],
        Stroke::new(1.0, text.gamma_multiply(0.35)),
    );

    let font = FontId::proportional(10.0);
    for t in nice_ticks(duration, 8) {
        let x = image.left() + (t / duration).clamp(0.0, 1.0) * image.width();
        painter.line_segment(
            [pos2(x, image.bottom()), pos2(x, image.bottom() + 4.0)],
            Stroke::new(1.0, text.gamma_multiply(0.45)),
        );
        painter.text(
            pos2(x, axis.top() + 3.0),
            Align2::LEFT_TOP,
            fmt_time_axis(t),
            font.clone(),
            text.gamma_multiply(0.85),
        );
    }
}

fn fmt_hz(hz: f32) -> String {
    if hz >= 10_000.0 {
        format!("{:.0}k", hz / 1000.0)
    } else if hz >= 1000.0 {
        format!("{:.1}k", hz / 1000.0)
    } else {
        format!("{:.0}", hz)
    }
}

fn fmt_time_axis(secs: f32) -> String {
    if secs >= 60.0 {
        let m = (secs / 60.0).floor() as u32;
        let s = secs - m as f32 * 60.0;
        format!("{m}:{s:04.1}")
    } else {
        format!("{secs:.1}s")
    }
}
