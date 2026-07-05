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

/// Which top-level screen the app shows: the library (Home), a transient
/// loading view while a scene fetches/decodes, or the editor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Loading,
    Editor,
}

/// The currently-selected object, owning the invariant "valid against the
/// scene's object count". Callers read through `get(len)`, which drops a stale
/// index (e.g. after an undo shrank the scene) instead of handing back an
/// out-of-range value — the one place that check now lives.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Selection(Option<usize>);

impl Selection {
    /// The selected index, validated against the current object count.
    pub fn get(self, len: usize) -> Option<usize> {
        self.0.filter(|&i| i < len)
    }

    /// The raw stored index without validation — for the few callers that
    /// immediately re-check it against live data themselves.
    pub fn raw(self) -> Option<usize> {
        self.0
    }

    pub fn set(&mut self, i: usize) {
        self.0 = Some(i);
    }

    /// Set (or clear) from an optional index — e.g. the result of a viewport
    /// pick, which selects a hit object or clears on a miss.
    pub fn set_opt(&mut self, i: Option<usize>) {
        self.0 = i;
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }
}

pub struct UiState {
    /// Library vs editor.
    pub screen: Screen,
    /// Display name of the current scene (shown in the top-bar chip).
    pub scene_name: String,
    pub mode: Mode,
    pub selected: Selection,
    pub tab: Tab,
    pub gizmo_local: bool,
    pub gizmo_modes: raster::gizmo::GizmoModes,
    /// egui time of the last camera motion (preview debounce).
    pub last_interact: f64,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            screen: Screen::Home,
            scene_name: "untitled".to_string(),
            mode: Mode::Render,
            selected: Selection::default(),
            tab: Tab::Object,
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
