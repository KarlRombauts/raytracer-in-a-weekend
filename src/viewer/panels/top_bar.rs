use eframe::egui::{self, Ui};

use super::super::{
    icons,
    state::{Mode, UiState},
    theme, widgets,
};
use super::Action;
use crate::scene::Scene;

pub fn show_top_bar(
    ui: &mut Ui,
    ui_state: &mut UiState,
    _scene: &Scene,
    can_undo: bool,
    can_redo: bool,
) -> Action {
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
        // Scene chip → returns to the library (Home). Shows the live scene name.
        let chip = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!(
                        "{}  {}  {}",
                        icons::FOLDER,
                        ui_state.scene_name,
                        icons::CARET_DOWN
                    ))
                    .monospace()
                    .color(theme::TEXT),
                )
                .fill(theme::FIELD_BG)
                .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD)),
            )
            .on_hover_text("Open another scene");
        if chip.clicked() {
            action = Action::GoHome;
        }

        // RIGHT GROUP: save buttons — rendered right-to-left so they hug the
        // right edge while the center toggle is placed absolutely below.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // "Save image" — dark pill, NOT accent blue. Picture icon.
            if widgets::pill_button(
                ui,
                &format!("{}  Save image", icons::IMAGE),
                false,
                true,
            )
            .clicked()
            {
                action = Action::SaveImage;
            }
            // "Save scene" — dark pill. Download/save-to-disk icon.
            if widgets::pill_button(ui, &format!("{}  Save scene", icons::DOWNLOAD), false, true)
                .clicked()
            {
                action = Action::SaveScene;
            }
            // "Load scene" — dark pill.
            if widgets::pill_button(ui, &format!("{}  Load scene", icons::FOLDER), false, true)
                .clicked()
            {
                action = Action::LoadScene;
            }

            // Undo / redo — compact icon pills, greyed out when their stack is
            // empty. Rendered right-to-left, so push Redo first to place Undo to
            // its left (reads "Undo  Redo" left-to-right).
            if widgets::pill_button(ui, icons::REDO, false, can_redo)
                .on_hover_text("Redo (Cmd/Ctrl+Shift+Z)")
                .clicked()
            {
                action = Action::Redo;
            }
            if widgets::pill_button(ui, icons::UNDO, false, can_undo)
                .on_hover_text("Undo (Cmd/Ctrl+Z)")
                .clicked()
            {
                action = Action::Undo;
            }
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
