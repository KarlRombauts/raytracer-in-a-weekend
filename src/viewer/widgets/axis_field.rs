use std::ops::RangeInclusive;

use eframe::egui::{self, Ui};

use crate::vec3::Vec3;

use super::super::theme;

#[derive(Clone, Copy)]
pub enum Axis {
    X,
    Y,
    Z,
    None,
}

impl Axis {
    fn glyph(self) -> Option<(&'static str, egui::Color32)> {
        match self {
            Axis::X => Some(("X", theme::AXIS_X)),
            Axis::Y => Some(("Y", theme::AXIS_Y)),
            Axis::Z => Some(("Z", theme::AXIS_Z)),
            Axis::None => Option::None,
        }
    }
}

pub fn axis_field(
    ui: &mut Ui,
    axis: Axis,
    value: &mut f32,
    speed: f32,
    decimals: Option<usize>,
    suffix: &str,
    range: Option<RangeInclusive<f32>>,
) -> bool {
    let mut dv = egui::DragValue::new(value).speed(speed);
    if !suffix.is_empty() {
        dv = dv.suffix(suffix);
    }
    if let Some(d) = decimals {
        dv = dv.fixed_decimals(d);
    }
    if let Some(r) = range {
        dv = dv.range(r);
    }

    let mut changed = false;
    egui::Frame::NONE
        .fill(theme::FIELD_BG)
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(2, 0))
        .show(ui, |ui| {
            ui.set_height(theme::FIELD_H);
            ui.horizontal_centered(|ui| {
                if let Some((g, c)) = axis.glyph() {
                    ui.allocate_ui_with_layout(
                        egui::vec2(18.0, theme::FIELD_H),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.label(egui::RichText::new(g).monospace().color(c));
                        },
                    );
                }
                ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
                ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
                ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::NONE;
                ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                // R4: monospace font for the DragValue text at a fixed size so all
                // value fields (X/Y/Z, scalar, int) render identically.
                // R5: focused/editing state — accent border, accent-soft selection,
                // matching rounding so the blue doesn't look jarring.
                ui.visuals_mut().widgets.active.bg_stroke =
                    egui::Stroke::new(1.0, theme::ACCENT);
                ui.visuals_mut().widgets.open.bg_stroke =
                    egui::Stroke::new(1.0, theme::ACCENT);
                ui.visuals_mut().selection.bg_fill = theme::accent_soft();
                ui.visuals_mut().selection.stroke =
                    egui::Stroke::new(1.0, theme::ACCENT);
                ui.style_mut().override_font_id =
                    Some(egui::FontId::new(12.5, egui::FontFamily::Monospace));
                changed = ui
                    .add_sized([ui.available_width(), theme::FIELD_H], dv)
                    .changed();
                ui.style_mut().override_font_id = None;
            });
        });
    changed
}

pub fn axis_vec(
    ui: &mut Ui,
    v: &mut Vec3,
    speed: f32,
    suffix: &str,
    decimals: Option<usize>,
    range: Option<RangeInclusive<f32>>,
) -> bool {
    let mut c = false;
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
        for (axis, comp) in [
            (Axis::X, &mut v.x),
            (Axis::Y, &mut v.y),
            (Axis::Z, &mut v.z),
        ] {
            ui.allocate_ui(egui::vec2(w, theme::FIELD_H), |ui| {
                c |= axis_field(ui, axis, comp, speed, decimals, suffix, range.clone());
            });
        }
    });
    c
}

pub fn int_field(ui: &mut Ui, value: &mut u32, range: Option<RangeInclusive<u32>>) -> bool {
    let mut f = *value as f32;
    let changed = axis_field(
        ui,
        Axis::None,
        &mut f,
        1.0,
        Some(0),
        "",
        range.map(|r| (*r.start() as f32)..=(*r.end() as f32)),
    );
    if changed {
        *value = f.round().max(0.0) as u32;
    }
    changed
}
