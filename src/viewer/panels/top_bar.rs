use eframe::egui::{self, Ui};

use super::super::{
    icons,
    state::{Mode, UiState},
    theme, widgets,
};
use super::Action;
use crate::scene::Scene;

pub fn show_top_bar(ui: &mut Ui, ui_state: &mut UiState, _scene: &Scene) -> Action {
    let mut action = Action::None;
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        // Logo glyph + wordmark.
        ui.label(
            egui::RichText::new(icons::APERTURE)
                .color(theme::ACCENT)
                .size(18.0),
        );
        ui.label(
            egui::RichText::new("Lumi")
                .family(theme::semibold())
                .color(theme::TEXT_STRONG)
                .size(15.0),
        );
        ui.add_space(6.0);
        // Scene chip (display-only until save/load lands).
        ui.label(
            egui::RichText::new(format!("{}  cornell-box.scene", icons::FOLDER))
                .monospace()
                .color(theme::TEXT),
        );

        // Right side: save buttons + mode toggle (right-to-left).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Save image (primary action) on the far right.
            if widgets::pill_button(ui, &format!("{}  Save image", icons::DOWNLOAD), true, true)
                .clicked()
            {
                action = Action::SaveImage;
            }
            // Save scene (disabled, coming soon).
            let _ =
                widgets::pill_button(ui, &format!("{}  Save scene", icons::FLOPPY), false, false)
                    .on_hover_text("Scene save/load is coming soon");
            ui.add_space(8.0);
            // Render / Edit mode toggle.
            widgets::segmented(
                ui,
                &mut ui_state.mode,
                (Mode::Render, icons::PLAY, "Render"),
                (Mode::Edit, icons::ARROWS_OUT_CARDINAL, "Edit"),
            );
        });
    });
    action
}
