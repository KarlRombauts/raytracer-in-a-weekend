use eframe::egui::Ui;
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
    c |= widgets::prop_row(ui, "Sky", |ui| {
        // `background` is the flat colour returned when a ray misses every
        // object (the sky). egui edits it as linear RGB in 0..1.
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
