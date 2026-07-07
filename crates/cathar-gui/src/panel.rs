//! Logic-style inspector panels — label/value rows and full-width sliders.

use std::hash::Hash;
use std::ops::RangeInclusive;

use egui::{Align, Button, CollapsingHeader, Layout, Response, RichText, Slider, Ui, Vec2};

/// Fixed inspector column width — Logic-style narrow plugin pane.
pub(crate) const INSPECTOR_W: f32 = 300.0;
/// Slider track width inside the pane (panel width minus padding/scrollbar).
pub(crate) const SLIDER_W: f32 = 248.0;

/// Tune spacing for the right-hand inspector column.
///
/// `panel_w` must come from the [`SidePanel`] `ui`, not from inside a
/// [`ScrollArea`] (where `available_width` is often unbounded and blows out layout).
pub(crate) fn prepare(ui: &mut Ui, panel_w: f32) {
    let content_w = panel_w.min(INSPECTOR_W);
    ui.set_max_width(content_w);
    ui.set_width(content_w);
    ui.spacing_mut().item_spacing = Vec2::new(0.0, 12.0);
    ui.spacing_mut().slider_width = SLIDER_W;
    ui.spacing_mut().indent = 12.0;
}

/// Uppercase section divider (Logic library-column style).
pub(crate) fn column_heading(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title.to_uppercase()).size(11.0).weak().extra_letter_spacing(0.6));
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(10.0);
}

/// Collapsible FX module with comfortable vertical rhythm.
pub(crate) fn fx_section(
    ui: &mut Ui,
    id: impl Hash,
    title: &str,
    add_contents: impl FnOnce(&mut Ui),
) {
    CollapsingHeader::new(RichText::new(title).size(13.0)).id_salt(id).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 14.0;
        ui.add_space(2.0);
        ui.indent("fx", |ui| {
            ui.set_max_width(SLIDER_W + 20.0);
            add_contents(ui);
        });
    });
}

fn param_label_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).monospace().size(11.5).weak());
        });
    });
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
        param_label_row(ui, label, &display);
        ui.add(Slider::new(value, range).show_value(false))
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
        param_label_row(ui, label, &display);
        ui.add(Slider::new(value, range).integer().show_value(false))
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
    ui.label(RichText::new(text).size(11.0).weak());
    ui.add_space(4.0);
}

fn apply_button_widget(label: &str) -> Button<'static> {
    Button::new(RichText::new(label.to_string()).size(12.0)).min_size(Vec2::new(SLIDER_W, 28.0))
}

/// Full-width primary action at the bottom of a module.
pub(crate) fn apply_button(ui: &mut Ui, label: &str) -> Response {
    ui.add_space(4.0);
    ui.add(apply_button_widget(label))
}

/// Like [`apply_button`] but returns a widget for `add_enabled`.
pub(crate) fn apply_button_enabled(ui: &mut Ui, enabled: bool, label: &str) -> Response {
    ui.add_space(4.0);
    ui.add_enabled(enabled, apply_button_widget(label))
}
