//! Magma colour ramp — matches the `cathar view` terminal spectrogram so the
//! GUI and TUI share one visual identity.

use egui::Color32;

const STOPS: [(f32, (u8, u8, u8)); 5] = [
    (0.00, (0, 0, 4)),
    (0.25, (43, 17, 86)),
    (0.50, (114, 31, 107)),
    (0.75, (216, 71, 68)),
    (1.00, (252, 253, 191)),
];

/// Map a normalised level `t` in `[0, 1]` to a magma RGB colour.
pub(crate) fn magma(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f).round() as u8;
            return Color32::from_rgb(lerp(c0.0, c1.0), lerp(c0.1, c1.1), lerp(c0.2, c1.2));
        }
    }
    Color32::from_rgb(252, 253, 191)
}
