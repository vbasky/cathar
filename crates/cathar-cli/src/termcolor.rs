//! Terminal color-depth adaptation. Emit 24-bit truecolor when the terminal
//! advertises it (`COLORTERM`), otherwise map each RGB value to the nearest
//! xterm-256 palette index so the gradients still look right on 256-color
//! terminals (e.g. macOS Terminal.app).

use ratatui::style::Color;

/// True if the terminal advertises 24-bit truecolor.
pub(crate) fn supports_truecolor() -> bool {
    std::env::var("COLORTERM")
        .map(|c| {
            let c = c.to_ascii_lowercase();
            c.contains("truecolor") || c.contains("24bit")
        })
        .unwrap_or(false)
}

/// An RGB color, downsampled to the xterm-256 palette when `truecolor` is false.
pub(crate) fn rgb(r: u8, g: u8, b: u8, truecolor: bool) -> Color {
    if truecolor { Color::Rgb(r, g, b) } else { Color::Indexed(to_xterm256(r, g, b)) }
}

/// Nearest xterm-256 index: the 6×6×6 color cube (16–231) or the grayscale
/// ramp (232–255), whichever is closer to the requested RGB.
fn to_xterm256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: i32| -> usize {
        let mut best = 0;
        let mut bd = i32::MAX;
        for (i, &l) in LEVELS.iter().enumerate() {
            let d = (v - l).abs();
            if d < bd {
                bd = d;
                best = i;
            }
        }
        best
    };
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let cube_idx = 16 + 36 * ri + 6 * gi + bi;

    // Nearest gray on the 232..=255 ramp (values 8, 18, … 238).
    let gray_i = (((r + g + b) / 3 - 8) as f32 / 10.0).round().clamp(0.0, 23.0) as i32;
    let gray_v = 8 + 10 * gray_i;
    let gray_idx = 232 + gray_i as usize;

    let dist = |c: (i32, i32, i32)| (c.0 - r).pow(2) + (c.1 - g).pow(2) + (c.2 - b).pow(2);
    if dist((gray_v, gray_v, gray_v)) < dist(cube) { gray_idx as u8 } else { cube_idx as u8 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_anchors() {
        assert_eq!(to_xterm256(0, 0, 0), 16); // cube black
        assert_eq!(to_xterm256(255, 255, 255), 231); // cube white
        assert_eq!(to_xterm256(128, 128, 128), 244); // grayscale ramp
    }

    #[test]
    fn truecolor_passthrough_vs_downsample() {
        assert!(matches!(rgb(10, 20, 30, true), Color::Rgb(10, 20, 30)));
        assert!(matches!(rgb(10, 20, 30, false), Color::Indexed(_)));
    }
}
