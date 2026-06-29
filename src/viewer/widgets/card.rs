use eframe::egui::{self};

use super::super::theme;

pub fn card(_ui: &egui::Ui) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme::FIELD_BG)
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(11))
}

/// Translucent dark frame for floating viewport overlays (blur isn't available
/// in egui, so we use an opaque-ish dark fill instead).
pub fn overlay_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_premultiplied(14, 15, 18, 210))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(18)))
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(12, 8))
}
