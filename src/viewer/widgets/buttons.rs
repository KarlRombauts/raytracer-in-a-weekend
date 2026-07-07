use eframe::egui::{self, Ui};

use super::super::theme;

/// 30px square icon button. A single glyph whose colour is chosen by state —
/// muted at rest, bright (or red, when `danger`) on hover — painted once. (The
/// old version drew a second, differently-sized glyph on top on hover, which
/// showed as two misaligned icons; egui's `override_text_color` blocks the more
/// idiomatic per-state `fg_stroke`, so we paint the one glyph ourselves.)
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str, danger: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
    let resp = resp
        .on_hover_text(tooltip)
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    let (fill, border, glyph) = if danger && resp.hovered() {
        (theme::danger_soft(), theme::DANGER_BORDER, theme::DANGER)
    } else if resp.hovered() {
        (theme::SURFACE_HOVER, theme::BORDER_HOVER, theme::TEXT)
    } else {
        (egui::Color32::TRANSPARENT, theme::BORDER_FIELD, theme::TEXT_MUTED)
    };

    // Resolve the glyph font from the Button text style so the icon matches every
    // other button at rest and on hover (one size, not two).
    let font = egui::TextStyle::Button.resolve(ui.style());
    let p = ui.painter();
    p.rect(
        rect,
        egui::CornerRadius::same(7),
        fill,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    p.text(rect.center(), egui::Align2::CENTER_CENTER, icon, font, glyph);
    resp.clicked()
}

/// A text pill button. `accent` fills with the accent colour (primary action);
/// otherwise a dark pill that brightens its border on hover (feedback driven by
/// egui's per-state widget visuals rather than a fixed fill).
pub fn pill_button(ui: &mut Ui, label: &str, accent: bool, enabled: bool) -> egui::Response {
    let text = egui::RichText::new(label).color(theme::TEXT_STRONG);
    let radius = egui::CornerRadius::same(8);
    let min = egui::vec2(0.0, 32.0);

    if accent {
        return ui.add_enabled(
            enabled,
            egui::Button::new(text).corner_radius(radius).min_size(min).fill(theme::ACCENT),
        );
    }

    // Dark pill: let the widget visuals carry the fill/border per state so it
    // reads as interactive (border brightens on hover) instead of a static chip.
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for st in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            st.weak_bg_fill = theme::SURFACE_HOVER;
            st.bg_stroke = egui::Stroke::new(1.0, theme::BORDER_PILL);
            st.corner_radius = radius;
        }
        w.hovered.bg_stroke = egui::Stroke::new(1.0, theme::BORDER_HOVER);
        ui.add_enabled(enabled, egui::Button::new(text).corner_radius(radius).min_size(min))
    })
    .inner
}
