# Scene Library (Home Screen) — Design

**Date:** 2026-06-29
**Branch:** `ui-redesign-lumi`
**Status:** Approved

## Goal

Add a "Welcome back" home screen (the **library**) that lists bundled sample
scenes as clickable cards, plus **New scene** and **Open .scene file…** actions.
The app opens on this screen; picking a card (or New scene) enters the editor.
The top-bar scene chip becomes a real button that returns to the library.

This matches `ui-design-mockup.html` (the "Welcome back" screen and the top-bar
`cornell-box.scene` chip).

## Non-goals

- **Recent scenes / persistence.** The grid shows bundled samples only. No
  recents list, no localStorage/config tracking.
- **Search.** Omitted (a handful of samples doesn't need it). No dead stub.
- **On-the-fly thumbnail rendering.** Thumbnails are pre-baked PNGs.

## Architecture

### Screen routing

`UiState` gains:

```rust
pub enum Screen { Home, Editor }
```

with `screen: Screen` (default `Screen::Home`). `ViewerApp::ui` branches at the
top level: in `Home` it renders only the library screen (full window, no editor
panels); in `Editor` it renders the existing panels unchanged.

`run_default` (and `web_app`) construct the app on `Screen::Home`. The initial
`Scene` passed to `ViewerApp::new` stays as today (cornell-box) so the render
engine has something valid to hold; it simply isn't shown until the user enters
the editor by choosing a sample or New scene.

### Sample-scene registry — `src/viewer/samples.rs`

```rust
pub struct Sample {
    pub name: &'static str,        // "Cornell Box"
    pub build: fn() -> Scene,      // scenes::cornell_box::cornell_box
    pub thumbnail: &'static [u8],  // include_bytes!(".../cornell-box.png")
}

pub static SAMPLES: &[Sample] = &[
    Sample {
        name: "Cornell Box",
        build: crate::scenes::cornell_box::cornell_box,
        thumbnail: include_bytes!("../../assets/thumbnails/cornell-box.png"),
    },
];

/// Minimal starting scene for "New scene": a camera + a neutral ground plane.
pub fn new_scene() -> Scene { /* ... */ }

/// kebab-case file slug for a sample name ("Cornell Box" -> "cornell-box").
pub fn slug(name: &str) -> String { /* ... */ }
```

Card metadata is **derived, not stored**: building a sample's `Scene` yields
`objects = scene.objects.len()` and `resolution = camera image_width ×
image_height`. The Home screen builds each sample once on first display and
caches `(name, objects, resolution, texture_handle)` so it isn't rebuilt every
frame.

### Home screen — `src/viewer/panels/home.rs`

`show_home(ui, &mut HomeState) -> HomeAction`, laid out per the mockup:

1. Header: aperture logo + "Lumi" wordmark + version badge.
2. "Welcome back" title + subtitle.
3. Action row: **New scene** (accent pill) and **Open .scene file…** (dark pill).
4. "Sample scenes" section header (uppercase, divider).
5. Responsive card grid (`egui` columns, ~278px min):
   - a leading dashed **New scene** card;
   - one **card per `SAMPLES` entry**: thumbnail image, name, `⬚ N objects`,
     `▦ W×H`.

`HomeState` (held by `ViewerApp`, lazily populated) caches the per-sample
`egui::TextureHandle` (decoded from the PNG bytes once) and the derived metadata.

```rust
pub enum HomeAction { None, NewScene, OpenSample(usize), OpenSceneFile }
```

### Top-bar chip → button

The current display-only label (`"cornell-box.scene"`) becomes a real button
(folder icon + **current scene name** + caret). On click it returns to the
library (`screen = Home`). The hardcoded string becomes `scene_name: String` in
`UiState`, set when a scene is opened:

- sample → its `name`,
- new → `"untitled"`,
- after Save/Load (existing merged path) → the file name.

`show_top_bar` returns a new `Action::GoHome` when the chip is clicked.

### Actions & wiring (`src/viewer/mod.rs`)

Extend the panel `Action` enum with `GoHome`. Home actions are handled where
`show_home` is called. The handlers:

- **OpenSample(i)** / **NewScene** — build the `Scene`, swap it in via the
  existing scene-load path merged from `scene-file-save-load` (which rebuilds the
  GL preview, resets the reset-camera target, and invalidates the render), set
  `scene_name`, set `screen = Editor`.
- **OpenSceneFile** — reuse the merged async `ScenePicker`; on success it already
  loads the scene; additionally set `screen = Editor` and `scene_name`.
- **GoHome** — set `screen = Home`.

A small helper `ViewerApp::load_scene(scene, name)` centralizes the swap + GL
rebuild + render invalidate + `scene_name`/`screen` updates, reused by the Home
handlers and the existing file-load path.

### "New scene"

`samples::new_scene()` returns a minimal `Scene`: a sensible default camera plus
a single neutral ground plane (a large quad), so the viewport isn't empty and the
user can immediately Add Object.

### Thumbnail generation — `just thumbnails`

A headless generator renders each registered sample to a pre-baked PNG:

- `main.rs` parses argv: with `--gen-thumbnails` it runs the generator instead of
  the viewer; otherwise `run_default()`.
- The generator iterates `SAMPLES`, builds each scene, renders at a small fixed
  size (320×200) and a modest sample count (e.g. 48), and writes
  `assets/thumbnails/<slug(name)>.png`. Reuses the existing offline render path
  (`ProgressiveRenderer` / the same engine used by the book scenes).
- `justfile` recipe:

  ```
  # Render pre-baked thumbnails for every library sample scene.
  thumbnails:
      cargo run --release -- --gen-thumbnails
  ```

To add a scene: write a `fn() -> Scene`, add a `Sample` entry, run
`just thumbnails`.

## Data flow

```
Home screen (screen == Home)
  ├─ New scene card/button  → HomeAction::NewScene  → load_scene(new_scene(), "untitled")
  ├─ sample card            → HomeAction::OpenSample(i) → load_scene((SAMPLES[i].build)(), name)
  └─ Open .scene file…      → HomeAction::OpenSceneFile → ScenePicker (async) → load_scene(...)
                                                                       ↓
                                                              screen = Editor
Editor (screen == Editor)
  └─ top-bar chip button    → Action::GoHome → screen = Home
```

## Testing

- `samples::slug` — unit tests ("Cornell Box" → "cornell-box", trims, lowercases,
  collapses spaces/punctuation to single hyphens).
- `samples::new_scene` — returns a scene with ≥1 object (the ground plane) and a
  valid camera (non-zero dimensions).
- `SAMPLES` integrity — every entry builds without panicking, has a non-empty
  thumbnail byte slice, and the thumbnail decodes as a valid PNG.
- Thumbnail generator — a unit test that the per-sample render produces a
  non-empty PNG at the requested dimensions (small sample count for speed), using
  the same function the CLI path calls.
- Home/top-bar interaction (click routing) is GUI-level and verified by the user
  running the app; no automated egui-interaction coverage (consistent with the
  rest of the viewer).

## Files

- **new** `src/viewer/samples.rs` — registry, `new_scene`, `slug`.
- **new** `src/viewer/panels/home.rs` — `show_home`, `HomeState`, `HomeAction`.
- **new** `assets/thumbnails/cornell-box.png` — generated.
- **new** thumbnail generator (in `samples.rs` or a small `src/bin`/lib fn called
  from `main.rs`).
- **edit** `src/viewer/state.rs` — `Screen`, `screen`, `scene_name`.
- **edit** `src/viewer/mod.rs` — screen branch, `load_scene`, Home wiring, module
  decl.
- **edit** `src/viewer/panels/top_bar.rs` — chip button + `Action::GoHome`, scene
  name from state.
- **edit** `src/viewer/panels/mod.rs` — export `home`.
- **edit** `src/main.rs` — `--gen-thumbnails` argv path.
- **edit** `justfile` — `thumbnails` recipe.
