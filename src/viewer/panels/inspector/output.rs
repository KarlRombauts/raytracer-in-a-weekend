use eframe::egui::{self, Ui};
use crate::camera::CameraConfig;
use super::super::super::{icons, theme, widgets};

pub fn output_tab(ui: &mut Ui, cam: &mut CameraConfig) -> bool {
    let mut c = false;
    widgets::section_header(ui, icons::IMAGE, "Resolution");
    let cur_h = ((cam.image_width as f64 / cam.aspect_ratio).round().max(1.0)) as u32;
    c |= widgets::prop_row(ui, "Width", |ui| {
        let mut w = cam.image_width;
        if widgets::int_field(ui, &mut w, Some(1..=8192)) {
            cam.image_width = w.max(1);
            cam.aspect_ratio = cam.image_width as f64 / cur_h as f64;
            true
        } else {
            false
        }
    });
    c |= widgets::prop_row(ui, "Height", |ui| {
        let mut h = cur_h;
        if widgets::int_field(ui, &mut h, Some(1..=8192)) {
            cam.aspect_ratio = cam.image_width as f64 / h.max(1) as f64;
            true
        } else {
            false
        }
    });

    widgets::section_header(ui, icons::SLIDERS, "Quality");
    c |= widgets::prop_row(ui, "Samples", |ui| {
        widgets::int_field(ui, &mut cam.samples, Some(1..=100_000))
    });
    c |= widgets::prop_row(ui, "Max bounces", |ui| {
        widgets::int_field(ui, &mut cam.max_depth, Some(1..=1_000))
    });

    widgets::prop_row(ui, "Format", |ui| {
        ui.label(egui::RichText::new("PNG · 16-bit").color(theme::TEXT));
    });
    c
}
