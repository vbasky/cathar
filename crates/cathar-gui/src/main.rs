//! Cathar GUI — pure-Rust spectral editor for the `cathar` restoration library.
//! Open a file, inspect L/R spectrograms, run restoration modules, play back
//! and save. Layout borrows pro-editor workflow patterns; colours are Cathar's
//! own (jade / ink / ember), not a commercial skin clone.

mod app;
mod axes;
mod colormap;
mod engine;
mod fonts;
mod histogram;
mod icons;
mod native_menu;
mod panel;
mod spectral_edit;
mod spectro;
mod theme;
mod visualizer;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app::CatharGui;
use egui::IconData;

/// Product name shown in the OS menu bar, Dock, and window title.
pub(crate) const APP_NAME: &str = "Cathar";

/// Guards against re-entrant Ctrl+C while a previous handler is still running.
static CTRL_C_SEEN: AtomicBool = AtomicBool::new(false);

fn install_ctrl_c_handler() {
    // Console Ctrl+C does not reliably deliver a winit event, so the eframe loop
    // may never notice a graceful close flag if the app is idle. On Windows,
    // dropping cpal/rodio streams during normal process teardown can also hang
    // indefinitely. Use an immediate hard exit (skip destructors).
    let result = ctrlc::set_handler(|| {
        if CTRL_C_SEEN.swap(true, Ordering::SeqCst) {
            // Second Ctrl+C while a previous exit is stuck — force again.
            std::process::exit(130);
        }
        eprintln!("\nInterrupted — exiting {APP_NAME}.");
        // `process::exit` does not run Drop; avoids rodio/cpal hang on Windows.
        std::process::exit(130);
    });
    if let Err(e) = result {
        // Non-fatal: GUI still works; only console interrupt is unavailable.
        eprintln!("note: Ctrl+C handler not installed ({e})");
    }
}

fn main() -> eframe::Result<()> {
    // Must run before NSApplication is created — menu bar uses process name.
    #[cfg(target_os = "macos")]
    macos::set_process_name(APP_NAME);

    install_ctrl_c_handler();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([900.0, 560.0])
        .with_title(APP_NAME);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        // Disable winit's built-in macOS menu so only our `muda` menus appear.
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::EventLoopBuilderExtMacOS;
                builder.with_default_menu(false);
            }
            let _ = builder;
        })),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(CatharGui::new(cc)))))
}

/// Per-OS icon layout so the mark sits correctly under each platform’s chrome.
///
/// | Platform | Plate inset | Mark fill of plate |
/// |----------|-------------|--------------------|
/// | **macOS** | ~10% transparent margin (matches Dock optical size) | ~82% |
/// | **Windows** | ~4% margin | ~92% |
/// | **Linux** | ~6% margin | ~86% |
struct IconLayout {
    /// Output canvas size (multiple of 4 for egui).
    size: u32,
    /// Rounded plate as a fraction of the canvas (1.0 = edge-to-edge — reads too big in Dock).
    plate_scale: f32,
    /// Long side of the mark as a fraction of the **plate** (not the full canvas).
    mark_fill: f32,
    /// Corner radius as a fraction of the plate side (~0.22 ≈ Big Sur continuous corner).
    corner_radius_frac: f32,
    /// If true, force α=255 everywhere.
    force_opaque: bool,
}

fn icon_layout() -> IconLayout {
    #[cfg(target_os = "macos")]
    {
        // Unbundled apps get no asset-catalog squircle — bake shape + inset so
        // the tile matches neighbouring Dock icons (full-bleed plates look larger).
        // ~80% plate = optical size of typical Dock icons next to system apps.
        IconLayout {
            size: 512,
            plate_scale: 0.80,
            mark_fill: 0.84,
            corner_radius_frac: 0.2237,
            force_opaque: false,
        }
    }
    #[cfg(target_os = "windows")]
    {
        IconLayout {
            size: 256,
            plate_scale: 0.96,
            mark_fill: 0.92,
            corner_radius_frac: 0.12,
            force_opaque: false,
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        IconLayout {
            size: 256,
            plate_scale: 0.94,
            mark_fill: 0.86,
            corner_radius_frac: 0.22,
            force_opaque: false,
        }
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        all(unix, not(target_os = "macos"))
    )))]
    {
        IconLayout {
            size: 256,
            plate_scale: 0.92,
            mark_fill: 0.88,
            corner_radius_frac: 0.18,
            force_opaque: false,
        }
    }
}

/// Window / Dock / taskbar icon from the bundled Cathar mark (`docs/logo.png`).
///
/// Always **light paper plate + dark mark** (terracotta accents kept). Source
/// art is white-on-navy; we key the navy field and recolor silver → charcoal so
/// the tile reads on dark Docks and light taskbars alike.
fn app_icon() -> Option<IconData> {
    let layout = icon_layout();
    let bytes = include_bytes!("../../../docs/logo.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    // Source plate / key colour (logo corner).
    let key = *img.get_pixel(0, 0); // ~rgb(19, 27, 38)
    let near = |a: u8, b: u8| (a as i32 - b as i32).abs() < 28;

    // Crop to the emblem — the asset has large empty margins.
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..h {
        for x in 0..w {
            let px = img.get_pixel(x, y);
            if near(px[0], key[0]) && near(px[1], key[1]) && near(px[2], key[2]) {
                continue;
            }
            any = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !any {
        min_x = 0;
        min_y = 0;
        max_x = w.saturating_sub(1);
        max_y = h.saturating_sub(1);
    }
    let crop_w = (max_x - min_x + 1).max(1);
    let crop_h = (max_y - min_y + 1).max(1);
    let cropped = image::imageops::crop_imm(&img, min_x, min_y, crop_w, crop_h).to_image();

    let size = layout.size;
    let plate_side = ((size as f32) * layout.plate_scale).round().clamp(16.0, size as f32) as u32;
    let plate_side = plate_side.max(16);

    let content = ((plate_side as f32) * layout.mark_fill).round().max(8.0);
    let scale = content / (crop_w.max(crop_h) as f32);
    let mark_w = ((crop_w as f32) * scale).round().max(1.0) as u32;
    let mark_h = ((crop_h as f32) * scale).round().max(1.0) as u32;
    let mark =
        image::imageops::resize(&cropped, mark_w, mark_h, image::imageops::FilterType::Lanczos3);

    // Warm paper plate + soft stone rim (high contrast on dark Dock).
    let plate_fill = image::Rgba([248u8, 246, 242, 255]);
    let rim = image::Rgba([185u8, 176, 164, 255]);

    let mut canvas = image::RgbaImage::from_pixel(size, size, image::Rgba([0, 0, 0, 0]));
    let mut plate_img =
        image::RgbaImage::from_pixel(plate_side, plate_side, image::Rgba([0, 0, 0, 0]));
    fill_rounded_rect(&mut plate_img, plate_fill, layout.corner_radius_frac);

    let pox = (size - plate_side) / 2;
    let poy = (size - plate_side) / 2;
    for y in 0..plate_side {
        for x in 0..plate_side {
            let px = *plate_img.get_pixel(x, y);
            if px[3] > 0 {
                canvas.put_pixel(x + pox, y + poy, px);
            }
        }
    }

    stroke_rounded_rect_rim(
        &mut canvas,
        pox,
        poy,
        plate_side,
        layout.corner_radius_frac,
        rim,
        (size as f32 * 0.014).clamp(1.5, 6.0),
    );

    let ox = pox + (plate_side - mark_w) / 2;
    let oy = poy + (plate_side - mark_h) / 2;
    for y in 0..mark_h {
        for x in 0..mark_w {
            let px = *mark.get_pixel(x, y);
            if near(px[0], key[0]) && near(px[1], key[1]) && near(px[2], key[2]) {
                continue;
            }
            let dx = x + ox;
            let dy = y + oy;
            if dx >= size || dy >= size {
                continue;
            }
            let dest = canvas.get_pixel(dx, dy);
            if dest[3] == 0 {
                continue;
            }
            let out = mark_dark_on_light(px);
            canvas.put_pixel(dx, dy, image::Rgba([out[0], out[1], out[2], 255]));
        }
    }

    let mut rgba = canvas.into_raw();
    if layout.force_opaque {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
    Some(IconData { rgba, width: size, height: size })
}

/// Light plate: force silver/white metal → solid charcoal; keep terracotta.
fn mark_dark_on_light(px: image::Rgba<u8>) -> image::Rgba<u8> {
    let r = px[0] as i32;
    let g = px[1] as i32;
    let b = px[2] as i32;
    let lum = (r + g + b) / 3;
    // Terracotta / warm brand accent — keep.
    let warm = r > g + 18 && r > b + 18 && r > 85;
    if warm {
        return px;
    }
    // Everything else that isn't already dark → near-black ink (main X + filigree).
    // Near-black ornament on the source stays; mid/light greys become ink.
    if lum < 55 {
        return px;
    }
    // Pure ink for the primary mark strokes.
    const INK: [u8; 3] = [22, 20, 18];
    // Soften only the dimmest grey AA fringe so edges stay anti-aliased.
    if lum < 120 {
        let t = ((lum - 55) as f32 / 65.0).clamp(0.0, 1.0);
        let mix = |c: u8, i: u8| ((c as f32) * (1.0 - t) + (i as f32) * t).round() as u8;
        return image::Rgba([mix(px[0], INK[0]), mix(px[1], INK[1]), mix(px[2], INK[2]), px[3]]);
    }
    image::Rgba([INK[0], INK[1], INK[2], px[3]])
}

/// Anti-aliased stroke along the rounded-rect edge of the plate on `canvas`.
fn stroke_rounded_rect_rim(
    canvas: &mut image::RgbaImage,
    ox: u32,
    oy: u32,
    side: u32,
    radius_frac: f32,
    color: image::Rgba<u8>,
    stroke_px: f32,
) {
    let w = side as f32;
    let h = side as f32;
    let r = (w.min(h) * radius_frac).clamp(1.0, w.min(h) * 0.5);
    let cx0 = r;
    let cy0 = r;
    let cx1 = w - 1.0 - r;
    let cy1 = h - 1.0 - r;
    let half = stroke_px * 0.5;

    for y in 0..side {
        for x in 0..side {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = if px < cx0 {
                cx0 - px
            } else if px > cx1 {
                px - cx1
            } else {
                0.0
            };
            let dy = if py < cy0 {
                cy0 - py
            } else if py > cy1 {
                py - cy1
            } else {
                0.0
            };
            let dist = (dx * dx + dy * dy).sqrt();
            // Distance to the rounded boundary: inside negative-ish via r - dist on corners;
            // for axis segments use distance to edge of AABB of centers.
            let d_edge = if dx == 0.0 && dy == 0.0 {
                // Interior of straight edges — distance to nearest outer edge.
                (px.min(py).min(w - px).min(h - py)).min(r)
            } else {
                // Near corner arcs: |dist - r|
                (dist - r).abs()
            };
            // Only draw a band near the outer perimeter, not deep inside.
            let near_perimeter = if dx == 0.0 && dy == 0.0 {
                let to_border = px.min(py).min(w - px).min(h - py);
                to_border < stroke_px + 0.5
            } else {
                d_edge < half + 0.75
            };
            if !near_perimeter {
                continue;
            }
            let band =
                if dx == 0.0 && dy == 0.0 { px.min(py).min(w - px).min(h - py) } else { d_edge };
            let alpha = if band <= half {
                200u8
            } else if band >= half + 0.85 {
                0u8
            } else {
                let t = 1.0 - (band - half) / 0.85;
                let t = t.clamp(0.0, 1.0);
                let t = t * t * (3.0 - 2.0 * t);
                (t * 200.0).round() as u8
            };
            if alpha == 0 {
                continue;
            }
            let dx = x + ox;
            let dy = y + oy;
            if dx >= canvas.width() || dy >= canvas.height() {
                continue;
            }
            let base = *canvas.get_pixel(dx, dy);
            if base[3] == 0 {
                continue; // only rim where plate exists
            }
            // Alpha-blend rim over plate.
            let a = alpha as f32 / 255.0;
            let blend = |c: u8, b: u8| ((c as f32) * a + (b as f32) * (1.0 - a)).round() as u8;
            canvas.put_pixel(
                dx,
                dy,
                image::Rgba([
                    blend(color[0], base[0]),
                    blend(color[1], base[1]),
                    blend(color[2], base[2]),
                    base[3],
                ]),
            );
        }
    }
}

/// Rounded-rect plate with anti-aliased edge (transparent outside).
///
/// `radius_frac` is the corner radius as a fraction of the short side
/// (≈0.22 matches macOS Big Sur icon template / continuous-corner look).
fn fill_rounded_rect(img: &mut image::RgbaImage, color: image::Rgba<u8>, radius_frac: f32) {
    let w = img.width() as f32;
    let h = img.height() as f32;
    // Full fraction of side — not half (half made corners look almost square).
    let r = (w.min(h) * radius_frac).clamp(1.0, w.min(h) * 0.5);
    let cx0 = r;
    let cy0 = r;
    let cx1 = w - 1.0 - r;
    let cy1 = h - 1.0 - r;

    for y in 0..img.height() {
        for x in 0..img.width() {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            // Distance from the rounded-rect interior (0 = inside).
            let dx = if px < cx0 {
                cx0 - px
            } else if px > cx1 {
                px - cx1
            } else {
                0.0
            };
            let dy = if py < cy0 {
                cy0 - py
            } else if py > cy1 {
                py - cy1
            } else {
                0.0
            };
            let dist = (dx * dx + dy * dy).sqrt();
            // Soft AA band (~1.25 px) so Dock edges aren’t jaggy.
            let alpha = if dist <= r - 0.65 {
                255u8
            } else if dist >= r + 0.65 {
                0u8
            } else {
                let t = ((r + 0.65 - dist) / 1.3).clamp(0.0, 1.0);
                // Smoothstep for cleaner AA
                let t = t * t * (3.0 - 2.0 * t);
                (t * 255.0).round() as u8
            };
            if alpha > 0 {
                img.put_pixel(x, y, image::Rgba([color[0], color[1], color[2], alpha]));
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use objc2_foundation::{NSProcessInfo, NSString};

    /// Set the process name used by the system menu bar and Dock.
    pub(crate) fn set_process_name(name: &str) {
        let info = NSProcessInfo::processInfo();
        info.setProcessName(&NSString::from_str(name));
    }

    /// After installing the main menu, force the application menu title.
    /// macOS sometimes keeps the executable name until this is reapplied.
    pub(crate) fn force_app_menu_title(name: &str) {
        use objc2_app_kit::NSApplication;
        use objc2_foundation::MainThreadMarker;

        set_process_name(name);

        let mtm = MainThreadMarker::new().expect("menu install on main thread");
        let app = NSApplication::sharedApplication(mtm);
        let Some(main) = app.mainMenu() else {
            return;
        };
        if main.numberOfItems() == 0 {
            return;
        }
        // First item is the application menu (title shown next to ).
        if let Some(item) = main.itemAtIndex(0) {
            let title = NSString::from_str(name);
            item.setTitle(&title);
            if let Some(sub) = item.submenu() {
                sub.setTitle(&title);
            }
        }
    }
}
