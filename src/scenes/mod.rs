pub mod cornell_box;
pub mod new_bvh;

// These demo scenes render straight to a file via `Camera::render`, which is
// native-only (it uses indicatif + filesystem output). They are not used by the
// browser entry point (which builds a `Scene` and renders interactively), so
// gate them out of the wasm build to keep it compiling cleanly.
#[cfg(not(target_arch = "wasm32"))]
pub mod bouncing_spheres;
#[cfg(not(target_arch = "wasm32"))]
pub mod earth;
#[cfg(not(target_arch = "wasm32"))]
pub mod obj;
#[cfg(not(target_arch = "wasm32"))]
pub mod perlin_spheres;
#[cfg(not(target_arch = "wasm32"))]
pub mod quads;
#[cfg(not(target_arch = "wasm32"))]
pub mod simple_light;
#[cfg(not(target_arch = "wasm32"))]
pub mod tris;

pub use cornell_box::*;
pub use new_bvh::*;

#[cfg(not(target_arch = "wasm32"))]
pub use bouncing_spheres::*;
#[cfg(not(target_arch = "wasm32"))]
pub use earth::*;
#[cfg(not(target_arch = "wasm32"))]
pub use obj::*;
#[cfg(not(target_arch = "wasm32"))]
pub use perlin_spheres::*;
#[cfg(not(target_arch = "wasm32"))]
pub use quads::*;
#[cfg(not(target_arch = "wasm32"))]
pub use simple_light::*;
#[cfg(not(target_arch = "wasm32"))]
pub use tris::*;
