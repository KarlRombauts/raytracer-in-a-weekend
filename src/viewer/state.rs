use super::raster;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Render,
    Edit,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Object,
    Camera,
    Output,
}

pub struct UiState {
    pub mode: Mode,
    pub selected: Option<usize>,
    pub tab: Tab,
    pub add_menu_open: bool,
    pub gizmo_local: bool,
    pub gizmo_modes: raster::gizmo::GizmoModes,
    /// egui time of the last camera motion (preview debounce).
    pub last_interact: f64,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            mode: Mode::Render,
            selected: None,
            tab: Tab::Object,
            add_menu_open: false,
            gizmo_local: false,
            gizmo_modes: raster::gizmo::GizmoModes {
                translate: true,
                rotate: true,
                scale: true,
            },
            last_interact: -1.0,
        }
    }
}
