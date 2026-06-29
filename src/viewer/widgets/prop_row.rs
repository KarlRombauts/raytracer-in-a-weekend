use super::super::theme;
use eframe::egui::{self, Ui};

/// Right-aligned label in a fixed column, then `content` filling the rest.
pub const LABEL_W: f32 = 84.0;

pub fn prop_row<R>(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let h = theme::FIELD_H;
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_W, h),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED));
            },
        );
        content(ui)
    })
    .inner
}

/// Small muted per-vector sub-label (11px, TEXT_DIM). Use for "Position",
/// "Target", "Location", "Rotation", "Scale", etc. — the second level of the
/// inspector hierarchy, below the accent section headers.
pub fn sub_label(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).color(theme::TEXT_DIM).size(11.0));
}

/// Accent-iconed, uppercase, letter-spaced sub-heading.
pub fn section_header(ui: &mut Ui, icon: &str, title: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).color(theme::ACCENT));
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .family(theme::semibold())
                .color(egui::Color32::from_rgb(0xb9, 0xbd, 0xc4))
                .size(11.5),
        );
    });
    ui.add_space(4.0);
}
