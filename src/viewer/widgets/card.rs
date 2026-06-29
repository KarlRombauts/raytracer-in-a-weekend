use eframe::egui::{self};

/// Translucent dark frame for floating viewport overlays (blur isn't available
/// in egui, so we use an opaque-ish dark fill instead).
pub fn overlay_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_premultiplied(14, 15, 18, 210))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(18)))
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(12, 8))
}
