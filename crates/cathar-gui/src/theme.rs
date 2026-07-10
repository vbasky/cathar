//! Cathar visual identity — full dark *and* light palettes.
//!
//! All chrome/canvas colours go through [`Palette`] so light mode is not a
//! half-switch (egui widgets light + hard-coded dark canvas). Call [`apply`]
//! whenever the user appearance or OS theme changes; custom paint code reads
//! the active palette via the accessors below.
//!
//! # Design tokens
//! Prefer the `RADIUS_*` / `CTRL_*` / `FONT_*` constants so buttons, checkboxes,
//! cards, and type stay consistent across toolbar, player, modules, and tools.

use std::cell::Cell;

use egui::style::HandleShape;
use egui::{
    Color32, Context, FontId, Frame, Margin, Rounding, Stroke, Style, TextStyle, Theme,
    ThemePreference, Visuals, vec2,
};

// ─── Design tokens ──────────────────────────────────────────────────────────

/// Checkbox / chip corners (must stay << icon size so boxes stay square).
pub(crate) const RADIUS_SM: f32 = 3.0;
/// Buttons, tool tiles, compact rows, combos.
pub(crate) const RADIUS_MD: f32 = 4.0;
/// Cards, search fields, floating module windows, group headers.
pub(crate) const RADIUS_LG: f32 = 6.0;

/// Standard control height (checkbox row, combo, Flat, ghost/primary buttons).
pub(crate) const CTRL_H: f32 = 30.0;
/// Square toolbar / transport icon buttons.
pub(crate) const TOOLBAR_BTN: f32 = 32.0;
/// Channel chips in the top bar.
pub(crate) const CHIP_H: f32 = 28.0;
/// Play/pause control height.
pub(crate) const TRANSPORT_PLAY: f32 = 36.0;

pub(crate) const FONT_CAPTION: f32 = 11.0;
pub(crate) const FONT_LABEL: f32 = 12.0;
pub(crate) const FONT_BODY: f32 = 13.0;
pub(crate) const FONT_BUTTON: f32 = 13.0;
pub(crate) const FONT_SECTION: f32 = 11.0;
pub(crate) const FONT_TITLE: f32 = 15.0;
pub(crate) const FONT_MONO: f32 = 12.0;

/// Hairline stroke used on cards and inputs.
pub(crate) fn stroke_hairline() -> Stroke {
    Stroke::new(1.0, hairline())
}

/// Standard chrome card (search, levels well, inset panels).
pub(crate) fn card_frame() -> Frame {
    Frame::none()
        .fill(well_bg())
        .stroke(stroke_hairline())
        .rounding(RADIUS_LG)
        .inner_margin(Margin::symmetric(12.0, 10.0))
}

/// User theme choice. [`Appearance::System`] follows the OS light/dark setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

impl Appearance {
    pub(crate) fn preference(self) -> ThemePreference {
        match self {
            Self::System => ThemePreference::System,
            Self::Dark => ThemePreference::Dark,
            Self::Light => ThemePreference::Light,
        }
    }
}

/// Resolved dark/light after applying `appearance` (System → OS report).
pub(crate) fn resolved(ctx: &Context, appearance: Appearance) -> Theme {
    match appearance {
        Appearance::System => ctx.system_theme().unwrap_or_else(|| ctx.theme()),
        Appearance::Dark => Theme::Dark,
        Appearance::Light => Theme::Light,
    }
}

// ─── Active palette ─────────────────────────────────────────────────────────

/// Full colour set for one appearance (dark or light).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub accent: Color32,
    pub well_bg: Color32,
    pub chrome_bg: Color32,
    pub surface: Color32,
    pub window_bg: Color32,
    pub hairline: Color32,
    pub wave_l: Color32,
    pub wave_r: Color32,
    pub playhead: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub axis: Color32,
    pub selection_stroke: Color32,
    pub selection_fill: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub player_bar: Color32,
    pub on_accent: Color32,
}

impl Palette {
    pub(crate) fn dark() -> Self {
        Self {
            accent: Color32::from_rgb(0, 214, 160),
            well_bg: Color32::from_rgb(10, 10, 10),
            chrome_bg: Color32::from_rgb(24, 22, 20),
            surface: Color32::from_rgb(38, 35, 32),
            window_bg: Color32::from_rgb(30, 28, 26),
            hairline: Color32::from_rgb(62, 56, 50),
            wave_l: Color32::from_rgb(0, 220, 168),
            wave_r: Color32::from_rgb(255, 148, 48),
            playhead: Color32::from_rgb(255, 72, 64),
            text: Color32::from_rgb(242, 236, 228),
            text_muted: Color32::from_rgb(150, 142, 130),
            axis: Color32::from_rgb(170, 160, 145),
            selection_stroke: Color32::from_rgb(0, 230, 170),
            selection_fill: Color32::from_rgba_unmultiplied(0, 214, 160, 55),
            ok: Color32::from_rgb(48, 210, 120),
            warn: Color32::from_rgb(255, 170, 40),
            player_bar: Color32::from_rgb(22, 20, 18),
            on_accent: Color32::from_rgb(12, 20, 18),
        }
    }

    /// Real light mode — warm paper chrome, light spectrogram well, same brand accent.
    pub(crate) fn light() -> Self {
        Self {
            accent: Color32::from_rgb(0, 150, 115),
            well_bg: Color32::from_rgb(236, 232, 226),
            chrome_bg: Color32::from_rgb(248, 245, 240),
            surface: Color32::from_rgb(255, 253, 250),
            window_bg: Color32::from_rgb(255, 252, 248),
            hairline: Color32::from_rgb(200, 192, 180),
            // Slightly deeper traces so they read on pale paper.
            wave_l: Color32::from_rgb(0, 150, 120),
            wave_r: Color32::from_rgb(200, 110, 20),
            playhead: Color32::from_rgb(210, 45, 40),
            text: Color32::from_rgb(32, 28, 24),
            text_muted: Color32::from_rgb(110, 102, 92),
            axis: Color32::from_rgb(100, 94, 86),
            selection_stroke: Color32::from_rgb(0, 140, 110),
            selection_fill: Color32::from_rgba_unmultiplied(0, 150, 115, 40),
            ok: Color32::from_rgb(20, 150, 90),
            warn: Color32::from_rgb(200, 120, 10),
            player_bar: Color32::from_rgb(242, 238, 232),
            on_accent: Color32::from_rgb(255, 255, 255),
        }
    }

    pub(crate) fn for_theme(t: Theme) -> Self {
        match t {
            Theme::Dark => Self::dark(),
            Theme::Light => Self::light(),
        }
    }
}

thread_local! {
    // Not `const` — palette includes non-const rgba construction.
    static ACTIVE: Cell<Option<Palette>> = const { Cell::new(None) };
}

fn set_active(p: Palette) {
    ACTIVE.with(|c| c.set(Some(p)));
}

/// Snapshot of the palette last applied by [`apply`] (defaults to dark).
pub(crate) fn current() -> Palette {
    ACTIVE.with(|c| c.get().unwrap_or_else(Palette::dark))
}

// Accessors — use these from paint/UI code (not hard-coded dark consts).
#[inline]
pub(crate) fn accent() -> Color32 {
    current().accent
}
#[inline]
pub(crate) fn well_bg() -> Color32 {
    current().well_bg
}
#[inline]
pub(crate) fn chrome_bg() -> Color32 {
    current().chrome_bg
}
#[inline]
pub(crate) fn surface() -> Color32 {
    current().surface
}
#[inline]
pub(crate) fn window_bg() -> Color32 {
    current().window_bg
}
#[inline]
pub(crate) fn hairline() -> Color32 {
    current().hairline
}
#[inline]
pub(crate) fn wave_l() -> Color32 {
    current().wave_l
}
#[inline]
pub(crate) fn wave_r() -> Color32 {
    current().wave_r
}
#[inline]
pub(crate) fn wave_tint() -> Color32 {
    current().wave_l
}
#[inline]
pub(crate) fn playhead() -> Color32 {
    current().playhead
}
#[inline]
pub(crate) fn text() -> Color32 {
    current().text
}
#[inline]
pub(crate) fn text_muted() -> Color32 {
    current().text_muted
}
#[inline]
pub(crate) fn axis() -> Color32 {
    current().axis
}
#[inline]
pub(crate) fn selection_stroke() -> Color32 {
    current().selection_stroke
}
#[inline]
pub(crate) fn selection_fill() -> Color32 {
    current().selection_fill
}
#[inline]
pub(crate) fn ok() -> Color32 {
    current().ok
}
#[inline]
pub(crate) fn warn() -> Color32 {
    current().warn
}
#[inline]
pub(crate) fn player_bar() -> Color32 {
    current().player_bar
}
#[inline]
pub(crate) fn on_accent() -> Color32 {
    current().on_accent
}

// ─── Apply to egui ──────────────────────────────────────────────────────────

/// Apply Cathar visuals + active palette for `appearance`.
pub(crate) fn apply(ctx: &Context, appearance: Appearance) {
    ctx.set_theme(appearance.preference());

    let theme = resolved(ctx, appearance);
    let p = Palette::for_theme(theme);
    set_active(p);

    let r = Rounding::same(RADIUS_MD);
    let visuals = match theme {
        Theme::Dark => dark_visuals(p, r),
        Theme::Light => light_visuals(p, r),
    };

    let mut text_styles = std::collections::BTreeMap::new();
    text_styles.insert(TextStyle::Body, FontId::proportional(FONT_BODY));
    text_styles.insert(TextStyle::Button, FontId::proportional(FONT_BUTTON));
    text_styles.insert(TextStyle::Heading, FontId::proportional(FONT_TITLE));
    text_styles.insert(TextStyle::Monospace, FontId::monospace(FONT_MONO));
    text_styles.insert(TextStyle::Small, FontId::proportional(FONT_CAPTION));

    let style = Style {
        visuals,
        text_styles,
        spacing: egui::style::Spacing {
            item_spacing: vec2(8.0, 8.0),
            button_padding: vec2(12.0, 6.0),
            interact_size: vec2(40.0, CTRL_H),
            slider_width: 160.0,
            slider_rail_height: 4.0,
            indent: 14.0,
            window_margin: Margin::same(12.0),
            menu_margin: Margin::symmetric(10.0, 6.0),
            // Square-ish checkboxes: size >> radius so they don't read as circles.
            icon_width: 16.0,
            icon_width_inner: 10.0,
            icon_spacing: 8.0,
            ..Default::default()
        },
        ..Default::default()
    };
    ctx.set_style(style);
}

fn dark_visuals(p: Palette, r: Rounding) -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = p.chrome_bg;
    v.window_fill = p.window_bg;
    v.extreme_bg_color = p.well_bg;
    v.faint_bg_color = p.surface;
    v.window_rounding = Rounding::same(RADIUS_LG);
    v.menu_rounding = Rounding::same(RADIUS_MD);
    v.hyperlink_color = p.accent;
    // Soft selection so default light text stays readable in combo menus
    // (solid accent made labels vanish / look corrupt).
    v.selection.bg_fill =
        Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 56);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.handle_shape = HandleShape::Rect { aspect_ratio: 0.45 };
    paint_widgets(
        &mut v,
        p.accent,
        p.text,
        p.on_accent,
        r,
        WidgetColors {
            inactive_bg: p.surface,
            inactive_border: Stroke::new(1.0, p.hairline),
            hover_bg: Color32::from_rgb(52, 48, 44),
            hover_border: Color32::from_rgb(0, 180, 140),
            open_bg: Color32::from_rgb(52, 48, 44),
            separator: p.hairline,
        },
    );
    v
}

fn light_visuals(p: Palette, r: Rounding) -> Visuals {
    let mut v = Visuals::light();
    v.panel_fill = p.chrome_bg;
    v.window_fill = p.window_bg;
    v.extreme_bg_color = p.well_bg;
    v.faint_bg_color = p.surface;
    v.window_rounding = Rounding::same(RADIUS_LG);
    v.menu_rounding = Rounding::same(RADIUS_MD);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill =
        Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 48);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.handle_shape = HandleShape::Rect { aspect_ratio: 0.45 };
    // Override light default greys with warm paper.
    v.override_text_color = Some(p.text);
    paint_widgets(
        &mut v,
        p.accent,
        p.text,
        p.on_accent,
        r,
        WidgetColors {
            inactive_bg: p.surface,
            inactive_border: Stroke::new(1.0, p.hairline),
            hover_bg: Color32::from_rgb(232, 246, 240),
            hover_border: Color32::from_rgb(0, 150, 115),
            open_bg: Color32::from_rgb(236, 248, 242),
            separator: p.hairline,
        },
    );
    v
}

struct WidgetColors {
    inactive_bg: Color32,
    inactive_border: Stroke,
    hover_bg: Color32,
    hover_border: Color32,
    open_bg: Color32,
    separator: Color32,
}

fn paint_widgets(
    v: &mut Visuals,
    accent: Color32,
    text: Color32,
    on_accent: Color32,
    r: Rounding,
    c: WidgetColors,
) {
    v.widgets.inactive.weak_bg_fill = c.inactive_bg;
    v.widgets.inactive.bg_fill = c.inactive_bg;
    v.widgets.inactive.bg_stroke = c.inactive_border;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.weak_bg_fill = c.hover_bg;
    v.widgets.hovered.bg_fill = c.hover_bg;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, c.hover_border);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, text);
    v.widgets.hovered.rounding = r;

    v.widgets.active.weak_bg_fill = accent;
    v.widgets.active.bg_fill = accent;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.0, on_accent);
    v.widgets.active.rounding = r;

    v.widgets.open.weak_bg_fill = c.open_bg;
    v.widgets.open.bg_fill = c.open_bg;
    v.widgets.open.bg_stroke = Stroke::new(1.0, c.hover_border);
    v.widgets.open.rounding = r;

    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.separator);
}
