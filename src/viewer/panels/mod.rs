#![allow(dead_code)]

mod inspector;
mod outliner;
mod top_bar;
mod viewport;

pub use inspector::show_inspector;
pub use outliner::show_outliner;
pub use top_bar::show_top_bar;
pub use viewport::show_viewport;

/// One-shot actions a panel asks `ViewerApp` to perform after layout.
#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    None,
    SaveImage,
    SaveScene,
    ResetCamera,
    Restart,
}
