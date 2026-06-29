#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if std::env::args().any(|a| a == "--gen-thumbnails") {
        raytracer_in_a_weekend::gen_thumbnails().expect("thumbnail generation");
        return;
    }
    raytracer_in_a_weekend::run_default();
}

// The wasm build ships as a cdylib driven from JS (see `WebHandle` in the lib);
// the binary target has no role there, so give it an empty `main` to satisfy
// `cargo check --target wasm32-unknown-unknown`.
#[cfg(target_arch = "wasm32")]
fn main() {}
