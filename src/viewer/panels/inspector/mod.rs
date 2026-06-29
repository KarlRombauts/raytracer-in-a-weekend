mod camera;
mod object;
mod output;

use eframe::egui::{self, Ui};

use crate::scene::Scene;

use super::super::{icons, state::{Tab, UiState}, widgets};

/// Returns `true` if the scene was dirtied (needs a render restart).
pub fn show_inspector(ui: &mut Ui, ui_state: &mut UiState, scene: &mut Scene) -> bool {
    let mut dirty = false;
    widgets::pill_tabs(ui, &mut ui_state.tab, &[
        (Tab::Object, icons::CUBE, "Object"),
        (Tab::Camera, icons::CAMERA, "Camera"),
        (Tab::Output, icons::IMAGE, "Output"),
    ]);
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        dirty = match ui_state.tab {
            Tab::Object => object::object_tab(ui, ui_state, scene),
            Tab::Camera => camera::camera_tab(ui, &mut scene.camera),
            Tab::Output => output::output_tab(ui, &mut scene.camera),
        };
    });
    dirty
}
