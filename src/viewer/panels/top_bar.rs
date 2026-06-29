use eframe::egui::Ui;

use crate::scene::Scene;

use super::super::state::UiState;
use super::Action;

pub fn show_top_bar(_ui: &mut Ui, _ui_state: &mut UiState, _scene: &mut Scene) -> Action {
    Action::None
}
