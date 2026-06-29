# Raytracer tasks. `target-cpu=native` is applied via .cargo/config.toml,
# so the release recipes below already build with full CPU optimizations.

# List available recipes
default:
    @just --list

# Render the current scene (edit `main.rs` to pick the scene) with full optimizations.
# The binary prints its own render time when finished and writes test.png.
render:
    cargo run --release

# Build optimized, then render and report total wall-clock time.
bench:
    cargo build --release
    time ./target/release/raytracer-in-a-weekend

# Open the most recent render.
view:
    open test.png

# Render, then open the result.
render-view: render view

# Render pre-baked thumbnails for every library sample scene.
thumbnails:
    cargo run --release -- --gen-thumbnails

# Remove build artifacts.
clean:
    cargo clean

# Pin a nightly that ships rust-src; override with `just nightly=... web`.
nightly := "nightly"

# Build the threaded WebAssembly bundle into ./dist (nightly + build-std, wasm only).
web:
    RUSTUP_TOOLCHAIN={{nightly}} \
    CARGO_UNSTABLE_BUILD_STD="panic_abort,std" \
    trunk build --release

# Serve the threaded build locally with COOP/COEP isolation headers.
serve:
    RUSTUP_TOOLCHAIN={{nightly}} \
    CARGO_UNSTABLE_BUILD_STD="panic_abort,std" \
    trunk serve --release

# Fast type-check for the wasm target (nightly + build-std, no bundling).
web-check:
    RUSTUP_TOOLCHAIN={{nightly}} \
    CARGO_UNSTABLE_BUILD_STD="panic_abort,std" \
    cargo check --target wasm32-unknown-unknown
