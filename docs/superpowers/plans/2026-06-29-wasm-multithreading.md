# Multithreaded WASM Build Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the interactive raytracer build and run in the browser with real multithreading (Web Worker pool via `wasm-bindgen-rayon`), while keeping the native build byte-for-byte behaviorally unchanged.

**Architecture:** A shared `RenderEngine` core owns the progressive renderer + scene snapshot and exposes a single `tick()` step. Native drives it from the existing background thread; wasm drives it by pumping one `tick()` per egui frame. Pixel parallelism comes from `rayon`'s `par_iter_mut`, which runs on the native rayon pool or the `wasm-bindgen-rayon` worker pool. All platform-divergent code (time, file save, file import, CLI batch render) is `cfg`-gated.

**Tech Stack:** Rust (edition 2024), eframe/egui 0.34 (glow), rayon, `wasm-bindgen-rayon`, `wasm-bindgen`, `web-sys`, `web-time`, Trunk, nightly toolchain (wasm only).

## Global Constraints

- **Native must stay on stable Rust.** Nightly + `build-std` is used ONLY for the wasm build, scoped to the `just web` recipe via environment variables. Never put `[unstable] build-std` in the shared `.cargo/config.toml` (it errors on stable) and never add a project-wide `rust-toolchain.toml`.
- **Native behavior is unchanged.** After every task, `cargo build`, `cargo test`, and `cargo run` must work exactly as before. The native render thread, cancel/restart, pause, and preview-scale semantics are preserved.
- **eframe version stays `0.34`** (gizmo-crate compatibility — see memory `eframe-035-api-notes`). The `eframe::App for ViewerApp` impl (which uses `fn ui`, not `update`) is NOT modified.
- **Public `RenderTask` API is preserved:** `spawn`, `invalidate`, `pause`, `resume`, `set_preview_scale`, `preview_scale`, `lock` keep their existing signatures. A new `pump(&self)` method is added (no-op on native).
- **Target gating idiom:** native-only = `#[cfg(not(target_arch = "wasm32"))]`, wasm-only = `#[cfg(target_arch = "wasm32")]`.
- **No auto-save.** The renderer never writes to disk on its own; saving is an explicit user action.

---

## File Structure

| File | Responsibility | Change |
|------|----------------|--------|
| `src/lib.rs` | Module tree + native `pub use` + wasm `WebHandle`/`start` entry | Create |
| `src/main.rs` | Native binary shim → `lib::run_default()` | Modify |
| `src/platform.rs` | `cfg`-split `save_png(name, bytes)` (native fs/rfd vs wasm Blob download) | Create |
| `src/render.rs` | `ProgressiveRenderer` + new `to_png_bytes()`; drop disk-save responsibility | Modify |
| `src/viewer/render_task.rs` | `SharedFrame` + `RenderEngine` core + `RenderTask` (native thread / wasm pump) | Modify |
| `src/viewer/mod.rs` | Call `render.pump()` each frame; add Save Image button | Modify |
| `src/viewer/controls.rs` | Gate OBJ import + texture picker out on wasm | Modify |
| `src/camera/camera.rs` | `web_time::Instant`; gate CLI `render()` to native | Modify |
| `Cargo.toml` | `[lib]` target; per-target dependency split | Modify |
| `.cargo/config.toml` | Host-scoped `target-cpu=native`; wasm target features + link-args | Modify |
| `Trunk.toml` | COOP/COEP serve headers; disable wasm-opt | Create |
| `index.html` | Trunk entry: load module, init thread pool, start app on canvas | Create |
| `justfile` | `web` (nightly+build-std trunk build) + `serve` + `web-check` recipes | Modify |

---

## Task 1: Split crate into lib + bin

**Files:**
- Create: `src/lib.rs`
- Modify: `src/main.rs`
- Modify: `Cargo.toml` (add `[lib]`)

**Interfaces:**
- Produces: `raytracer_in_a_weekend::run_default()` (native entry that opens the viewer on the Cornell box). Module paths (`crate::camera`, `crate::render`, `crate::scene`, …) become library-internal and are unchanged for all other files.

- [ ] **Step 1: Move the module tree into `src/lib.rs`**

Create `src/lib.rs` with the module declarations currently in `main.rs`, plus a native run helper:

```rust
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
```

> Note: `platform` is declared here but its file is created in Task 4. Until then, comment out the `pub mod platform;` line OR create an empty `src/platform.rs`. Create an empty `src/platform.rs` now so the tree compiles:
>
> ```rust
> // Platform-specific helpers. Implemented in Task 4.
> ```

- [ ] **Step 2: Reduce `src/main.rs` to a shim**

Replace the entire contents of `src/main.rs` with:

```rust
fn main() {
    raytracer_in_a_weekend::run_default();
}
```

(The crate name with hyphens becomes `raytracer_in_a_weekend` as an identifier. The previously-unused `new_bvh` reference is dropped; it is still reachable via `scenes` for callers that want it.)

- [ ] **Step 3: Add the `[lib]` target to `Cargo.toml`**

Insert directly after the `[package]` block:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

- [ ] **Step 4: Build and test — native regression**

Run: `cargo build`
Expected: compiles with no errors (warnings about unused `new_bvh` are acceptable).

Run: `cargo test`
Expected: all existing tests pass (or "0 tests" if none) — no failures.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/main.rs src/platform.rs Cargo.toml
git commit -m "refactor: split into lib + bin for wasm target"
```

---

## Task 2: Cross-platform time via `web-time`

**Files:**
- Modify: `Cargo.toml` (add `web-time`)
- Modify: `src/camera/camera.rs:4` (import)
- Modify: `src/viewer/render_task.rs:3` (import)

**Interfaces:**
- Produces: every `Instant` in the crate resolves to `web_time::Instant`, which is `std::time::Instant` on native and a browser-clock shim on wasm.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml` under `[dependencies]`:

```toml
web-time = "1.1"
```

- [ ] **Step 2: Swap the imports**

In `src/camera/camera.rs`, change:

```rust
use std::time::Instant;
```
to:
```rust
use web_time::Instant;
```

In `src/viewer/render_task.rs`, change:

```rust
use std::time::{Duration, Instant};
```
to:
```rust
use std::time::Duration;
use web_time::Instant;
```

(`Duration` stays from `std`; `web_time` re-exports the same type so either works, but keep the diff minimal.)

- [ ] **Step 3: Build — native regression**

Run: `cargo build`
Expected: compiles, no errors.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/camera/camera.rs src/viewer/render_task.rs
git commit -m "refactor: use web-time::Instant for wasm compatibility"
```

---

## Task 3: Extract `RenderEngine` core + per-platform drivers

**Files:**
- Modify: `src/viewer/render_task.rs` (full rewrite of the orchestration; `SharedFrame` unchanged)
- Modify: `src/viewer/mod.rs` (add one `self.render.pump(&ctx);` call)
- Test: `src/viewer/render_task.rs` (inline `#[cfg(test)]` module)

**Interfaces:**
- Consumes: `crate::scene::{build_world, Scene}`, `crate::camera::Camera`, `crate::render::ProgressiveRenderer`.
- Produces:
  - `RenderEngine::new(ctx, scene, shared, generation, preview_scale, paused) -> RenderEngine`
  - `RenderEngine::tick(&mut self) -> bool` — advances at most one pass; returns `true` if it did work this call (more work may remain), `false` if idle (paused / waiting for an edit / already finished).
  - `RenderTask` keeps `spawn/invalidate/pause/resume/set_preview_scale/preview_scale/lock` unchanged, and gains `pub fn pump(&self)` (no-op on native, drives `tick` on wasm).

- [ ] **Step 1: Write the failing test for `RenderEngine`**

Add to the bottom of `src/viewer/render_task.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::cornell_box;

    /// A fresh engine, once invalidated, accumulates one pass per tick until it
    /// reaches the scene's target sample count, then goes idle.
    #[test]
    fn engine_accumulates_passes_to_target() {
        let scene = cornell_box();
        // Keep the test fast: trace a tiny image with a few samples.
        let mut s = scene.clone();
        s.camera.image_width = 16;
        s.camera.samples = 3;
        let target = s.camera.samples;
        let scene = std::sync::Arc::new(std::sync::Mutex::new(s));

        let ctx = eframe::egui::Context::default();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(SharedFrame {
            rgba: vec![],
            width: 0,
            height: 0,
            passes: 0,
            total: target,
            done: false,
            elapsed: 0.0,
        }));
        let generation = std::sync::Arc::new(AtomicU64::new(0));
        let preview_scale = std::sync::Arc::new(AtomicU32::new(1));
        let paused = std::sync::Arc::new(AtomicBool::new(false));

        let mut engine = RenderEngine::new(
            ctx,
            scene,
            shared.clone(),
            generation.clone(),
            preview_scale,
            paused,
        );

        // First invalidation kicks off a render.
        generation.fetch_add(1, Ordering::Relaxed);

        // Tick until idle; bounded so a bug can't hang the test.
        let mut ticks = 0;
        while engine.tick() {
            ticks += 1;
            assert!(ticks < 100, "engine never went idle");
        }

        let frame = shared.lock().unwrap();
        assert_eq!(frame.passes, target, "should accumulate exactly `target` passes");
        assert!(frame.done, "should be marked done at full resolution");
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }
}
```

- [ ] **Step 2: Run the test — verify it fails**

Run: `cargo test engine_accumulates_passes_to_target`
Expected: FAIL to compile with "cannot find type `RenderEngine`" (it does not exist yet).

- [ ] **Step 3: Rewrite `src/viewer/render_task.rs` with the shared core + drivers**

Replace the file contents above the `#[cfg(test)]` module with:

```rust
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use web_time::Instant;

use eframe::egui;

use crate::camera::Camera;
use crate::group::IntersectGroup;
use crate::render::ProgressiveRenderer;
use crate::scene::{build_world, Scene};

/// Frame handed from the renderer to the UI. `width`/`height` always match the
/// dimensions of `rgba`, so the UI can resize its texture when the render
/// resolution changes.
pub struct SharedFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub passes: u32,
    pub total: u32,
    pub done: bool,
    pub elapsed: f64,
}

/// One in-flight render: the renderer plus the immutable inputs it was started
/// with. Recreated whenever the scene is invalidated.
struct ActiveRender {
    renderer: ProgressiveRenderer,
    camera: Camera,
    world: IntersectGroup,
    target: u32,
    scale: u32,
    start: Instant,
    /// Set once the target is reached and `done` has been published, so we stop
    /// re-publishing / re-waking the UI.
    finished: bool,
}

/// The platform-independent render core. `tick()` advances the render by at most
/// one pass; the native thread and the wasm per-frame pump both drive it.
pub struct RenderEngine {
    ctx: egui::Context,
    scene: Arc<Mutex<Scene>>,
    shared: Arc<Mutex<SharedFrame>>,
    generation: Arc<AtomicU64>,
    preview_scale: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
    /// Last generation we started a render for; `u64::MAX` means "never".
    last_gen: u64,
    active: Option<ActiveRender>,
}

impl RenderEngine {
    pub fn new(
        ctx: egui::Context,
        scene: Arc<Mutex<Scene>>,
        shared: Arc<Mutex<SharedFrame>>,
        generation: Arc<AtomicU64>,
        preview_scale: Arc<AtomicU32>,
        paused: Arc<AtomicBool>,
    ) -> Self {
        RenderEngine {
            ctx,
            scene,
            shared,
            generation,
            preview_scale,
            paused,
            last_gen: u64::MAX,
            active: None,
        }
    }

    /// Build a fresh render from the current scene snapshot, render its first
    /// pass, and publish it (dimensions + pixels) before returning.
    fn start_render(&mut self) {
        let snapshot = self.scene.lock().unwrap().clone();
        let world = build_world(&snapshot);
        let target = snapshot.camera.samples;

        // Reduced resolution while interacting so passes complete fast enough to
        // track the mouse; full resolution (scale 1) when idle.
        let scale = self.preview_scale.load(Ordering::Relaxed).max(1);
        let mut cam_cfg = snapshot.camera.clone();
        if scale > 1 {
            cam_cfg.image_width = (cam_cfg.image_width / scale).max(1);
        }
        let camera = Camera::from(cam_cfg);
        let (w, h) = (camera.image_width(), camera.image_height());

        let mut renderer = ProgressiveRenderer::new(w, h);
        let start = Instant::now();

        // Render the first pass BEFORE publishing the new dimensions so a
        // resolution change never flashes black: the UI keeps showing the
        // previous frame until this one is ready to replace it.
        renderer.add_pass(&camera, &world);
        {
            let mut s = self.shared.lock().unwrap();
            s.width = w;
            s.height = h;
            s.rgba = renderer.to_rgba();
            s.passes = renderer.passes();
            s.total = target;
            s.done = false;
            s.elapsed = start.elapsed().as_secs_f64();
        }
        self.ctx.request_repaint();

        self.active = Some(ActiveRender {
            renderer,
            camera,
            world,
            target,
            scale,
            start,
            finished: false,
        });
    }

    /// Advance the render by at most one pass. Returns `true` if work was done
    /// (call again soon), `false` if idle (paused, waiting for an edit, or the
    /// current render has finished).
    pub fn tick(&mut self) -> bool {
        // Idle while paused (Edit mode); drop any in-flight render.
        if self.paused.load(Ordering::Relaxed) {
            self.active = None;
            return false;
        }

        // A newer edit (or the very first render) restarts from a fresh world.
        let current_gen = self.generation.load(Ordering::Relaxed);
        if current_gen != self.last_gen {
            self.last_gen = current_gen;
            self.start_render();
            return true;
        }

        let Some(a) = self.active.as_mut() else {
            return false; // nothing to do until the next invalidate
        };

        if a.renderer.passes() >= a.target {
            // Reached the target: mark done once (full-resolution only — reduced
            // previews are throwaway and snap back via a later invalidate).
            if !a.finished {
                a.finished = true;
                if a.scale == 1 {
                    self.shared.lock().unwrap().done = true;
                    self.ctx.request_repaint();
                }
            }
            return false;
        }

        // Add one more pass and publish it.
        a.renderer.add_pass(&a.camera, &a.world);
        {
            let mut s = self.shared.lock().unwrap();
            s.rgba = a.renderer.to_rgba();
            s.passes = a.renderer.passes();
            s.elapsed = a.start.elapsed().as_secs_f64();
        }
        self.ctx.request_repaint();
        true
    }
}

/// Owns the render core plus the shared state it publishes into. On native a
/// background thread drives the engine; on wasm the UI pumps it once per frame.
pub struct RenderTask {
    shared: Arc<Mutex<SharedFrame>>,
    generation: Arc<AtomicU64>,
    /// Resolution divisor: 1 = full quality, >1 = reduced preview while the user
    /// is interacting.
    preview_scale: Arc<AtomicU32>,
    /// While true the renderer idles instead of tracing (Edit mode).
    paused: Arc<AtomicBool>,
    /// wasm-only: the engine lives on the main thread and is pumped each frame.
    #[cfg(target_arch = "wasm32")]
    engine: std::cell::RefCell<RenderEngine>,
}

impl RenderTask {
    /// Create the shared buffer + control atomics and start driving the engine.
    /// On native this spawns the long-lived render thread (the window must stay
    /// on the main thread). On wasm the engine is stored for per-frame pumping.
    pub fn spawn(
        ctx: egui::Context,
        scene: Arc<Mutex<Scene>>,
        width: u32,
        height: u32,
        initial_total: u32,
    ) -> Self {
        let shared = Arc::new(Mutex::new(SharedFrame {
            rgba: vec![0u8; (width * height * 4) as usize],
            width,
            height,
            passes: 0,
            total: initial_total,
            done: false,
            elapsed: 0.0,
        }));
        let generation = Arc::new(AtomicU64::new(0));
        let preview_scale = Arc::new(AtomicU32::new(1));
        let paused = Arc::new(AtomicBool::new(false));

        let engine = RenderEngine::new(
            ctx,
            scene,
            shared.clone(),
            generation.clone(),
            preview_scale.clone(),
            paused.clone(),
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut engine = engine;
            std::thread::spawn(move || loop {
                // `tick` returns false when idle (paused, no pending edit, or
                // finished); sleep then so we neither busy-spin nor trace.
                if !engine.tick() {
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            RenderTask {
                shared,
                generation,
                preview_scale,
                paused,
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            RenderTask {
                shared,
                generation,
                preview_scale,
                paused,
                engine: std::cell::RefCell::new(engine),
            }
        }
    }

    /// Drive the engine forward. No-op on native (the thread does it); on wasm
    /// this advances one pass per call and is invoked once per egui frame.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn pump(&self) {}

    #[cfg(target_arch = "wasm32")]
    pub fn pump(&self) {
        // `tick` self-requests a repaint when it publishes, which schedules the
        // next frame (and therefore the next pump) until the render goes idle.
        let _ = self.engine.borrow_mut().tick();
    }

    /// Signal that the scene changed: cancels the in-flight render and restarts.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Stop tracing (Edit mode).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    /// Resume tracing and restart from the current scene (Render mode).
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.invalidate();
    }

    /// Set the resolution divisor for subsequent renders (1 = full quality).
    pub fn set_preview_scale(&self, scale: u32) {
        self.preview_scale.store(scale.max(1), Ordering::Relaxed);
    }

    /// Current resolution divisor.
    pub fn preview_scale(&self) -> u32 {
        self.preview_scale.load(Ordering::Relaxed)
    }

    /// Lock and read the current shared frame.
    pub fn lock(&self) -> MutexGuard<'_, SharedFrame> {
        self.shared.lock().unwrap()
    }
}
```

> Behavioral note vs. the old code: the old loop saved `test.png` and set `done`
> only at `scale == 1`. Saving is removed here (Task 4 adds the Save button);
> `done` is still set only at full resolution, matching the old UI.

- [ ] **Step 4: Run the test — verify it passes**

Run: `cargo test engine_accumulates_passes_to_target`
Expected: PASS.

- [ ] **Step 5: Add the per-frame pump call in the viewer**

In `src/viewer/mod.rs`, inside `fn ui`, immediately after `let ctx = ui.ctx().clone();` add:

```rust
        // On wasm this advances the path trace one pass per frame (no render
        // thread in the browser); on native it is a no-op.
        self.render.pump();
```

- [ ] **Step 6: Build, test, and smoke-run native**

Run: `cargo build`
Expected: compiles, no errors.

Run: `cargo test`
Expected: all pass.

Run: `cargo run` (then close the window after confirming the Cornell box renders and refines, Edit mode pauses, Reset camera works)
Expected: identical behavior to before this task — progressive refinement, no `test.png` written on completion (auto-save intentionally removed; Save button comes in Task 4).

- [ ] **Step 7: Commit**

```bash
git add src/viewer/render_task.rs src/viewer/mod.rs
git commit -m "refactor: extract RenderEngine core with native-thread/wasm-pump drivers"
```

---

## Task 4: `to_png_bytes` + platform save + Save Image button

**Files:**
- Modify: `src/render.rs` (add `to_png_bytes`; remove `save_png`)
- Modify: `src/platform.rs` (native `save_png`)
- Modify: `src/viewer/mod.rs` (Save Image button)
- Modify: `src/camera/camera.rs` (CLI `render` no longer relevant to viewer save; left intact here, gated in Task 5)
- Test: `src/render.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `ProgressiveRenderer` accumulation buffer.
- Produces:
  - `ProgressiveRenderer::to_png_bytes(&self) -> Vec<u8>` — gamma-corrected PNG-encoded bytes of the current image.
  - `crate::platform::save_png(suggested_name: &str, bytes: &[u8])` — native: rfd save dialog → `std::fs::write`; wasm impl added in Task 6.

- [ ] **Step 1: Write the failing test for `to_png_bytes`**

Add to the bottom of `src/render.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_png_bytes_round_trips_dimensions() {
        let mut r = ProgressiveRenderer::new(8, 4);
        // No passes needed; encoding the (black) buffer must still be valid PNG.
        let bytes = r.to_png_bytes();
        // PNG magic number.
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // Decodes back to the original dimensions.
        let img = image::load_from_memory(&bytes).expect("valid PNG");
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 4);
        let _ = &mut r;
    }
}
```

- [ ] **Step 2: Run the test — verify it fails**

Run: `cargo test to_png_bytes_round_trips_dimensions`
Expected: FAIL to compile with "no method named `to_png_bytes`".

- [ ] **Step 3: Implement `to_png_bytes` and remove `save_png`**

In `src/render.rs`, replace the `save_png` method with `to_png_bytes`:

```rust
    /// Current image as PNG-encoded bytes (gamma-corrected, opaque RGB).
    pub fn to_png_bytes(&self) -> Vec<u8> {
        let scale = self.sample_scale();
        let mut img = image::RgbImage::new(self.width, self.height);
        for (idx, c) in self.accum.iter().enumerate() {
            let x = idx as u32 % self.width;
            let y = idx as u32 / self.width;
            img.put_pixel(x, y, image::Rgb((*c * scale).to_rgb_vec()));
        }
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("PNG encode");
        bytes
    }
```

Also update the doc comment on `to_rgba` (line ~11) that mentions `save_png` so it reads `to_rgba`/`to_png_bytes` instead — no functional change, just keep the comment honest.

- [ ] **Step 4: Run the test — verify it passes**

Run: `cargo test to_png_bytes_round_trips_dimensions`
Expected: PASS.

- [ ] **Step 5: Implement native `platform::save_png`**

Replace the contents of `src/platform.rs` with:

```rust
//! Platform-specific helpers that differ between native and the browser.

/// Save PNG `bytes` to a user-chosen location.
///
/// Native: opens a save dialog, writes the file. Wasm: triggers a browser
/// download (implemented in the wasm cfg block).
#[cfg(not(target_arch = "wasm32"))]
pub fn save_png(suggested_name: &str, bytes: &[u8]) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .set_file_name(suggested_name)
        .save_file()
    {
        if let Err(e) = std::fs::write(&path, bytes) {
            eprintln!("failed to save {}: {e}", path.display());
        }
    }
}
```

(The wasm implementation is added in Task 6, once the wasm deps exist.)

- [ ] **Step 6: Add the Save Image button to the side panel**

In `src/viewer/mod.rs`, find the status row block:

```rust
                    ui.horizontal(|ui| {
                        if done {
                            ui.label("done — saved test.png");
                        } else {
                            ui.spinner();
                            ui.label("rendering…");
                        }
                    });
```

Replace it with:

```rust
                    ui.horizontal(|ui| {
                        if done {
                            ui.label("done");
                        } else {
                            ui.spinner();
                            ui.label("rendering…");
                        }
                        if ui.button(format!("{}  Save image", icons::FLOPPY)).clicked() {
                            let bytes = {
                                // Re-encode the current shown frame from the
                                // shared RGBA buffer (already gamma-corrected).
                                let s = self.render.lock();
                                encode_rgba_png(&s.rgba, s.width, s.height)
                            };
                            crate::platform::save_png("render.png", &bytes);
                        }
                    });
```

> The `icons::FLOPPY` glyph: check `src/viewer/icons.rs` for an existing
> floppy/save glyph constant. If none exists, add one following the existing
> pattern in that file (the icons are Phosphor codepoints), e.g.
> `pub const FLOPPY: &str = "\u{...}";`. If you prefer not to add an icon, use a
> plain `ui.button("Save image")` instead.

Add this free function near the top of `src/viewer/mod.rs` (after the imports):

```rust
/// Encode an already-gamma-corrected RGBA buffer to PNG bytes (RGB, opaque).
fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut img = eframe::egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        rgba,
    );
    let _ = &mut img; // silence unused if width/height are zero
    let mut rgb = image::RgbImage::new(width, height);
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        let x = i as u32 % width.max(1);
        let y = i as u32 / width.max(1);
        rgb.put_pixel(x, y, image::Rgb([px[0], px[1], px[2]]));
    }
    let mut bytes = Vec::new();
    rgb.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("PNG encode");
    bytes
}
```

> Design choice: the button encodes from the **shown** `SharedFrame` RGBA so
> "what you see is what you save", including at preview resolution. (If you want
> it to always save full-res, that requires reaching into the engine's renderer
> and is out of scope — the shared frame is the displayed image.) The
> `ProgressiveRenderer::to_png_bytes` added above is the unit-tested encoder and
> is kept for any non-UI caller; the viewer uses the lighter `encode_rgba_png`
> over the published frame to avoid locking the engine. Remove `to_png_bytes`
> only if no other caller needs it — keep it; it is tested.

- [ ] **Step 7: Build, test, smoke-run native**

Run: `cargo build`
Expected: compiles. (If `eframe::egui::ColorImage` import in `encode_rgba_png` is unused, delete the first two lines of the function body — the `rgb` loop is the real encoder.)

Run: `cargo test`
Expected: all pass.

Run: `cargo run` — let one pass render, click **Save image**, confirm the save dialog appears and writes a valid PNG. Confirm NO `test.png` is auto-written on completion.

- [ ] **Step 8: Commit**

```bash
git add src/render.rs src/platform.rs src/viewer/mod.rs src/viewer/icons.rs
git commit -m "feat: explicit Save Image button; remove render auto-save"
```

---

## Task 5: Per-target dependency split, build config, and wasm gating

**Files:**
- Modify: `Cargo.toml` (split `[dependencies]` into shared / native / wasm)
- Modify: `.cargo/config.toml`
- Modify: `src/camera/camera.rs` (gate CLI `render` to native)
- Modify: `src/viewer/controls.rs` (gate OBJ import + texture picker out on wasm)

**Interfaces:**
- Produces: a `Cargo.toml` where `indicatif`/`rfd` and native eframe features are native-only, and the wasm toolchain crates exist for wasm. No new Rust symbols.

- [ ] **Step 1: Rewrite `Cargo.toml` dependencies into per-target tables**

Replace the entire `[dependencies]` block with:

```toml
[dependencies]
image = { version = "*", features = ["rayon"] }
rayon = "*"
rand = { version = "0.9.1", features = ["small_rng"] }
typed-builder = "0.21.0"
palette = "0.7.6"
rand_distr = "0.5.1"
glam = { version = "0.29", features = ["mint"] }
bytemuck = "1"
glow = "0.17"
transform-gizmo-egui = "0.9"
web-time = "1.1"

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
indicatif = "0.18.0"
rfd = "0.17.2"
eframe = { version = "0.34", default-features = false, features = ["glow", "default_fonts", "accesskit", "x11", "wayland"] }

[target.'cfg(target_arch = "wasm32")'.dependencies]
eframe = { version = "0.34", default-features = false, features = ["glow", "default_fonts", "accesskit"] }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
wasm-bindgen-rayon = "1.3"
console_error_panic_hook = "0.1"
getrandom = { version = "0.3", features = ["wasm_js"] }
web-sys = { version = "0.3", features = ["Window", "Document", "HtmlCanvasElement", "HtmlAnchorElement", "Blob", "BlobPropertyBag", "Url"] }
```

> `wasm-bindgen-rayon` requires the `wasm-bindgen` major version to match the one
> Trunk's `wasm-bindgen-cli` uses. If the wasm build later errors with a
> "schema version mismatch", pin `wasm-bindgen` to the exact version
> `wasm-bindgen-rayon` depends on (check `cargo tree -p wasm-bindgen-rayon`).

- [ ] **Step 2: Rewrite `.cargo/config.toml`**

Replace the file with:

```toml
# target-cpu=native unlocks AVX etc. for the f32 math on the host. Scoped to
# non-wasm so it never leaks into the WebAssembly build.
[target.'cfg(not(target_arch = "wasm32"))']
rustflags = ["-C", "target-cpu=native"]

# Threaded WebAssembly: shared memory + atomics for the Web Worker rayon pool,
# and the wasm_js backend for getrandom. (build-std / nightly are supplied by
# the `just web` recipe so native stays on stable.)
[target.wasm32-unknown-unknown]
rustflags = [
  "-C", "target-feature=+atomics,+bulk-memory",
  "-C", "link-arg=--shared-memory",
  "-C", "link-arg=--max-memory=1073741824",
  "-C", "link-arg=--import-memory",
  "--cfg", "getrandom_backend=\"wasm_js\"",
]
```

- [ ] **Step 3: Gate the CLI `Camera::render` to native**

In `src/camera/camera.rs`, add the attribute directly above `pub fn render`:

```rust
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render(&self, world: &IntersectGroup) {
```

(This is the `indicatif` + `img.save("test.png")` batch path; it has no meaning in the browser.)

- [ ] **Step 4: Gate the OBJ import and texture picker out on wasm**

In `src/viewer/controls.rs`, the OBJ import button block (around line 180) and the texture `image_picker_row` (around line 484) both use synchronous `rfd::FileDialog` + `std::fs`. Wrap each file-dialog body so wasm shows a disabled control.

For the OBJ button (find the `if ui.button(... "OBJ").clicked() { if let Some(path) = rfd::FileDialog::new()...` block):

```rust
        #[cfg(not(target_arch = "wasm32"))]
        if ui
            .button(format!("{}  {}  OBJ", icons::PLUS, icons::POLYGON))
            .clicked()
        {
            // ... existing rfd::FileDialog pick_file body unchanged ...
        }
        #[cfg(target_arch = "wasm32")]
        ui.add_enabled(false, egui::Button::new(format!("{}  {}  OBJ", icons::PLUS, icons::POLYGON)))
            .on_disabled_hover_text("OBJ import isn't available in the browser yet");
```

For `image_picker_row`, gate the picker button similarly:

```rust
        #[cfg(not(target_arch = "wasm32"))]
        if ui.button("Choose\u{2026}").clicked() {
            // ... existing rfd::FileDialog + std::fs::read body unchanged ...
        }
        #[cfg(target_arch = "wasm32")]
        ui.add_enabled(false, egui::Button::new("Choose\u{2026}"))
            .on_disabled_hover_text("Image import isn't available in the browser yet");
```

> Keep the `changed` return value working: on wasm the disabled branch never sets
> `changed`, so the existing `let mut changed = false;` and `changed` return are
> correct unchanged.

- [ ] **Step 5: Build and test — native regression (the real gate for this task)**

Run: `cargo build`
Expected: compiles, no errors — native picks the `not(wasm32)` deps and code paths.

Run: `cargo test`
Expected: all pass.

Run: `cargo run` — confirm OBJ import + texture picker still work on native (they are unchanged on native).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .cargo/config.toml src/camera/camera.rs src/viewer/controls.rs
git commit -m "build: per-target deps + wasm gating for cli render and file import"
```

---

## Task 6: Wasm entry point, build tooling, and in-browser verification

**Files:**
- Modify: `src/lib.rs` (wasm `WebHandle`/`start`)
- Modify: `src/platform.rs` (wasm `save_png` via Blob download)
- Create: `index.html`
- Create: `Trunk.toml`
- Modify: `justfile` (web/serve/web-check recipes)

**Interfaces:**
- Consumes: `crate::viewer` app construction, `wasm_bindgen_rayon::init_thread_pool`.
- Produces: a `WebHandle` JS class with an async `start(canvas)` method; a working `just web` / `just serve`.

> This task's deliverable is "the threaded app runs in a browser". Several steps
> have real browser-integration variability (wasm-opt thread flags, thread-pool
> init ordering). Each includes the known-good config and an explicit
> verification + documented fallback.

- [ ] **Step 1: Add the wasm entry point to `src/lib.rs`**

Append to `src/lib.rs`:

```rust
#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// JS-facing handle. `new WebHandle()` then `await handle.start(canvas)`.
    #[wasm_bindgen]
    pub struct WebHandle {
        runner: eframe::WebRunner,
    }

    #[wasm_bindgen]
    impl WebHandle {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Self {
            console_error_panic_hook::set_once();
            Self {
                runner: eframe::WebRunner::new(),
            }
        }

        /// Initialize the rayon worker pool, then start the eframe app on the
        /// given canvas. Must be `await`ed from JS.
        #[wasm_bindgen]
        pub async fn start(
            &self,
            canvas: web_sys::HtmlCanvasElement,
        ) -> Result<(), JsValue> {
            // Spawn the Web Worker pool BEFORE any rayon `par_iter` runs.
            let threads = web_sys::window()
                .and_then(|w| w.navigator().hardware_concurrency().into())
                .map(|n: f64| n as usize)
                .filter(|n| *n > 0)
                .unwrap_or(4);
            wasm_bindgen_rayon::init_thread_pool(threads).await;

            let scene = crate::scenes::cornell_box();
            self.runner
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |cc| {
                        Ok(Box::new(crate::viewer::web_app(cc, scene)) as Box<dyn eframe::App>)
                    }),
                )
                .await
        }
    }
}
```

> `init_thread_pool` returns a `Promise`; `.await` it. `hardware_concurrency()`
> returns `f64`; the `.into()`/`filter` guards against `0`.

- [ ] **Step 2: Expose a wasm-friendly app constructor in the viewer**

`viewer::run` (native) calls `eframe::run_native`. The wasm path needs to build
the same `ViewerApp` from a `CreationContext`. In `src/viewer/mod.rs`, the
`ViewerApp::new(cc, scene, width, height)` constructor already exists but is
private. Add a public wrapper:

```rust
/// Build the viewer app for the web runner (mirrors `run` minus the native
/// window setup). Public so `lib::web` can construct it.
#[cfg(target_arch = "wasm32")]
pub fn web_app(cc: &eframe::CreationContext<'_>, scene: Scene) -> ViewerApp {
    let camera = Camera::from(scene.camera.clone());
    let width = camera.image_width();
    let height = camera.image_height();
    ViewerApp::new(cc, scene, width, height)
}
```

> `ViewerApp::new` already pulls the glow context via `cc.gl` — the web runner
> provides it (eframe glow backend = WebGL on wasm). No change needed there.

- [ ] **Step 3: Implement wasm `platform::save_png` (Blob download)**

Append to `src/platform.rs`:

```rust
#[cfg(target_arch = "wasm32")]
pub fn save_png(suggested_name: &str, bytes: &[u8]) {
    use wasm_bindgen::{JsCast, JsValue};

    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let mut opts = web_sys::BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)
        .expect("create blob");
    let url = web_sys::Url::create_object_url_with_blob(&blob).expect("object url");

    let document = web_sys::window().unwrap().document().unwrap();
    let anchor: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .unwrap()
        .dyn_into()
        .unwrap();
    anchor.set_href(&url);
    anchor.set_download(suggested_name);
    anchor.click();
    web_sys::Url::revoke_object_url(&url).ok();
    let _ = JsValue::NULL;
}
```

> Add `js-sys = "0.3"` to the wasm-only dependency table in `Cargo.toml` (it
> ships with `wasm-bindgen`; declare it explicitly for `js_sys::Uint8Array`).
> If `BlobPropertyBag::new()`/`set_type` API differs in the resolved `web-sys`
> version, the builder may instead need the older `.type_("image/png")` setter —
> adjust to whichever the compiler accepts.

- [ ] **Step 4: Create `index.html` (Trunk entry)**

Create `index.html` at the repo root:

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Raytracer</title>
    <link data-trunk rel="rust" data-wasm-opt="0" data-bindgen-target="web" />
    <style>
      html, body { margin: 0; height: 100%; background: #1e1e1e; }
      #the_canvas_id { width: 100%; height: 100%; display: block; }
    </style>
  </head>
  <body>
    <canvas id="the_canvas_id"></canvas>
    <script type="module">
      import init, { WebHandle } from '/raytracer-in-a-weekend.js';
      async function main() {
        await init();
        const handle = new WebHandle();
        await handle.start(document.getElementById('the_canvas_id'));
      }
      main();
    </script>
  </body>
</html>
```

> `data-wasm-opt="0"` disables wasm-opt for the local-first build — wasm-opt
> strips thread/TLS exports unless told to keep them, and disabling it avoids
> that whole class of failure. The JS module path
> `/raytracer-in-a-weekend.js` is Trunk's default output name (the crate name).
> If Trunk emits a different name, copy it from the `trunk build` output.
> `init_thread_pool` is called from inside Rust `start`, so no separate
> `initThreadPool` import is needed here.

- [ ] **Step 5: Create `Trunk.toml`**

Create `Trunk.toml` at the repo root:

```toml
[build]
target = "index.html"

[serve]
# SharedArrayBuffer requires cross-origin isolation.
headers = { "Cross-Origin-Opener-Policy" = "same-origin", "Cross-Origin-Embedder-Policy" = "require-corp" }
```

- [ ] **Step 6: Add justfile recipes**

Append to `justfile`:

```just
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
```

- [ ] **Step 7: Install the toolchain prerequisites (one-time)**

Run:
```bash
rustup toolchain install nightly --component rust-src
rustup target add wasm32-unknown-unknown --toolchain nightly
cargo install trunk
```
Expected: nightly toolchain with `rust-src`, the wasm target, and `trunk` available.

- [ ] **Step 8: Wasm type-check**

Run: `just web-check`
Expected: `cargo check` for `wasm32-unknown-unknown` completes with no errors. Fix any wasm-only compile errors surfaced here (missing `web-sys` feature flags, `js-sys` import, `web-sys` Blob builder API differences) before moving on.

- [ ] **Step 9: Build the bundle**

Run: `just web`
Expected: Trunk compiles the wasm, runs `wasm-bindgen`, emits `dist/`. Note the actual emitted `.js` filename in the output; if it differs from `/raytracer-in-a-weekend.js`, update the import in `index.html` and rebuild.

- [ ] **Step 10: In-browser verification (the deliverable)**

Run: `just serve`, then open the printed `http://localhost:8080` in a browser.

Verify, in order:
1. **App loads** — the Cornell box viewer appears on the canvas (check the devtools console for panics; `console_error_panic_hook` prints Rust panics there).
2. **Cross-origin isolation** — in the console, `crossOriginIsolated` evaluates to `true`. (If `false`, the COOP/COEP headers aren't applied — confirm `Trunk.toml [serve] headers`.)
3. **Threads spawned** — in devtools, the Sources/Threads panel (or `performance`/Workers) shows ~`navigator.hardwareConcurrency` Web Workers. The render visibly refines pass-by-pass.
4. **Parallelism works** — no console error like "rayon: thread pool not initialized". If you see one, the fallback is to call `initThreadPool` from JS instead: change `index.html` to `import init, { WebHandle, initThreadPool } from ...`, `await init(); await initThreadPool(navigator.hardwareConcurrency);` BEFORE `new WebHandle().start(...)`, and remove the `init_thread_pool(...).await` line from `src/lib.rs::start`.
5. **Save image** — click the button; the browser downloads `render.png`; open it and confirm it matches the on-screen image.
6. **Edit mode** — switching to Edit shows the GL preview. (Known risk: the WebGL canvas may lack a depth buffer, which the Edit-mode preview uses. If the preview renders without depth testing, that's a separate follow-up — the threaded Render path, which is this plan's goal, is unaffected. Note it but do not block on it.)

- [ ] **Step 11: Confirm native still builds (final regression)**

Run: `cargo build && cargo test`
Expected: native compiles and all tests pass — the wasm additions are all `cfg`-gated.

- [ ] **Step 12: Commit**

```bash
git add src/lib.rs src/platform.rs src/viewer/mod.rs index.html Trunk.toml justfile Cargo.toml
git commit -m "feat: threaded wasm build via wasm-bindgen-rayon + trunk"
```

---

## Self-Review notes

- **Spec coverage:** lib/bin split (T1), `web-time` (T2), RenderEngine + drivers (T3), Save button + no auto-save (T4), per-target deps + `.cargo/config` + CLI/import gating (T5), wasm entry + Trunk + COOP/COEP + thread pool + in-browser verify (T6). All spec sections map to a task.
- **Out-of-scope items** (deployment/hosting, browser file *import*, Edit-mode web depth buffer) are explicitly deferred and flagged where they surface.
- **Known execution-time variability** is concentrated in Task 6 (wasm-opt, thread-pool init ordering, exact `web-sys` Blob API, Trunk output filename) with documented fallbacks rather than placeholders.
```
