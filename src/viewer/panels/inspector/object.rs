use eframe::egui::{self, Ui};
use crate::scene::{Scene, Shape};
use super::super::super::{command::SceneCommand, controls, icons, state::UiState, theme, widgets};

pub fn object_tab(
    ui: &mut Ui,
    ui_state: &mut UiState,
    scene: &mut Scene,
    cmds: &mut Vec<SceneCommand>,
) -> bool {
    let mut dirty = false;
    let Some(i) = ui_state.selected.get(scene.objects.len()) else {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.label(egui::RichText::new(icons::CUBE).color(theme::TEXT_DIM).size(28.0));
            ui.label(egui::RichText::new("No object selected").color(theme::TEXT_MUTED));
            ui.label(
                egui::RichText::new("Pick something in the scene, or add a new object.")
                    .color(theme::TEXT_DIM)
                    .size(12.0),
            );
        });
        return false;
    };

    // Header: icon + name + type badge + duplicate + delete.
    let mut do_dup = false;
    let mut do_del = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(controls::shape_icon(&scene.objects[i].shape)).color(theme::SELECTION));
        ui.add(
            egui::TextEdit::singleline(&mut scene.objects[i].name)
                .desired_width(140.0)
                .margin(egui::Margin::symmetric(8, 4)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::TRASH, "Delete object", true) {
                do_del = true;
            }
            if widgets::icon_button(ui, icons::COPY, "Duplicate object", false) {
                do_dup = true;
            }
        });
    });

    if do_dup {
        cmds.push(SceneCommand::DuplicateObject(i));
        return dirty;
    }
    if do_del {
        cmds.push(SceneCommand::DeleteObject(i));
        return dirty;
    }

    // Meshes get full material editing too: their BVH is material-agnostic and
    // the material is applied via a wrapper at world-build time (no rebuild).
    widgets::section_header(ui, icons::PALETTE, "Material");
    dirty |= controls::material_controls(ui, &mut scene.objects[i].material);

    if matches!(scene.objects[i].shape, Shape::Sphere { .. } | Shape::Box { .. }) {
        widgets::section_header(ui, icons::SHAPES, "Geometry");
        dirty |= controls::shape_controls(ui, &mut scene.objects[i].shape);
    }

    widgets::section_header(ui, icons::ARROWS_OUT_CARDINAL, "Transform");
    dirty |= controls::transform_controls(ui, &mut scene.objects[i].transform);

    dirty
}
