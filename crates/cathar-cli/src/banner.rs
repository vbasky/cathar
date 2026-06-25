//! The startup banner printed by the `cathar` CLI.

use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::{IsTerminal, Write, stderr};

const LOGO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/logo.png"));

/// Inline-image display size in terminal pixels (embedded asset is 256×256).
const LOGO_DISPLAY_PX: u32 = 120;

/// Print the logo and version to stderr. The tagline lives in clap's `about` text.
pub(crate) fn print() {
    if color_enabled() {
        render_logo();
    }
    eprintln!("  cathar {}", env!("CARGO_PKG_VERSION"));
}

fn color_enabled() -> bool {
    std::env::var("NO_COLOR").is_err() && stderr().is_terminal()
}

fn term_program_is(name: &str) -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|t| t == name)
}

fn terminal_supports_inline_image() -> bool {
    term_program_is("iTerm.app")
        || term_program_is("WezTerm")
        || term_program_is("ghostty")
        || term_program_is("WarpTerminal")
        || term_program_is("rio")
        || std::env::var("LC_TERMINAL").is_ok_and(|t| t == "iTerm2" || t == "WezTerm")
        || std::env::var("KONSOLE_VERSION").is_ok()
}

fn render_logo() -> bool {
    if !terminal_supports_inline_image() {
        return false;
    }

    let b64 = STANDARD.encode(LOGO);
    writeln!(
        stderr(),
        "  \x1b]1337;File=inline=1;preserveAspectRatio=1;width={LOGO_DISPLAY_PX}px;height={LOGO_DISPLAY_PX}px;size={}:{b64}\x07",
        LOGO.len(),
    )
    .ok()
    .map(|()| true)
    .unwrap_or(false)
}
