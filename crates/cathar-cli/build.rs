//! Embed a CLI-sized logo from `docs/logo.png` at compile time.
//!
//! Resize only — same approach as sheathe's banner embed.

use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder, imageops, load_from_memory};
use std::env;
use std::fs;
use std::path::PathBuf;

/// Embedded PNG resolution (display size is set separately in `banner.rs`).
const CLI_LOGO_PX: u32 = 256;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let logo_src = manifest_dir.join("logo.png");
    let png = fs::read(&logo_src).unwrap_or_else(|e| panic!("reading {}: {e}", logo_src.display()));

    let img = load_from_memory(&png).expect("decoding docs/logo.png");
    let rgba =
        imageops::resize(&img.to_rgba8(), CLI_LOGO_PX, CLI_LOGO_PX, imageops::FilterType::Triangle);

    let mut out_png = Vec::new();
    PngEncoder::new(&mut out_png)
        .write_image(rgba.as_raw(), CLI_LOGO_PX, CLI_LOGO_PX, ColorType::Rgba8.into())
        .expect("encoding cli logo png");

    fs::write(out_dir.join("logo.png"), &out_png).expect("writing OUT_DIR/logo.png");
    println!("cargo:rerun-if-changed={}", logo_src.display());
}
