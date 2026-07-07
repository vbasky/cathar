//! Phosphor icon helpers (font installed in `fonts.rs`).

use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{
    Button, Color32, FontFamily, FontId, Response, RichText, SelectableLabel, TextFormat, Ui, Vec2,
    Widget, WidgetText,
};

pub(crate) use egui_phosphor::regular::{
    ARROW_COUNTER_CLOCKWISE, ARROWS_CLOCKWISE, DESKTOP, FLOPPY_DISK, FOLDER_OPEN, MOON, PAUSE,
    PLAY, SUN, SWAP,
};

pub(crate) const ICON_SIZE: f32 = 17.0;
pub(crate) const TOOLBAR_ICON: f32 = 16.0;
pub(crate) const TRANSPORT_ICON: f32 = 20.0;

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
    Button::new(widget(icon, TOOLBAR_ICON)).min_size(Vec2::new(32.0, 26.0))
}

/// Larger transport control (play/pause).
pub(crate) fn transport_button(icon: &'static str) -> Button<'static> {
    Button::new(widget(icon, TRANSPORT_ICON)).min_size(Vec2::new(40.0, 32.0))
}

/// Toggle toolbar control for A/B compare and similar states.
pub(crate) fn toolbar_toggle(selected: bool, icon: &'static str) -> SelectableLabel {
    SelectableLabel::new(selected, widget(icon, TOOLBAR_ICON))
}

/// Menu row: phosphor glyph + label in one clickable item.
pub(crate) struct MenuItem {
    job: LayoutJob,
}

impl MenuItem {
    pub(crate) fn new(icon: &str, text: &str) -> Self {
        let mut job = LayoutJob::default();
        job.append(
            icon,
            0.0,
            // PLACEHOLDER → egui recolours with the widget's text colour;
            // TextFormat's default colour is grey, which reads as "disabled".
            TextFormat {
                font_id: FontId::new(ICON_SIZE, family()),
                color: Color32::PLACEHOLDER,
                ..Default::default()
            },
        );
        job.append(
            &format!("  {text}"),
            0.0,
            TextFormat {
                font_id: FontId::proportional(ICON_SIZE),
                color: Color32::PLACEHOLDER,
                ..Default::default()
            },
        );
        Self { job }
    }
}

impl Widget for MenuItem {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.add(Button::new(self.job))
    }
}
