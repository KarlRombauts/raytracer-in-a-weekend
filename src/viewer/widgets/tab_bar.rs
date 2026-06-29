use eframe::egui::{self, Ui};

use super::super::theme;

/// 3-up pill tab selector. `tabs` is (value, icon, label). Returns true if the
/// selection changed.
pub fn pill_tabs<T: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut T,
    tabs: &[(T, &str, &str)],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let n = tabs.len() as f32;
        let w = (ui.available_width() - (n - 1.0) * ui.spacing().item_spacing.x) / n;
        for (val, icon, label) in tabs {
            let active = *current == *val;
            let text = egui::RichText::new(format!("{icon}  {label}")).color(if active {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            });
            let fill = if active {
                theme::accent_soft()
            } else {
                egui::Color32::TRANSPARENT
            };
            // Active tab gets a 1px ACCENT inset border; inactive: no border.
            let stroke = if active {
                egui::Stroke::new(1.0, theme::ACCENT)
            } else {
                egui::Stroke::NONE
            };
            let btn = egui::Button::new(text)
                .fill(fill)
                .stroke(stroke)
                .corner_radius(egui::CornerRadius::same(7))
                .min_size(egui::vec2(w, 31.0));
            if ui.add(btn).clicked() && !active {
                *current = *val;
                changed = true;
            }
        }
    });
    changed
}

/// Two-segment toggle (Render / Edit) styled as the mockup centre pill.
pub fn segmented<M: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut M,
    left: (M, &str, &str),
    right: (M, &str, &str),
) -> bool {
    let mut changed = false;
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(0x0f, 0x10, 0x14))
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (val, icon, label) in [left, right] {
                    let active = *current == val;
                    let text = egui::RichText::new(format!("{icon}  {label}")).color(if active {
                        egui::Color32::WHITE
                    } else {
                        theme::TEXT_MUTED
                    });
                    let fill = if active {
                        theme::ACCENT
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    // Only active segment gets a stroke; inactive: no border.
                    let stroke = egui::Stroke::NONE;
                    let btn = egui::Button::new(text)
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(egui::CornerRadius::same(7));
                    if ui.add(btn).clicked() && !active {
                        *current = val;
                        changed = true;
                    }
                }
            });
        });
    changed
}
