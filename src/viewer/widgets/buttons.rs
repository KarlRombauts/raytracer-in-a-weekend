use eframe::egui::{self, Ui};

use super::super::theme;

/// 30px square icon button with a tinted hover; `danger` tints red on hover.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str, danger: bool) -> bool {
    let resp = ui.add_sized(
        [30.0, 30.0],
        egui::Button::new(egui::RichText::new(icon).color(theme::TEXT_MUTED))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD)),
    );
    let resp = resp.on_hover_text(tooltip);
    if danger && resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            egui::CornerRadius::same(7),
            egui::Stroke::new(1.0, egui::Color32::from_rgb(0x7a, 0x3a, 0x3a)),
            egui::StrokeKind::Inside,
        );
    }
    resp.clicked()
}

/// A text pill button. `accent` fills with the accent colour (primary action).
pub fn pill_button(ui: &mut Ui, label: &str, accent: bool, enabled: bool) -> egui::Response {
    let mut btn = egui::Button::new(egui::RichText::new(label).color(theme::TEXT_STRONG))
        .corner_radius(egui::CornerRadius::same(8))
        .min_size(egui::vec2(0.0, 32.0));
    btn = if accent {
        btn.fill(theme::ACCENT)
    } else {
        // Dark pill: fill #22252a, border #33373d (mockup values).
        btn.fill(egui::Color32::from_rgb(0x22, 0x25, 0x2a))
            .stroke(egui::Stroke::new(
                1.0,
                egui::Color32::from_rgb(0x33, 0x37, 0x3d),
            ))
    };
    ui.add_enabled(enabled, btn)
}
