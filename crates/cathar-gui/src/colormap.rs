//! Cathar spectrogram colour ramp.
//!
//! Intentionally **not** the industry blue→cyan→orange map used by RX and many
//! commercial editors. Cathar's ramp follows the app identity: warm ink silence,
//! teal energy, ember hotspots — "cleansed" audio reading as sea-glass heat.

use egui::Color32;

/// Default display ramp: ink → deep teal → vivid jade → gold → hot ember.
/// Mid/high stops stay saturated so energy doesn’t read as pastel wash.
const CATHAR_STOPS: [(f32, (u8, u8, u8)); 7] = [
    (0.00, (8, 8, 8)),       // ink
    (0.14, (8, 48, 52)),     // deep teal
    (0.32, (0, 120, 118)),   // strong teal
    (0.50, (0, 190, 150)),   // saturated jade
    (0.68, (220, 150, 30)),  // gold
    (0.85, (255, 110, 30)),  // hot ember
    (1.00, (255, 230, 160)), // bright peak
];

/// Magma (parity with `cathar view` TUI) — available if we wire a menu toggle later.
#[allow(dead_code)]
const MAGMA_STOPS: [(f32, (u8, u8, u8)); 5] = [
    (0.00, (0, 0, 4)),
    (0.25, (43, 17, 86)),
    (0.50, (114, 31, 107)),
    (0.75, (216, 71, 68)),
    (1.00, (252, 253, 191)),
];

/// Map a normalised level `t` in `[0, 1]` to Cathar spectrogram colour.
pub(crate) fn cathar(t: f32) -> Color32 {
    sample(t, &CATHAR_STOPS)
}

/// Magma ramp (TUI parity).
#[allow(dead_code)]
pub(crate) fn magma(t: f32) -> Color32 {
    sample(t, &MAGMA_STOPS)
}

fn sample(t: f32, stops: &[(f32, (u8, u8, u8))]) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f).round() as u8;
            return Color32::from_rgb(lerp(c0.0, c1.0), lerp(c0.1, c1.1), lerp(c0.2, c1.2));
        }
    }
    let last = stops.last().map(|s| s.1).unwrap_or((255, 255, 255));
    Color32::from_rgb(last.0, last.1, last.2)
}
