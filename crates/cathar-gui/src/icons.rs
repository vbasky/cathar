//! Phosphor icon helpers (font installed in `fonts.rs`).

use std::sync::Arc;

use egui::{Button, Color32, FontFamily, RichText, Vec2, Widget, WidgetText};

use crate::theme::{
    CHIP_H, FONT_LABEL, RADIUS_LG, RADIUS_MD, TOOLBAR_BTN, TRANSPORT_PLAY, on_accent,
};

pub(crate) use egui_phosphor::regular::{
    ARROW_COUNTER_CLOCKWISE, ARROWS_CLOCKWISE, ARROWS_OUT_LINE_HORIZONTAL, BROOM, CHART_BAR,
    CLOCK_CLOCKWISE, COMPASS, DISC, DROP, EAR, EQUALIZER, FADERS, FAST_FORWARD, GAUGE, HEADPHONES,
    LIGHTNING, MAGNIFYING_GLASS, MICROPHONE, MICROPHONE_STAGE, MONITOR, MUSIC_NOTE, MUSIC_NOTES,
    PAUSE, PLAY, PLAYLIST, REWIND, SKIP_BACK, SKIP_FORWARD, SPARKLE, SPEAKER_HIGH, SPEAKER_SLASH,
    STOP, SWAP, WAVEFORM, WIND, WRENCH,
};

pub(crate) const TOOLBAR_ICON: f32 = 18.0;
pub(crate) const TRANSPORT_ICON: f32 = 22.0;
pub(crate) const TOOL_ICON: f32 = 18.0;

/// Explicit family — icons must not go through the system UI font first.
pub(crate) fn family() -> FontFamily {
    FontFamily::Name(Arc::from("phosphor"))
}

pub(crate) fn rich(icon: &str, size: f32) -> RichText {
    RichText::new(icon).family(family()).size(size)
}

pub(crate) fn widget(icon: &str, size: f32) -> WidgetText {
    rich(icon, size).into()
}

/// Square toolbar control — icon only; attach `.on_hover_text()` when adding.
pub(crate) fn toolbar_button(icon: &'static str) -> Button<'static> {
    Button::new(widget(icon, TOOLBAR_ICON)).min_size(Vec2::splat(TOOLBAR_BTN)).rounding(RADIUS_MD)
}

/// Toggle toolbar control for A/B compare and viewer modes.
/// Same hit target as [`toolbar_button`] so rows line up.
pub(crate) fn toolbar_toggle(selected: bool, icon: &'static str) -> impl Widget + 'static {
    let stroke = if selected {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, crate::theme::hairline())
    };
    let icon_color = if selected { on_accent() } else { Color32::PLACEHOLDER };
    Button::new(rich(icon, TOOLBAR_ICON).color(icon_color))
        .min_size(Vec2::splat(TOOLBAR_BTN))
        .rounding(RADIUS_MD)
        .selected(selected)
        .fill(if selected { crate::theme::accent() } else { crate::theme::surface() })
        .stroke(stroke)
}

/// Text chip for L / R / L+R / L|R channel selection.
pub(crate) fn channel_chip(selected: bool, label: &str) -> Button<'static> {
    let color = if selected { on_accent() } else { Color32::PLACEHOLDER };
    let text = RichText::new(label.to_string()).size(FONT_LABEL).strong().color(color);
    let mut b = Button::new(text).min_size(Vec2::new(42.0, CHIP_H)).rounding(RADIUS_MD).stroke(
        if selected {
            egui::Stroke::NONE
        } else {
            egui::Stroke::new(1.0, crate::theme::hairline())
        },
    );
    if selected {
        b = b.fill(crate::theme::accent());
    } else {
        b = b.fill(crate::theme::surface());
    }
    b
}

/// Primary play/pause control on the transport strip.
pub(crate) fn transport_play_button(playing: bool, icon: &'static str) -> Button<'static> {
    let icon_color = if playing { on_accent() } else { Color32::PLACEHOLDER };
    Button::new(rich(icon, TRANSPORT_ICON).color(icon_color))
        .min_size(Vec2::new(TRANSPORT_PLAY + 6.0, TRANSPORT_PLAY))
        .rounding(RADIUS_LG)
        .fill(if playing { crate::theme::accent() } else { crate::theme::surface() })
        .stroke(if playing {
            egui::Stroke::NONE
        } else {
            egui::Stroke::new(1.0, crate::theme::hairline())
        })
}
