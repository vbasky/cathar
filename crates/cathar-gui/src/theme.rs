//! A macOS-dark-flavoured egui theme.
//!
//! egui draws every widget itself (it is not native AppKit), so this can only
//! *resemble* Aqua — but tuning the accent, rounding, fills and spacing to
//! macOS dark mode gets it close, and stays cohesive with the dark spectrogram.

use egui::{Color32, Context, Margin, Rounding, Stroke, ThemePreference, Visuals, vec2};

/// Apply the theme to `ctx`. Pins to dark so an OS light/dark switch mid-session
/// won't override the styling.
pub(crate) fn apply(ctx: &Context) {
    ctx.set_theme(ThemePreference::Dark);

    let accent = Color32::from_rgb(10, 132, 255); // macOS dark-mode system blue
    let text = Color32::from_rgb(235, 235, 240);
    let r = Rounding::same(6.0);

    let mut v = Visuals::dark();
    v.panel_fill = Color32::from_rgb(30, 30, 32);
    v.window_fill = Color32::from_rgb(30, 30, 32);
    v.extreme_bg_color = Color32::from_rgb(18, 18, 20); // slider troughs / fields
    v.faint_bg_color = Color32::from_rgb(40, 40, 43);
    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(8.0);
    v.hyperlink_color = accent;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(10, 132, 255, 110);
    v.selection.stroke = Stroke::new(1.0, accent);

    // Resting controls — flat, borderless, rounded (macOS push-button feel).
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(58, 58, 60);
    v.widgets.inactive.bg_fill = Color32::from_rgb(58, 58, 60);
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.inactive.rounding = r;

    // Hover — slightly lighter with a hairline border.
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(72, 72, 74);
    v.widgets.hovered.bg_fill = Color32::from_rgb(72, 72, 74);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(92, 92, 96));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.hovered.rounding = r;

    // Pressed / active / on → accent blue.
    v.widgets.active.weak_bg_fill = accent;
    v.widgets.active.bg_fill = accent;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.rounding = r;

    // Open combos / expanded collapsers.
    v.widgets.open.weak_bg_fill = Color32::from_rgb(72, 72, 74);
    v.widgets.open.bg_fill = Color32::from_rgb(72, 72, 74);
    v.widgets.open.bg_stroke = Stroke::new(1.0, Color32::from_rgb(92, 92, 96));
    v.widgets.open.rounding = r;

    // Labels, separators.
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(56, 56, 58));

    let mut style = (*ctx.style()).clone();
    style.visuals = v;
    style.spacing.item_spacing = vec2(8.0, 7.0);
    style.spacing.button_padding = vec2(10.0, 5.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.slider_width = 130.0;
    style.spacing.window_margin = Margin::same(10.0);
    ctx.set_style(style);
}
