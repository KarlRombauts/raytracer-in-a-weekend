# Multithreaded WASM build (native + browser) — Design

Date: 2026-06-29
Status: Approved (pending spec review)

## Goal

Make the interactive raytracer build and run in the browser with real
multithreading, while keeping the native build's behavior unchanged. `rayon`
does not provide threads on `wasm32-unknown-unknown` by itself; the browser
needs a Web Worker pool backed by `SharedArrayBuffer` + WASM atomics, provided
by `wasm-bindgen-rayon`.

Scope is **local-first**: get a threaded wasm binary building and running in a
browser served locally (with the required isolation headers). Deployment/hosting
is explicitly deferred.

## Background — current state

- **Binary-only crate** (`src/main.rs`, no `lib.rs`). `main()` calls
  `viewer::run(cornell_box())`.
- **Parallelism** comes from two `rayon` sites:
  - `render.rs` — `ProgressiveRenderer::add_pass` uses `accum.par_iter_mut()`
    (one sample per pixel per pass; this is the path the interactive viewer
    uses).
  - `camera.rs` — `Camera::render` uses `par_enumerate_pixels_mut` (the CLI
    batch render-to-file path; not used by the viewer).
- **Threading model** (`viewer/render_task.rs`): a persistent
  `std::thread::spawn` background thread runs an infinite loop — watches a
  generation counter, rebuilds the world on scene changes, renders progressive
  passes into an `Arc<Mutex<SharedFrame>>`, and wakes the UI via
  `ctx.request_repaint()`. Supports cancel-and-restart, pause (Edit mode), and a
  resolution `preview_scale`.
- **eframe/egui viewer** with a glow (OpenGL) rasterized Edit-mode preview and a
  transform gizmo.

## WASM-hostile inventory

1. `.cargo/config.toml` forces `target-cpu=native` globally — breaks any wasm
   build.
2. Binary-only crate — `wasm-bindgen` needs a `cdylib` lib target.
3. `std::thread::spawn` persistent render thread — `std` cannot spawn threads on
   `wasm32-unknown-unknown`.
4. `std::time::Instant` (`render_task.rs`, `camera.rs`) — panics on wasm.
5. `img.save(path)` / `ProgressiveRenderer::save_png` to disk + auto-save on
   render completion — no filesystem in the browser.
6. Synchronous `rfd::FileDialog` + `std::fs::read` (OBJ import
   `controls.rs:184`, texture picker `controls.rs:485`) — sync `rfd` and
   `std::fs` do not exist on wasm.
7. eframe canvas bootstrap (index.html, build tool) + COOP/COEP isolation
   headers for `SharedArrayBuffer`.

## Architecture — chosen approach

**Per-platform render driver over a shared render core.** Native keeps its
background thread untouched; wasm pumps one pass per animation frame. The
pixel-level parallelism is shared via `rayon` / `wasm-bindgen-rayon`.

(Approaches considered and rejected: a real worker thread on wasm via
`wasm_thread` keeping the loop architecture identical — more moving parts,
awkward worker→main repaint marshaling, a second threading dependency on top of
`wasm-bindgen-rayon`; and dropping the background thread on both and pumping
per-frame everywhere — a behavioral regression on native, since a heavy pass
would stutter the UI and the clean cancel/restart model would be lost.)

## Components

### 1. Crate restructure — lib + bin

- Add `src/lib.rs` declaring all existing modules and exposing a native API
  (`pub use` of `scenes`, `viewer::run`, etc.).
- `main.rs` becomes a thin shim into the lib; native behavior unchanged.
- Wasm entry point in `lib.rs`:
  `#[cfg(target_arch = "wasm32")] #[wasm_bindgen] pub async fn start(...)` —
  installs `console_error_panic_hook`, boots eframe's `WebRunner` onto a canvas.
- `Cargo.toml`:
  ```toml
  [lib]
  crate-type = ["cdylib", "rlib"]
  ```

### 2. Shared render core + two drivers

Extract the orchestration currently inside the `std::thread::spawn` closure into
a **`RenderEngine`** struct owning the `ProgressiveRenderer`, built world,
camera, target sample count, and the `Arc<Mutex<SharedFrame>>`. API:

- `restart_if_invalidated()` — when the generation counter changed, rebuild
  world + camera from the scene snapshot (respecting `preview_scale` and
  `paused`), reset the progressive renderer.
- `step() -> bool` — add one pass, publish the frame into `SharedFrame`, return
  whether more passes remain (and whether the render was cancelled mid-flight,
  matching today's post-publish cancel check).

Two thin drivers keep the existing `RenderTask` **public API** (`spawn`,
`invalidate`, `pause`, `resume`, `set_preview_scale`, `preview_scale`, `lock`)
so `viewer/mod.rs` changes minimally:

- **Native:** the existing background thread loops over
  `restart_if_invalidated()` / `step()`. Behavior identical to today, including
  the "render first pass before publishing new dimensions" anti-flash detail.
- **Wasm:** no thread. `pump(&ctx)` advances at most one `step()` per call and
  requests a repaint while work remains.

`viewer/mod.rs` calls `self.render.pump(ctx)` once per `update()` — a no-op on
native, the driver on wasm. The `accum.par_iter_mut()` in `add_pass` is
unchanged and runs across the `wasm-bindgen-rayon` worker pool on wasm / the
native rayon pool on native. **This is the source of multithreading on both
targets.**

`SharedFrame` stays behind `Arc<Mutex<…>>` on both targets (atomics-backed
`Mutex` works under `wasm-bindgen-rayon`).

### 3. Dependencies & target gating (`Cargo.toml`)

- **Shared:** `image`, `rand`, `glam`, `bytemuck`, `palette`, `typed-builder`,
  `rand_distr`, `rayon`, `web-time`.
- **Native-only** (`[target.'cfg(not(target_arch="wasm32"))'.dependencies]`):
  `indicatif`, `rfd`, `eframe` with `glow`/`default_fonts`/`accesskit`/`x11`/
  `wayland`.
- **Wasm-only** (`[target.'cfg(target_arch="wasm32")'.dependencies]`):
  `eframe` (`glow`/`default_fonts`/`accesskit`, no `x11`/`wayland`),
  `wasm-bindgen`, `wasm-bindgen-rayon`, `wasm-bindgen-futures`,
  `web-sys` (features: `Blob`, `BlobPropertyBag`, `Url`, `HtmlAnchorElement`,
  `HtmlCanvasElement`, `Document`, `Window`, …), `console_error_panic_hook`,
  `getrandom` with the `wasm_js` backend.
- **`web-time::Instant`** replaces every `std::time::Instant` (re-exports `std`
  on native).
- `Camera::render` (CLI batch path: `indicatif` + `img.save`) gated behind
  `#[cfg(not(target_arch = "wasm32"))]`.

### 4. Build config

- `.cargo/config.toml` — scope `target-cpu=native` to the host; add wasm target
  features:
  ```toml
  [target.'cfg(not(target_arch = "wasm32"))']
  rustflags = ["-C", "target-cpu=native"]

  [target.wasm32-unknown-unknown]
  rustflags = ["-C", "target-feature=+atomics,+bulk-memory,+mutable-globals",
               "--cfg", "getrandom_backend=\"wasm_js\""]
  ```
- Threaded wasm needs nightly + `build-std`. Kept **out of the shared config**
  so native stays on stable; lives only in a `just web` recipe via env
  (`RUSTUP_TOOLCHAIN=nightly`, `CARGO_UNSTABLE_BUILD_STD=panic_abort,std`)
  invoking **Trunk**.
- `Trunk.toml` sets isolation headers for local serving:
  ```toml
  [serve]
  headers = { "Cross-Origin-Opener-Policy"="same-origin", "Cross-Origin-Embedder-Policy"="require-corp" }
  ```
- `index.html` (Trunk entry): init the wasm module, call
  `initThreadPool(navigator.hardwareConcurrency)` from `wasm-bindgen-rayon`,
  then start the eframe app on the canvas.

> Implementation note: exact eframe 0.34 `WebRunner` signature and the
> `wasm-bindgen-rayon` + Trunk init ordering to be verified against current docs
> (Context7) during implementation, not assumed from memory.

### 5. File I/O — Save button, no auto-save, gated importers

- **Remove** the viewer auto-save (`render_task.rs:143`,
  `ProgressiveRenderer::save_png` call).
- Add a **Save Image** button in the side panel near the progress/status row. On
  click: encode the current accumulation buffer to PNG bytes in memory (new
  `ProgressiveRenderer::to_png_bytes()`), then call a cfg-split
  `platform::save_png(suggested_name, &bytes)`:
  - **Native:** `rfd::FileDialog::save_file()` → `std::fs::write`.
  - **Wasm:** `web-sys` `Blob` + temporary `<a download>` element click.
- **OBJ import** and **texture picker** gated out on wasm — buttons render
  disabled with a tooltip ("file import not available in browser yet"). Native
  unchanged. Porting to `AsyncFileDialog` is future work.

## Testing / verification

- **Native regression:** `cargo build` and `cargo run` work unchanged; render +
  Edit mode + the new Save button all function; CLI `Camera::render` path still
  compiles and runs.
- **Wasm type-check:** `cargo check --target wasm32-unknown-unknown` passes
  (validates gating).
- **Wasm runtime:** `just web` → open `localhost`; confirm the app loads, passes
  accumulate progressively, render speed scales with cores (workers spawned =
  `navigator.hardwareConcurrency`), **Save Image** downloads a PNG, and COOP/COEP
  headers are served.

## Out of scope (deferred)

- Deployment / hosting (e.g. GitHub Pages COOP/COEP service-worker hack).
- Browser file **import** (OBJ, textures) — needs `AsyncFileDialog`.
- "Download on render completion" behavior (replaced by the explicit Save
  button).
