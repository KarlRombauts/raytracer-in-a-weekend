use eframe::egui::{self, Rect, Ui};

use crate::camera::CameraConfig;

use super::super::{icons, raster::gizmo::GizmoModes, state::Mode, theme, widgets};

/// Draw viewport overlays (resolution badge, Reset-camera chip, Edit toolbar)
/// floating on top of `rect` using `egui::Area` at `Order::Foreground`.
/// Returns `true` if the Reset-camera chip was clicked.
pub fn overlays(
    ui: &Ui,
    rect: Rect,
    mode: Mode,
    gizmo_modes: &mut GizmoModes,
    gizmo_local: &mut bool,
    res: (u32, u32),
) -> bool {
    let mut reset = false;

    // Resolution badge (top-left).
    egui::Area::new("vp_res".into())
        .fixed_pos(rect.left_top() + egui::vec2(16.0, 14.0))
        .order(egui::Order::Foreground)
        .movable(false)
        .show(ui.ctx(), |ui| {
            widgets::overlay_frame().show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("{}  {} × {}", icons::IMAGE, res.0, res.1))
                        .monospace()
                        .color(theme::TEXT),
                );
            });
        });

    // Reset-camera chip (bottom-left).
    egui::Area::new("vp_reset".into())
        .fixed_pos(rect.left_bottom() + egui::vec2(16.0, -44.0))
        .order(egui::Order::Foreground)
        .movable(false)
        .show(ui.ctx(), |ui| {
            widgets::overlay_frame().show(ui, |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{}  Reset camera", icons::RESET))
                                .color(theme::TEXT),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE),
                    )
                    .clicked()
                {
                    reset = true;
                }
            });
        });

    // Edit toolbar (top-center) — only in Edit mode.
    if mode == Mode::Edit {
        egui::Area::new("vp_tools".into())
            .fixed_pos(rect.center_top() + egui::vec2(-150.0, 14.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                widgets::overlay_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        tool(
                            ui,
                            &mut gizmo_modes.translate,
                            icons::ARROWS_OUT_CARDINAL,
                            "Move",
                        );
                        tool(
                            ui,
                            &mut gizmo_modes.rotate,
                            icons::ARROWS_CLOCKWISE,
                            "Rotate",
                        );
                        tool(ui, &mut gizmo_modes.scale, icons::RESIZE, "Scale");
                        ui.separator();
                        local_axes_toggle(ui, gizmo_local);
                    });
                });
            });
    }

    reset
}

/// Gizmo mode toggle pill: accent-filled when active, transparent when not.
fn tool(ui: &mut Ui, on: &mut bool, icon: &str, label: &str) {
    let (fill, text_color) = if *on {
        (theme::accent_soft(), theme::ACCENT)
    } else {
        (egui::Color32::TRANSPARENT, theme::TEXT_MUTED)
    };
    let btn = egui::Button::new(
        egui::RichText::new(format!("{icon}  {label}")).color(text_color),
    )
    .fill(fill)
    .corner_radius(egui::CornerRadius::same(7))
    .stroke(egui::Stroke::NONE);
    if ui.add(btn).clicked() {
        *on ^= true;
    }
}

/// Custom "Local axes" toggle: a small accent-filled square (with check when
/// on, outlined when off) followed by the label text.
fn local_axes_toggle(ui: &mut Ui, on: &mut bool) {
    let box_size = 15.0;
    let desired = egui::vec2(box_size, box_size);
    // Lay out the square + label in a horizontal group.
    let resp = ui
        .horizontal(|ui| {
            let (rect, response) =
                ui.allocate_exact_size(desired, egui::Sense::click());
            let painter = ui.painter();
            if *on {
                painter.rect_filled(
                    rect,
                    egui::CornerRadius::same(4),
                    theme::ACCENT,
                );
                // Draw a checkmark glyph centered in the square.
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::CHECK,
                    egui::FontId::proportional(11.0),
                    egui::Color32::WHITE,
                );
            } else {
                painter.rect_stroke(
                    rect,
                    egui::CornerRadius::same(4),
                    egui::Stroke::new(1.0, theme::BORDER_FIELD),
                    egui::StrokeKind::Inside,
                );
            }
            ui.label(
                egui::RichText::new("Local axes")
                    .color(theme::TEXT_MUTED)
                    .size(12.5),
            );
            response
        })
        .inner;
    if resp.clicked() {
        *on ^= true;
    }
}

/// Output from [`status_dock`].
pub struct StatusOut {
    pub restart: bool,
    pub dirty: bool,
}

/// Render the thin progress line + status row below the viewport.
///
/// Returns a [`StatusOut`] indicating whether a restart was requested and/or
/// the camera config (samples / bounces) was changed.
pub fn status_dock(
    ui: &mut Ui,
    mode: Mode,
    done: bool,
    passes: u32,
    total: u32,
    elapsed: f32,
    cam: &mut CameraConfig,
) -> StatusOut {
    let mut out = StatusOut {
        restart: false,
        dirty: false,
    };

    // Thin progress line.
    let frac = if total > 0 {
        passes as f32 / total as f32
    } else {
        0.0
    };
    let (line, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(line, 0.0, egui::Color32::from_rgb(0x1c, 0x1f, 0x24));
    let mut fill = line;
    fill.set_width(line.width() * frac);
    ui.painter().rect_filled(fill, 0.0, theme::ACCENT);

    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let (dot, text) = match (mode, done) {
            (Mode::Edit, _) => (theme::TEXT_DIM, "Editing".to_string()),
            (Mode::Render, true) => (
                egui::Color32::from_rgb(0x54, 0xc9, 0x8a),
                "Done".to_string(),
            ),
            (Mode::Render, false) => (theme::ACCENT, "Rendering\u{2026}".to_string()),
        };
        ui.label(egui::RichText::new("●").color(dot).size(10.0));
        ui.label(egui::RichText::new(text).color(theme::TEXT_STRONG));
        if mode == Mode::Render {
            ui.separator();
            ui.label(
                egui::RichText::new(format!("{passes} / {total} passes · {elapsed:.1}s"))
                    .monospace()
                    .color(theme::TEXT),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::RESET, "Restart render", false) {
                out.restart = true;
            }
            // Fixed-width wrappers prevent int_field from stretching to fill
            // available width, which caused overlap with the status text.
            ui.label(
                egui::RichText::new("Bounces")
                    .color(theme::TEXT_DIM)
                    .size(11.0),
            );
            ui.allocate_ui(egui::vec2(56.0, theme::FIELD_H), |ui| {
                out.dirty |= widgets::int_field(ui, &mut cam.max_depth, Some(1..=1_000));
            });
            ui.label(
                egui::RichText::new("Samples")
                    .color(theme::TEXT_DIM)
                    .size(11.0),
            );
            ui.allocate_ui(egui::vec2(78.0, theme::FIELD_H), |ui| {
                out.dirty |= widgets::int_field(ui, &mut cam.samples, Some(1..=100_000));
            });
        });
    });

    out
}
