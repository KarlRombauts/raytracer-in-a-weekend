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

    // We need three groups: left (logo + scene chip), center (toggle), right
    // (save buttons). egui's horizontal layout can't center a widget between
    // two variable-width groups natively, so we allocate the full bar rect and
    // use `ui.put` to place the toggle at the exact center.
    let bar_rect = ui.max_rect();

    // --- LEFT GROUP: logo + scene chip ---
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add_space(4.0);
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
        // Scene chip with caret (display-only until save/load lands).
        ui.label(
            egui::RichText::new(format!(
                "{}  cornell-box.scene  {}",
                icons::FOLDER,
                icons::CARET_DOWN
            ))
            .monospace()
            .color(theme::TEXT),
        );

        // RIGHT GROUP: save buttons — rendered right-to-left so they hug the
        // right edge while the center toggle is placed absolutely below.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // "Save image" — dark pill, NOT accent blue.
            if widgets::pill_button(
                ui,
                &format!("{}  Save image", icons::DOWNLOAD),
                false,
                true,
            )
            .clicked()
            {
                action = Action::SaveImage;
            }
            // "Save scene" — dark pill, disabled.
            let _ = widgets::pill_button(
                ui,
                &format!("{}  Save scene", icons::FLOPPY),
                false,
                false,
            )
            .on_hover_text("Scene save/load is coming soon");
        });
    });

    // --- CENTER: Render / Edit segmented toggle placed at the bar's center ---
    // Measure the toggle's desired size by creating a transient sizing ui, then
    // place it with `ui.put` at a centered rect within the bar.
    //
    // The toggle is ~180px wide. We place it centered in bar_rect.
    let toggle_w = 184.0;
    let toggle_h = 38.0;
    let center_x = bar_rect.center().x;
    let center_y = bar_rect.center().y;
    let toggle_rect = egui::Rect::from_center_size(
        egui::pos2(center_x, center_y),
        egui::vec2(toggle_w, toggle_h),
    );

    ui.put(toggle_rect, |ui: &mut Ui| {
        // segmented returns bool but put needs a Response; wrap in a horizontal.
        ui.horizontal(|ui| {
            widgets::segmented(
                ui,
                &mut ui_state.mode,
                (Mode::Render, icons::PLAY, "Render"),
                (Mode::Edit, icons::ARROWS_OUT_CARDINAL, "Edit"),
            );
        })
        .response
    });

    action
}
