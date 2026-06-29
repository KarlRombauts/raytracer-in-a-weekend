use eframe::egui::{self, Ui};

/// A styled combo. `body` adds `selectable_label`s and returns whether the
/// selection changed. Returns that flag.
pub fn styled_combo(
    ui: &mut Ui,
    id: &str,
    current: &str,
    width: f32,
    body: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(width)
        .show_ui(ui, |ui| {
            changed = body(ui);
        });
    changed
}
