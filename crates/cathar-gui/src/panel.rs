//! Modules catalogue + floating tool chrome.
//!
//! Control chrome (buttons, checkboxes, sections) goes through the shared
//! helpers here so module panels match the rest of the app.

use std::ops::RangeInclusive;

use egui::{
    Align, Button, Color32, FontId, Layout, Rect, Response, RichText, Sense, Slider, Stroke, Ui,
    Vec2, pos2,
};

use crate::icons;
use crate::theme::{
    self, CTRL_H, FONT_BODY, FONT_BUTTON, FONT_CAPTION, FONT_LABEL, FONT_SECTION, RADIUS_LG,
    RADIUS_MD, RADIUS_SM,
};

/// Right modules column width.
pub(crate) const MODULES_W: f32 = 268.0;
/// Floating module window content width.
pub(crate) const MODULE_WIN_W: f32 = 360.0;
/// Clearance so value labels / sliders don’t sit under the scrollbar.
pub(crate) const SCROLL_GUTTER: f32 = 22.0;
pub(crate) const SLIDER_W: f32 = 280.0;

/// Tune spacing inside a floating module window.
pub(crate) fn prepare_module(ui: &mut Ui) {
    ui.set_min_width(MODULE_WIN_W - 32.0);
    ui.set_max_width(MODULE_WIN_W + 48.0);
    ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
    ui.spacing_mut().icon_spacing = 8.0;
    ui.spacing_mut().icon_width = 16.0;
    ui.spacing_mut().icon_width_inner = 10.0;
    ui.spacing_mut().interact_size.y = CTRL_H;
    ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
    let track = (ui.available_width() - SCROLL_GUTTER).clamp(160.0, SLIDER_W);
    ui.spacing_mut().slider_width = track;
}

/// Checkbox row — square box + caption (same control used by Equalizer “On”).
pub(crate) fn check_row(ui: &mut Ui, value: &mut bool, text: &str) -> Response {
    square_checkbox(ui, value, text)
}

/// Section header inside a module window (or side-panel card).
pub(crate) fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).size(FONT_SECTION).strong().color(theme::text_muted()));
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(6.0);
}

/// Side-panel subsection (Levels / History) — no full separator, quieter.
pub(crate) fn side_section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(
        RichText::new(title).size(FONT_LABEL).strong().color(theme::text().gamma_multiply(0.88)),
    );
    ui.add_space(4.0);
}

/// Catalogue group: coloured rail + icon + title, collapsible body.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tool_group(
    ui: &mut Ui,
    id: &str,
    icon: &str,
    title: &str,
    subtitle: &str,
    accent: Color32,
    default_open: bool,
    // When true, keep the group expanded (e.g. active tool lives here).
    force_open: bool,
    add_contents: impl FnOnce(&mut Ui),
) {
    let id = ui.make_persistent_id(id);
    let mut open = ui.ctx().data_mut(|d| d.get_temp::<bool>(id).unwrap_or(default_open));
    if force_open {
        open = true;
    }

    // Header row
    let full_w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(full_w, 36.0), Sense::click());
    let hover = resp.hovered();
    let bg = if hover {
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 28)
    } else {
        theme::surface()
    };
    ui.painter().rect_filled(rect, RADIUS_LG, bg);
    // Left accent rail
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, Vec2::new(3.5, rect.height())),
        0.0,
        accent,
    );
    // Chevron
    let chev = if open { "▾" } else { "▸" };
    ui.painter().text(
        rect.left_center() + Vec2::new(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        chev,
        FontId::proportional(FONT_LABEL),
        theme::text_muted(),
    );
    // Icon
    ui.painter().text(
        rect.left_center() + Vec2::new(28.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon,
        FontId::new(16.0, icons::family()),
        accent,
    );
    // Title + subtitle
    ui.painter().text(
        rect.left_center() + Vec2::new(50.0, -6.0),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(FONT_BODY),
        theme::text(),
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(50.0, 8.0),
        egui::Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(FONT_CAPTION),
        theme::text_muted(),
    );

    if resp.clicked() {
        open = !open;
        ui.ctx().data_mut(|d| d.insert_temp(id, open));
    } else {
        ui.ctx().data_mut(|d| d.insert_temp(id, open));
    }

    if open {
        ui.add_space(4.0);
        ui.indent(id, |ui| {
            ui.set_max_width(full_w - 8.0);
            add_contents(ui);
        });
        ui.add_space(8.0);
    } else {
        ui.add_space(6.0);
    }
}

/// Tool tile with leading icon (RX-style module list). Returns true when clicked.
pub(crate) fn tool_tile(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    blurb: &str,
    selected: bool,
    enabled: bool,
) -> bool {
    let h = 46.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());

    let (bg, border, title_c, blurb_c, icon_c) = if !enabled {
        (
            Color32::TRANSPARENT,
            Stroke::NONE,
            theme::text_muted().gamma_multiply(0.65),
            theme::text_muted().gamma_multiply(0.5),
            theme::text_muted().gamma_multiply(0.45),
        )
    } else if selected {
        (
            theme::selection_fill(),
            Stroke::new(1.2, theme::accent()),
            theme::text(),
            theme::accent(),
            theme::accent(),
        )
    } else if resp.hovered() {
        (
            theme::surface(),
            Stroke::new(1.0, theme::hairline()),
            theme::text(),
            theme::text_muted(),
            theme::text(),
        )
    } else {
        let s = theme::surface();
        let hln = theme::hairline();
        (
            Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 90),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(hln.r(), hln.g(), hln.b(), 140)),
            theme::text().gamma_multiply(0.92),
            theme::text_muted(),
            theme::text_muted(),
        )
    };

    ui.painter().rect(rect, RADIUS_MD, bg, border);
    // Icon well (RX list glyph column)
    let icon_well =
        egui::Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(28.0));
    ui.painter().rect_filled(
        icon_well,
        RADIUS_MD,
        if selected {
            Color32::from_rgba_unmultiplied(
                theme::accent().r(),
                theme::accent().g(),
                theme::accent().b(),
                40,
            )
        } else {
            theme::well_bg()
        },
    );
    ui.painter().text(
        icon_well.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        FontId::new(icons::TOOL_ICON, icons::family()),
        icon_c,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(42.0, 9.0),
        egui::Align2::LEFT_TOP,
        title,
        FontId::proportional(FONT_LABEL),
        title_c,
    );
    ui.painter().text(
        rect.left_top() + Vec2::new(42.0, 26.0),
        egui::Align2::LEFT_TOP,
        blurb,
        FontId::proportional(FONT_CAPTION),
        blurb_c,
    );

    if selected {
        ui.painter().text(
            rect.right_center() - Vec2::new(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            "●",
            FontId::proportional(FONT_CAPTION),
            theme::accent(),
        );
    }

    enabled && resp.clicked()
}

/// Compact history / display row.
pub(crate) fn compact_row(ui: &mut Ui, label: &str, selected: bool, enabled: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let bg = if selected {
        theme::selection_fill()
    } else if resp.hovered() && enabled {
        theme::surface()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, RADIUS_MD, bg);
    if selected {
        ui.painter().rect_stroke(
            rect,
            RADIUS_MD,
            Stroke::new(1.0, theme::accent().gamma_multiply(0.55)),
        );
    }
    ui.painter().text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(FONT_LABEL),
        if enabled {
            if selected { theme::text() } else { theme::text().gamma_multiply(0.88) }
        } else {
            theme::text_muted().gamma_multiply(0.7)
        },
    );
    enabled && resp.clicked()
}

fn param_label_row(ui: &mut Ui, label: &str, value: &str) {
    // Reserve gutter so monospace values never crowd the scrollbar.
    let content_w = (ui.available_width() - SCROLL_GUTTER).max(120.0);
    ui.allocate_ui_with_layout(
        Vec2::new(content_w, 18.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(RichText::new(label).size(FONT_LABEL));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(value).monospace().size(FONT_LABEL).color(theme::text_muted()),
                );
            });
        },
    );
    ui.add_space(3.0);
}

pub(crate) fn param_f32(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    decimals: usize,
) -> Response {
    let display = format!("{:.*}", decimals, *value);
    ui.vertical(|ui| {
        let content_w = (ui.available_width() - SCROLL_GUTTER).max(120.0);
        ui.set_max_width(content_w);
        ui.spacing_mut().slider_width = (content_w - 4.0).clamp(120.0, SLIDER_W);
        param_label_row(ui, label, &display);
        ui.add(Slider::new(value, range).show_value(false).trailing_fill(true))
    })
    .inner
}

pub(crate) fn param_i32(
    ui: &mut Ui,
    label: &str,
    value: &mut i32,
    range: RangeInclusive<i32>,
) -> Response {
    let display = value.to_string();
    ui.vertical(|ui| {
        let content_w = (ui.available_width() - SCROLL_GUTTER).max(120.0);
        ui.set_max_width(content_w);
        ui.spacing_mut().slider_width = (content_w - 4.0).clamp(120.0, SLIDER_W);
        param_label_row(ui, label, &display);
        ui.add(Slider::new(value, range).integer().show_value(false).trailing_fill(true))
    })
    .inner
}

pub(crate) fn param_usize(
    ui: &mut Ui,
    label: &str,
    value: &mut usize,
    range: RangeInclusive<usize>,
) -> Response {
    let mut v = *value as i32;
    let lo = *range.start() as i32;
    let hi = *range.end() as i32;
    let resp = param_i32(ui, label, &mut v, lo..=hi);
    *value = v as usize;
    resp
}

pub(crate) fn param_u32(
    ui: &mut Ui,
    label: &str,
    value: &mut u32,
    range: RangeInclusive<u32>,
) -> Response {
    let mut v = *value as i32;
    let lo = *range.start() as i32;
    let hi = *range.end() as i32;
    let resp = param_i32(ui, label, &mut v, lo..=hi);
    *value = v as u32;
    resp
}

pub(crate) fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(FONT_CAPTION).color(theme::text_muted()));
    ui.add_space(2.0);
}

fn primary_button(label: &str) -> Button<'static> {
    Button::new(RichText::new(label.to_string()).size(FONT_BUTTON).color(theme::on_accent()))
        .fill(theme::accent())
        .rounding(RADIUS_MD)
        .min_size(Vec2::new(76.0, CTRL_H))
}

fn ghost_button(label: &str) -> Button<'static> {
    Button::new(RichText::new(label.to_string()).size(FONT_BUTTON).color(theme::text()))
        .fill(theme::surface())
        .stroke(Stroke::new(1.0, theme::hairline()))
        .rounding(RADIUS_MD)
        .min_size(Vec2::new(72.0, CTRL_H))
}

pub(crate) fn render_button(ui: &mut Ui, label: &str) -> Response {
    ui.add_space(4.0);
    ui.add(primary_button(label))
}

pub(crate) fn render_button_enabled(ui: &mut Ui, enabled: bool, label: &str) -> Response {
    ui.add_space(4.0);
    ui.add_enabled(enabled, primary_button(label))
}

pub(crate) fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(ghost_button(label))
}

/// Classic **square** checkbox (not a pill/circle).
pub(crate) fn square_checkbox(ui: &mut Ui, checked: &mut bool, text: &str) -> Response {
    const BOX: f32 = 16.0;
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), FontId::proportional(FONT_BODY), theme::text())
    });
    let gap = 8.0;
    let desired = Vec2::new(BOX + gap + galley.size().x + 4.0, CTRL_H);

    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }

    let box_min = pos2(rect.left() + 1.0, rect.center().y - BOX * 0.5);
    let box_rect = Rect::from_min_size(box_min, Vec2::splat(BOX));

    let (fill, stroke, check_c) = if *checked {
        (theme::accent(), Stroke::new(1.0, theme::accent()), theme::on_accent())
    } else if resp.hovered() {
        (theme::surface(), Stroke::new(1.25, theme::accent().gamma_multiply(0.75)), theme::text())
    } else {
        (theme::surface(), Stroke::new(1.25, theme::hairline()), theme::text())
    };

    let painter = ui.painter();
    painter.rect_filled(box_rect, RADIUS_SM, fill);
    painter.rect_stroke(box_rect, RADIUS_SM, stroke);

    if *checked {
        let c = box_rect.center();
        let a = pos2(c.x - 4.0, c.y + 0.5);
        let b = pos2(c.x - 1.0, c.y + 3.5);
        let d = pos2(c.x + 4.5, c.y - 3.5);
        painter.line_segment([a, b], Stroke::new(1.8, check_c));
        painter.line_segment([b, d], Stroke::new(1.8, check_c));
    }

    let text_pos = pos2(box_rect.right() + gap, rect.center().y - galley.size().y * 0.5);
    painter.galley(text_pos, galley, theme::text());

    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn action_footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.spacing_mut().interact_size.y = CTRL_H;
        add(ui);
    });
}

/// RX-style action row: Preview · Bypass · Compare · Render.
pub(crate) fn action_row(ui: &mut Ui) -> (bool, bool, bool, bool) {
    let mut preview = false;
    let mut bypass = false;
    let mut compare = false;
    let mut render = false;
    action_footer(ui, |ui| {
        preview =
            ui.add(ghost_button("Preview")).on_hover_text("Audition without committing").clicked();
        bypass = ui
            .add(ghost_button("Bypass"))
            .on_hover_text("Clear preview / hear current edit")
            .clicked();
        compare = ui
            .add(ghost_button("Compare"))
            .on_hover_text("A/B with the pristine open file")
            .clicked();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            render = ui.add(primary_button("Render")).on_hover_text("Commit to history").clicked();
        });
    });
    (preview, bypass, compare, render)
}

/// Footer for live-only modules (e.g. graphic EQ): Compare · Render.
pub(crate) fn compare_render_row(ui: &mut Ui) -> (bool, bool) {
    let mut compare = false;
    let mut render = false;
    action_footer(ui, |ui| {
        compare = ui
            .add(ghost_button("Compare"))
            .on_hover_text("A/B with the pristine open file")
            .clicked();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            render = ui
                .add(primary_button("Render"))
                .on_hover_text("Bake the current curve into edit history")
                .clicked();
        });
    });
    (compare, render)
}

// ─── Mixer-style controls (vertical faders + dials + VU) ────────────────────

/// Map linear amplitude 0…1 → dBFS (floor −60).
pub(crate) fn linear_to_db(peak: f32) -> f32 {
    if peak <= 1e-10 { -60.0 } else { (20.0 * peak.log10()).clamp(-60.0, 6.0) }
}

/// Green → yellow → orange → red like classic pro meters.
pub(crate) fn vu_color(db: f32) -> Color32 {
    // −60…−18 green, −18…−6 yellow/orange, −6…0 red
    if db < -18.0 {
        let t = ((db + 60.0) / 42.0).clamp(0.0, 1.0);
        lerp_rgb((20, 160, 70), (80, 210, 90), t)
    } else if db < -6.0 {
        let t = ((db + 18.0) / 12.0).clamp(0.0, 1.0);
        lerp_rgb((80, 210, 90), (255, 170, 30), t)
    } else {
        let t = ((db + 6.0) / 6.0).clamp(0.0, 1.0);
        lerp_rgb((255, 170, 30), (255, 50, 40), t)
    }
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t).round() as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t).round() as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t).round() as u8,
    )
}

/// Horizontal VU strip with dB readout and green→red fill (player L/R).
///
/// Total row width is **fixed** so changing dB text (`-7` vs `-60`) never reflows
/// the player bar (which made volume appear to jump).
pub(crate) fn vu_meter_h(ui: &mut Ui, label: &str, peak_linear: f32, width: f32) {
    let db = linear_to_db(peak_linear.clamp(0.0, 1.5));
    // Map −60…0 dB → 0…1 for bar length (clip above 0 lights full + red).
    let t = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    // Fixed glyph slots: "L"/"R" + bar + "−60".."  0" style width.
    const DB_W: f32 = 28.0;
    const LAB_W: f32 = 12.0;
    let row_w = LAB_W + 5.0 + width + 5.0 + DB_W;
    ui.allocate_ui_with_layout(
        Vec2::new(row_w, 14.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.add_sized(
                [LAB_W, 14.0],
                egui::Label::new(
                    RichText::new(label)
                        .size(theme::FONT_CAPTION)
                        .strong()
                        .color(theme::text_muted()),
                ),
            );
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 12.0), Sense::hover());
            ui.painter().rect_filled(rect, theme::RADIUS_SM, theme::well_bg());
            ui.painter().rect_stroke(rect, theme::RADIUS_SM, Stroke::new(1.0, theme::hairline()));
            let fill_w = (t * rect.width()).max(if peak_linear > 0.0 { 1.5 } else { 0.0 });
            if fill_w > 0.0 {
                let slices = 24usize;
                let sw = fill_w / slices as f32;
                for i in 0..slices {
                    let x0 = rect.left() + i as f32 * sw;
                    if x0 >= rect.left() + fill_w {
                        break;
                    }
                    let local_t = (i as f32 + 0.5) / slices as f32 * t;
                    let local_db = -60.0 + local_t * 60.0;
                    let w = (rect.left() + fill_w - x0).min(sw).max(0.5);
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            pos2(x0, rect.top()),
                            Vec2::new(w, rect.height()),
                        ),
                        0.0,
                        vu_color(local_db),
                    );
                }
            }
            // Fixed-width dB so layout never shifts with value.
            ui.add_sized(
                [DB_W, 14.0],
                egui::Label::new(
                    RichText::new(format!("{db:>3.0}"))
                        .monospace()
                        .size(theme::FONT_CAPTION)
                        .color(theme::text_muted()),
                ),
            );
            resp.on_hover_text(format!("{label}: {db:.1} dBFS"));
        },
    );
}

/// How a fader/dial value is shown on the control.
#[derive(Clone, Copy)]
pub(crate) enum ValueFmt {
    /// Fixed decimals (`2.50`).
    Fixed(usize),
    /// Display as percent: `value * 100` then `decimals` places + `%`.
    Percent(usize),
}

impl ValueFmt {
    fn format(self, v: f32) -> String {
        match self {
            Self::Fixed(d) => format!("{v:.d$}", d = d),
            Self::Percent(d) => format!("{:.d$}%", v * 100.0, d = d),
        }
    }
}

/// Vertical mixer fader (bottom = min, top = max).
///
/// Layout (top → bottom): value pill · track + grip · label.
pub(crate) fn vertical_fader(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    fmt: ValueFmt,
    accent: Color32,
) -> Response {
    let lo = *range.start();
    let hi = *range.end();
    let span = (hi - lo).max(1e-6);
    let mut t = ((*value - lo) / span).clamp(0.0, 1.0);

    let size = Vec2::new(64.0, 168.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let id = ui.id().with("vfader").with(id);
    let resp = ui.interact(rect, id, Sense::click_and_drag());

    // Soft channel well behind the whole control
    ui.painter().rect_filled(rect.shrink(1.0), RADIUS_LG, theme::well_bg().gamma_multiply(0.55));
    ui.painter().rect_stroke(rect.shrink(1.0), RADIUS_LG, theme::stroke_hairline());

    // Value pill
    let val_s = fmt.format(*value);
    let pill = egui::Rect::from_center_size(
        pos2(rect.center().x, rect.top() + 14.0),
        Vec2::new(52.0, 18.0),
    );
    ui.painter().rect_filled(pill, RADIUS_MD, theme::surface());
    ui.painter().rect_stroke(pill, RADIUS_MD, theme::stroke_hairline());
    ui.painter().text(
        pill.center(),
        egui::Align2::CENTER_CENTER,
        val_s,
        FontId::monospace(FONT_CAPTION),
        theme::text(),
    );

    // Track
    let track_w = 5.0;
    let track_top = rect.top() + 32.0;
    let track_bot = rect.bottom() - 24.0;
    let track = egui::Rect::from_min_max(
        pos2(rect.center().x - track_w * 0.5, track_top),
        pos2(rect.center().x + track_w * 0.5, track_bot),
    );
    // Tick marks on the left of the track
    for u in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let y = track.bottom() - track.height() * u;
        ui.painter().line_segment(
            [pos2(track.left() - 6.0, y), pos2(track.left() - 2.0, y)],
            Stroke::new(1.0, theme::hairline()),
        );
    }
    ui.painter().rect_filled(track, 2.5, theme::surface());
    ui.painter().rect_stroke(track, 2.5, Stroke::new(1.0, theme::hairline()));

    if (resp.dragged() || resp.clicked()) && track.height() > 0.0 {
        if let Some(p) = ui.ctx().pointer_interact_pos().or(resp.interact_pointer_pos()) {
            t = ((track.bottom() - p.y) / track.height()).clamp(0.0, 1.0);
            *value = lo + t * span;
        }
    }

    let fill_h = track.height() * t;
    if fill_h > 0.5 {
        // Gradient-ish fill: accent at top of fill, dimmer near bottom
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                pos2(track.left(), track.bottom() - fill_h),
                track.right_bottom(),
            ),
            2.5,
            accent,
        );
    }
    // Cap with grip
    let cap_y = track.bottom() - fill_h;
    let cap = egui::Rect::from_center_size(pos2(track.center().x, cap_y), Vec2::new(22.0, 14.0));
    ui.painter().rect_filled(cap, 3.0, theme::surface());
    ui.painter().rect_stroke(cap, 3.0, Stroke::new(1.5, accent));
    for dy in [-3.0_f32, 0.0, 3.0] {
        ui.painter().line_segment(
            [
                pos2(cap.left() + 5.0, cap.center().y + dy),
                pos2(cap.right() - 5.0, cap.center().y + dy),
            ],
            Stroke::new(1.0, theme::hairline()),
        );
    }

    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 6.0),
        egui::Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(11.0),
        theme::text_muted(),
    );
    resp.on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text(format!("{label}: {}", fmt.format(*value)))
}

/// Rotary dial (Gain / Reduction style). Drag vertically to change.
/// Kept for modules that prefer rotaries over faders (e.g. de-bleed).
#[allow(dead_code)]
pub(crate) fn dial(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    fmt: ValueFmt,
) -> Response {
    let lo = *range.start();
    let hi = *range.end();
    let span = (hi - lo).max(1e-6);
    let mut t = ((*value - lo) / span).clamp(0.0, 1.0);

    let size = Vec2::new(84.0, 112.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let id = ui.id().with("dial").with(id);
    let resp = ui.interact(rect, id, Sense::click_and_drag());

    // Channel well
    ui.painter().rect_filled(rect.shrink(1.0), RADIUS_LG, theme::well_bg().gamma_multiply(0.55));
    ui.painter().rect_stroke(rect.shrink(1.0), RADIUS_LG, theme::stroke_hairline());

    let c = pos2(rect.center().x, rect.top() + 40.0);
    let r = 30.0;
    ui.painter().circle_filled(c, r + 2.0, theme::hairline().gamma_multiply(0.28));
    ui.painter().circle_filled(c, r, theme::surface());
    ui.painter().circle_stroke(c, r, Stroke::new(1.5, theme::hairline()));

    if resp.dragged() {
        let dy = -resp.drag_delta().y;
        t = (t + dy / 140.0).clamp(0.0, 1.0);
        *value = lo + t * span;
    }

    // Background + active arcs (270° travel)
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let steps = 40;
    let arc_r = r - 5.0;
    for i in 0..steps {
        let u0 = i as f32 / steps as f32;
        let u1 = (i + 1) as f32 / steps as f32;
        let ang0 = a0 + sweep * u0;
        let ang1 = a0 + sweep * u1;
        let p0 = pos2(c.x + arc_r * ang0.cos(), c.y + arc_r * ang0.sin());
        let p1 = pos2(c.x + arc_r * ang1.cos(), c.y + arc_r * ang1.sin());
        ui.painter().line_segment([p0, p1], Stroke::new(4.0, theme::well_bg()));
    }
    for i in 0..steps {
        let u0 = i as f32 / steps as f32;
        let u1 = (i + 1) as f32 / steps as f32;
        if u0 >= t {
            break;
        }
        let ang0 = a0 + sweep * u0;
        let ang1 = a0 + sweep * u1.min(t);
        let p0 = pos2(c.x + arc_r * ang0.cos(), c.y + arc_r * ang0.sin());
        let p1 = pos2(c.x + arc_r * ang1.cos(), c.y + arc_r * ang1.sin());
        ui.painter().line_segment([p0, p1], Stroke::new(4.0, theme::accent()));
    }
    // Needle + hub
    let ang = a0 + sweep * t;
    let tip = pos2(c.x + (r - 11.0) * ang.cos(), c.y + (r - 11.0) * ang.sin());
    ui.painter().line_segment([c, tip], Stroke::new(2.0, theme::text()));
    ui.painter().circle_filled(c, 4.5, theme::accent());
    ui.painter().circle_stroke(c, 4.5, Stroke::new(1.0, theme::hairline()));

    let val_s = fmt.format(*value);
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 22.0),
        egui::Align2::CENTER_CENTER,
        val_s,
        FontId::monospace(12.0),
        theme::text(),
    );
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 6.0),
        egui::Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(11.0),
        theme::text_muted(),
    );
    resp.on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text(format!("{label}: {} — drag up/down", fmt.format(*value)))
}

/// Row of icon chips (e.g. Voice / Bass / Drums / Other).
/// Returns the index of the clicked chip, if any.
pub(crate) fn stem_chips(
    ui: &mut Ui,
    items: &[(&str, &str)], // (icon, label)
    selected: usize,
) -> Option<usize> {
    let mut hit = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (i, (icon, label)) in items.iter().enumerate() {
            let sel = i == selected;
            let fill = if sel { theme::accent() } else { theme::surface() };
            let fg = if sel { theme::on_accent() } else { theme::text() };
            let mut job = egui::text::LayoutJob::default();
            job.append(
                icon,
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(14.0, icons::family()),
                    color: fg,
                    ..Default::default()
                },
            );
            job.append(
                &format!("  {label}"),
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(12.0),
                    color: fg,
                    ..Default::default()
                },
            );
            let r = ui.add(
                Button::new(job)
                    .fill(fill)
                    .stroke(if sel { Stroke::NONE } else { Stroke::new(1.0, theme::hairline()) })
                    .rounding(RADIUS_MD)
                    .min_size(Vec2::new(0.0, theme::TOOLBAR_BTN)),
            );
            if r.clicked() {
                hit = Some(i);
            }
        }
    });
    hit
}
