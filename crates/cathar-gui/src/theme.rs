//! macOS-flavoured egui themes (dark + light).
//!
//! egui draws every widget itself (it is not native AppKit), so this can only
//! *resemble* Aqua — but tuning accent, rounding, fills and spacing gets close.

use egui::style::HandleShape;
use egui::{
    Color32, Context, FontId, Margin, Rounding, Stroke, Style, TextStyle, Theme, ThemePreference,
    Visuals, vec2,
};

/// User theme choice. [`Appearance::System`] follows the OS light/dark setting
/// (reported by eframe/winit each frame).
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

/// Apply Cathar's custom visuals for `appearance`.
pub(crate) fn apply(ctx: &Context, appearance: Appearance) {
    ctx.set_theme(appearance.preference());

    let accent = Color32::from_rgb(10, 132, 255);
    let r = Rounding::same(5.0);

    let visuals = match resolved(ctx, appearance) {
        Theme::Dark => dark_visuals(accent, r),
        Theme::Light => light_visuals(accent, r),
    };

    let mut text_styles = std::collections::BTreeMap::new();
    text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
    text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
    text_styles.insert(TextStyle::Heading, FontId::proportional(14.0));
    text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    text_styles.insert(TextStyle::Small, FontId::proportional(11.0));

    let style = Style {
        visuals,
        text_styles,
        spacing: egui::style::Spacing {
            item_spacing: vec2(8.0, 8.0),
            button_padding: vec2(12.0, 6.0),
            interact_size: vec2(40.0, 24.0),
            slider_width: 200.0,
            slider_rail_height: 4.0, // thin groove instead of a fat pill
            indent: 14.0,
            window_margin: Margin::same(12.0),
            menu_margin: Margin::symmetric(8.0, 2.0),
            ..Default::default()
        },
        ..Default::default()
    };
    ctx.set_style(style);
}

fn dark_visuals(accent: Color32, r: Rounding) -> Visuals {
    let text = Color32::from_rgb(235, 235, 240);
    let mut v = Visuals::dark();
    v.panel_fill = Color32::from_rgb(38, 38, 40);
    v.window_fill = Color32::from_rgb(38, 38, 40);
    v.extreme_bg_color = Color32::from_rgb(22, 22, 24);
    v.faint_bg_color = Color32::from_rgb(48, 48, 50);
    v.window_rounding = Rounding::same(8.0);
    v.menu_rounding = Rounding::same(6.0);
    v.hyperlink_color = accent;
    // selection.bg_fill also colours the slider trailing fill — keep it crisp,
    // not washed. A slim rectangular handle reads cleaner than the fat circle.
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(10, 132, 255, 190);
    v.selection.stroke = Stroke::new(1.0, accent);
    v.handle_shape = HandleShape::Rect { aspect_ratio: 0.5 };
    paint_widgets(&mut v, accent, text, r, appearance_dark());
    v
}

fn light_visuals(accent: Color32, r: Rounding) -> Visuals {
    let text = Color32::from_rgb(28, 28, 30);
    let mut v = Visuals::light();
    v.panel_fill = Color32::from_rgb(236, 236, 238);
    v.window_fill = Color32::from_rgb(236, 236, 238);
    v.extreme_bg_color = Color32::from_rgb(224, 224, 228);
    v.faint_bg_color = Color32::from_rgb(244, 244, 246);
    v.window_rounding = Rounding::same(8.0);
    v.menu_rounding = Rounding::same(6.0);
    v.hyperlink_color = accent;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(10, 132, 255, 200);
    v.selection.stroke = Stroke::new(1.0, accent);
    v.handle_shape = HandleShape::Rect { aspect_ratio: 0.5 };
    paint_widgets(&mut v, accent, text, r, appearance_light());
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

fn appearance_dark() -> WidgetColors {
    WidgetColors {
        inactive_bg: Color32::from_rgb(58, 58, 60),
        inactive_border: Stroke::NONE, // flat dark buttons
        hover_bg: Color32::from_rgb(72, 72, 74),
        hover_border: Color32::from_rgb(92, 92, 96),
        open_bg: Color32::from_rgb(72, 72, 74),
        separator: Color32::from_rgb(56, 56, 58),
    }
}

fn appearance_light() -> WidgetColors {
    WidgetColors {
        // Crisp white buttons with a hairline border stand out from the grey
        // panel (macOS light push-button look) instead of blending into it.
        inactive_bg: Color32::from_rgb(252, 252, 253),
        inactive_border: Stroke::new(1.0, Color32::from_rgb(201, 201, 208)),
        hover_bg: Color32::from_rgb(244, 244, 246),
        hover_border: Color32::from_rgb(176, 176, 184),
        open_bg: Color32::from_rgb(244, 244, 246),
        separator: Color32::from_rgb(206, 206, 214),
    }
}

fn paint_widgets(v: &mut Visuals, accent: Color32, text: Color32, r: Rounding, c: WidgetColors) {
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
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.rounding = r;

    v.widgets.open.weak_bg_fill = c.open_bg;
    v.widgets.open.bg_fill = c.open_bg;
    v.widgets.open.bg_stroke = Stroke::new(1.0, c.hover_border);
    v.widgets.open.rounding = r;

    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.separator);
}
