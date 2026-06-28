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

# Remove build artifacts.
clean:
    cargo clean
