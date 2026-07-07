//! Use the host OS system font instead of egui's bundled Ubuntu-Light.
//!
//! egui ships Ubuntu-Light as its default proportional face, which reads as
//! non-native on every platform. We load the platform's own UI font from a
//! well-known path (pure `std::fs`, no `font-kit`/fontconfig C dependency),
//! validate it parses with `ab_glyph` (the same rasteriser egui uses), and put
//! it first in the proportional family — egui's fonts stay as emoji/fallback.

use egui::{FontData, FontDefinitions, FontFamily};

/// Ordered candidate paths for the native UI sans-serif face.
fn sans_candidates() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &[
            "/System/Library/Fonts/SFNS.ttf", // San Francisco (system UI font)
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ]
    }
    #[cfg(target_os = "windows")]
    {
        &["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\arial.ttf"]
    }
    #[cfg(target_os = "linux")]
    {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        &[]
    }
}

/// Read the first candidate that both exists and parses as a font.
fn load_first_valid(paths: &[&str]) -> Option<(String, Vec<u8>)> {
    for path in paths {
        let Ok(bytes) = std::fs::read(path) else { continue };
        if ab_glyph::FontVec::try_from_vec(bytes.clone()).is_err() {
            continue; // e.g. a .ttc collection ab_glyph can't open standalone
        }
        let name = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("system")
            .to_string();
        return Some((name, bytes));
    }
    None
}

/// Install the system font on `ctx`. Returns the font's name on success, or
/// `None` when no candidate was usable (egui keeps its default — no harm).
pub(crate) fn install_system_font(ctx: &egui::Context) -> Option<String> {
    let (name, bytes) = load_first_valid(sans_candidates())?;
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(name.clone(), FontData::from_owned(bytes));
    // Prepend to Proportional so it wins; keep egui's faces behind it as
    // fallback (they cover glyphs the system font may lack, e.g. emoji).
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, name.clone());
    // Also make it the last-resort monospace fallback.
    fonts.families.entry(FontFamily::Monospace).or_default().push(name.clone());
    ctx.set_fonts(fonts);
    Some(name)
}
