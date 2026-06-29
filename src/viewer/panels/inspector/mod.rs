mod camera;
mod object;
mod output;

use eframe::egui::Ui;

use crate::scene::Scene;

use super::super::state::UiState;

/// Returns `true` if the scene was dirtied (needs a render restart).
pub fn show_inspector(_ui: &mut Ui, _ui_state: &mut UiState, _scene: &mut Scene) -> bool {
    false
}
