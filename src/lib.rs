pub mod camera;
pub mod color;
pub mod geometry;
pub mod group;
pub mod interval;
pub mod material;
pub mod platform;
pub mod ray;
pub mod render;
pub mod sampling;
pub mod scene;
pub mod scenes;
pub mod texture;
pub mod vec3;
pub mod viewer;

/// Native entry: open the interactive viewer on the default scene.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_default() {
    use crate::scenes::cornell_box;
    viewer::run(cornell_box());
}
