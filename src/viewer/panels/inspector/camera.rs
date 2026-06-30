use eframe::egui::{self, Ui};
use crate::camera::CameraConfig;
use super::super::super::{icons, widgets};
use widgets::Axis;

pub fn camera_tab(ui: &mut Ui, cam: &mut CameraConfig) -> bool {
    let mut c = false;
    widgets::section_header(ui, icons::CROSSHAIR, "View");
    widgets::sub_label(ui, "Position");
    c |= widgets::axis_vec(ui, &mut cam.look_from, 1.0, "", None, None);
    widgets::sub_label(ui, "Target");
    c |= widgets::axis_vec(ui, &mut cam.look_at, 1.0, "", None, None);
    c |= widgets::prop_row(ui, "Roll", |ui| {
        widgets::axis_field(ui, Axis::None, &mut cam.roll, 0.5, Some(1), "°", Some(-180.0..=180.0))
    });

    widgets::section_header(ui, icons::APERTURE, "Lens");
    c |= widgets::prop_row(ui, "FOV", |ui| {
        widgets::axis_field(ui, Axis::None, &mut cam.fov, 0.2, Some(1), "°", Some(1.0..=179.0))
    });
    c |= widgets::prop_row(ui, "DoF", |ui| {
        widgets::axis_field(ui, Axis::None, &mut cam.dof_angle, 0.05, Some(2), "°", Some(0.0..=180.0))
    });
    c |= widgets::prop_row(ui, "Focus", |ui| {
        widgets::axis_field(ui, Axis::None, &mut cam.focus_dist, 1.0, Some(1), "", Some(0.001..=1.0e6))
    });

    widgets::section_header(ui, icons::PALETTE, "World");

    // HDR environment map. `Some(name)` looks up `assets/hdrs/<name>.hdr` and
    // uses it for the sky (background + reflections + image-based lighting);
    // `None` falls back to the flat colour below.
    c |= widgets::prop_row(ui, "Sky Map", |ui| {
        let skies = crate::texture::env_map::available_skies();
        let mut changed = false;
        egui::ComboBox::from_id_salt("sky_map")
            .selected_text(cam.sky.clone().unwrap_or_else(|| "Solid colour".to_string()))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                if ui.selectable_label(cam.sky.is_none(), "Solid colour").clicked()
                    && cam.sky.is_some()
                {
                    cam.sky = None;
                    changed = true;
                }
                for s in &skies {
                    let selected = cam.sky.as_deref() == Some(s.as_str());
                    if ui.selectable_label(selected, s).clicked() && !selected {
                        cam.sky = Some(s.clone());
                        changed = true;
                    }
                }
            });
        changed
    });

    // Flat sky colour — used when no HDR map is selected. Edited as linear RGB.
    c |= widgets::prop_row(ui, "Sky Colour", |ui| {
        let mut rgb = [cam.background.x, cam.background.y, cam.background.z];
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            cam.background = crate::color::Color::new(rgb[0], rgb[1], rgb[2]);
            true
        } else {
            false
        }
    });
    c
}
