# Editor UI Redesign (Lumi) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the egui viewer's editor screen to match `ui-design-mockup.html` (the "Lumi" design) and reorganize the UI into reusable theme/widget/panel modules, plus add object visibility and duplicate.

**Architecture:** A `theme` module owns palette + fonts + egui `Style`. A `widgets` toolkit provides reusable atoms (axis field, prop row, card, combo, tab bar, buttons). A `panels` module has one function per zone (top bar, outliner, viewport, inspector tabs). `ViewerApp` shrinks to orchestration over a new `UiState`. Panels are free functions `fn show(ui, &mut UiState, &mut Scene, …) -> Out` returning a dirty flag and/or a small action enum that `mod.rs` applies centrally.

**Tech Stack:** Rust, eframe/egui 0.34 (Glow backend), Phosphor + IBM Plex fonts, `transform-gizmo-egui`, `glow`.

## Global Constraints

- **egui API is 0.34** (`App::ui(&mut self, ui: &mut egui::Ui, _frame)` hands the root `Ui`; we lay panels inside it). Do not call `eframe::App::update`.
- **Everything must also build for `wasm32-unknown-unknown`.** Native-only features (`rfd` file dialogs, PNG save) stay behind `#[cfg(not(target_arch = "wasm32"))]` with a disabled+tooltip fallback under `#[cfg(target_arch = "wasm32")]`, matching the existing pattern in `controls.rs`.
- **App name is "Lumi".**
- **No new runtime dependencies** — fonts are vendored `.ttf` files in `assets/fonts/`.
- Side panels are fixed width: **left outliner 286px, right inspector 342px**. Center flexes. No fixed-size scaler.
- Palette/spacing values come from the mockup verbatim (see Task 3).
- `cargo fmt` after each task. Native check: `cargo build`. Wasm check: `just web-check`. Tests: `cargo test`.

---

## File Structure

```
src/viewer/
  mod.rs            // ViewerApp: state + orchestration (shrinks)
  theme.rs          // NEW palette consts, font install, apply_style
  icons.rs          // + new glyphs (eye, copy, folder, download, caret, play, reset)
  state.rs          // NEW UiState, Mode, Tab
  controls.rs       // shrinks to shared editing helpers reused by inspector tabs
  widgets/
    mod.rs          // re-exports
    axis_field.rs   // axis_field, axis_vec, scalar_field, int_field
    prop_row.rs     // prop_row, section_header
    card.rs         // card frame, overlay_frame
    combo.rs        // styled_combo
    tab_bar.rs      // pill_tabs, segmented (Render/Edit)
    buttons.rs      // icon_button, pill_button, toolbar_button
  panels/
    mod.rs          // re-exports + Action enum
    top_bar.rs      // show_top_bar
    outliner.rs     // show_outliner
    viewport.rs     // show_viewport (image/GL host, overlays, status dock)
    inspector/
      mod.rs        // show_inspector (tab bar + dispatch)
      object.rs     // object_tab
      camera.rs     // camera_tab
      output.rs     // output_tab
assets/fonts/
  IBMPlexSans-Medium.ttf      // NEW (vendored, OFL)
  IBMPlexSans-SemiBold.ttf    // NEW
  IBMPlexMono-Medium.ttf      // NEW
```

---

## Task 1: Object visibility (`hidden` flag)

**Files:**
- Modify: `src/scene.rs` (`ObjectSpec` struct ~244, `build_world` ~342, the `cornell_box` builder helpers at ~423 and ~433)
- Modify: `src/scenes/cornell_box.rs` (~9, ~66, ~75), `src/scenes/new_bvh.rs` (~22, ~33), `src/viewer/raster/pick.rs` (~73), `src/viewer/controls.rs` (`default_sphere` ~663, `default_box` ~677), `src/scene.rs` `from_obj` (~293)
- Modify: `src/viewer/raster/renderer.rs` (`rebuild`/draw loop — skip hidden)
- Test: `src/scene.rs` (`#[cfg(test)]` module — add if absent)

**Interfaces:**
- Produces: `ObjectSpec { …, pub hidden: bool }` (default `false` on every constructor). `build_world` skips `hidden` objects entirely (not drawn, not a light).

- [ ] **Step 1: Write the failing test**

Add to `src/scene.rs` (in or appended as a `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod visibility_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;

    fn emissive(name: &str) -> ObjectSpec {
        ObjectSpec {
            name: name.into(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 0.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 1.0, 0.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        }
    }

    #[test]
    fn hidden_object_is_excluded_from_world_and_lights() {
        let mut scene = Scene {
            camera: CameraConfig::default(),
            objects: vec![emissive("a"), emissive("b")],
        };
        let full = build_world(&scene);
        scene.objects[1].hidden = true;
        let partial = build_world(&scene);
        // One fewer light registered when an emitter is hidden.
        assert_eq!(full.lights.len(), 2);
        assert_eq!(partial.lights.len(), 1);
    }
}
```

(If `CameraConfig::default()` does not exist, build one with the same fields the existing scenes use — check `src/camera/config.rs` and mirror an existing construction.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test visibility_tests 2>&1 | tail -20`
Expected: FAIL — `ObjectSpec` has no field `hidden` (compile error).

- [ ] **Step 3: Add the field and the filter**

In `src/scene.rs`, add to `ObjectSpec`:

```rust
#[derive(Clone)]
pub struct ObjectSpec {
    pub name: String,
    pub shape: Shape,
    pub material: MaterialSpec,
    pub transform: Transform,
    /// When true the object is omitted from the rendered world and the GL
    /// preview (toggled by the outliner eye). Default false.
    pub hidden: bool,
}
```

In `build_world`, skip hidden objects at the top of the loop:

```rust
for obj in &scene.objects {
    if obj.hidden {
        continue;
    }
    let geom = obj.build();
    // …unchanged…
}
```

Add `hidden: false` to **every** `ObjectSpec { … }` literal. Find them all:

Run: `grep -rn "ObjectSpec {" src/ | grep -v "pub struct ObjectSpec"`

Expected hits to update (add `hidden: false` to each literal): `src/scene.rs` `from_obj` (~293), the two `cornell_box`-builder helpers in `scene.rs` (~423, ~433), `src/scenes/cornell_box.rs` (~9, ~66, ~75), `src/scenes/new_bvh.rs` (~22, ~33), `src/viewer/raster/pick.rs` (~73), `src/viewer/controls.rs` `default_sphere` (~663) and `default_box` (~677).

- [ ] **Step 4: Skip hidden objects in the GL preview**

In `src/viewer/raster/renderer.rs`, the rebuild/draw uses `scene.objects`. Two changes:
1. `paint` currently rebuilds when `self.built_count != scene.objects.len()`. Change the trigger to also rebuild when the visible set changes. Add a field `built_visible: usize` and compute `let visible = scene.objects.iter().filter(|o| !o.hidden).count();` then rebuild when `self.built_visible != visible` (keep the existing length check too).
2. Wherever `rebuild` iterates `scene.objects` to build draw meshes, filter: `for obj in scene.objects.iter().filter(|o| !o.hidden)`. Ensure the per-object index used for the `selected` outline still refers to the original `scene.objects` index (if the renderer maps draw-mesh order to selection, keep an index map: push the original index alongside each built mesh). If the renderer keys the outline by original index, preserve that mapping; do not let hidden objects shift the highlighted index.

Set `built_visible` in `rebuild`.

- [ ] **Step 5: Run tests + builds**

Run: `cargo test 2>&1 | tail -15`
Expected: PASS (including `hidden_object_is_excluded_from_world_and_lights`).
Run: `cargo build 2>&1 | tail -5`
Expected: builds clean.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: object visibility (hidden flag) excluded from world and preview"
```

---

## Task 2: Object duplicate helper

**Files:**
- Modify: `src/scene.rs` (add free function near `placeable_bounds`)
- Test: `src/scene.rs` tests

**Interfaces:**
- Produces: `pub fn duplicate_object(objects: &mut Vec<ObjectSpec>, i: usize) -> Option<usize>` — clones `objects[i]`, inserts the clone immediately after `i` with `" copy"` appended to its name, returns the new index (`i + 1`), or `None` if `i` is out of range.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `src/scene.rs`:

```rust
#[test]
fn duplicate_inserts_clone_after_with_suffixed_name() {
    let mut objs = vec![emissive("Light"), emissive("Box")];
    let new_i = super::duplicate_object(&mut objs, 0).unwrap();
    assert_eq!(new_i, 1);
    assert_eq!(objs.len(), 3);
    assert_eq!(objs[1].name, "Light copy");
    assert_eq!(objs[2].name, "Box"); // original order preserved after the insert
    assert!(super::duplicate_object(&mut objs, 99).is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test duplicate_inserts 2>&1 | tail -20`
Expected: FAIL — `duplicate_object` not found.

- [ ] **Step 3: Implement**

In `src/scene.rs`:

```rust
/// Duplicate the object at `i`: insert a clone right after it with " copy"
/// appended to the name. Returns the new object's index, or None if `i` is out
/// of range. Cheap — meshes share their `Arc` BVH.
pub fn duplicate_object(objects: &mut Vec<ObjectSpec>, i: usize) -> Option<usize> {
    let mut clone = objects.get(i)?.clone();
    clone.name = format!("{} copy", clone.name);
    objects.insert(i + 1, clone);
    Some(i + 1)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test duplicate_inserts 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: duplicate_object helper (clone inserted after, name suffixed)"
```

---

## Task 3: Theme module — palette, fonts, style

**Files:**
- Create: `src/viewer/theme.rs`
- Create (download): `assets/fonts/IBMPlexSans-Medium.ttf`, `IBMPlexSans-SemiBold.ttf`, `IBMPlexMono-Medium.ttf`
- Modify: `src/viewer/mod.rs` (declare `mod theme;`, call `theme::install` in `ViewerApp::new`)

**Interfaces:**
- Produces: color consts (see below) as `egui::Color32`; `pub fn install(ctx: &egui::Context)` which installs fonts (IBM Plex Sans proportional, IBM Plex Mono monospace, Phosphor fallback) **and** applies the style. Replaces the direct `icons::install` call.

- [ ] **Step 1: Vendor the fonts**

Download the OFL-licensed IBM Plex TTFs into `assets/fonts/`:

```bash
mkdir -p assets/fonts
base=https://raw.githubusercontent.com/IBM/plex/v6.4.0/packages
curl -fL "$base/plex-sans/fonts/complete/ttf/IBMPlexSans-Medium.ttf"   -o assets/fonts/IBMPlexSans-Medium.ttf
curl -fL "$base/plex-sans/fonts/complete/ttf/IBMPlexSans-SemiBold.ttf" -o assets/fonts/IBMPlexSans-SemiBold.ttf
curl -fL "$base/plex-mono/fonts/complete/ttf/IBMPlexMono-Medium.ttf"   -o assets/fonts/IBMPlexMono-Medium.ttf
ls -l assets/fonts/IBMPlex*.ttf
```

Expected: three non-empty `.ttf` files. If the `v6.4.0/packages/...` path 404s, try the legacy layout `https://raw.githubusercontent.com/IBM/plex/master/IBM-Plex-Sans/fonts/complete/ttf/IBMPlexSans-Medium.ttf` (and `IBM-Plex-Mono/...`). Verify each file is a real font: `file assets/fonts/IBMPlexSans-Medium.ttf` should report TrueType/OpenType.

- [ ] **Step 2: Write `theme.rs`**

Create `src/viewer/theme.rs`:

```rust
//! The Lumi visual language: palette, fonts, and egui style. Installed once at
//! startup. Colours and spacing are taken verbatim from `ui-design-mockup.html`.

use eframe::egui::{self, Color32, CornerRadius, Stroke};

// --- Palette (mockup hexes) ---
pub const BG_APP: Color32 = Color32::from_rgb(0x0d, 0x0e, 0x11);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x16, 0x17, 0x1b);
pub const BG_TOPBAR: Color32 = Color32::from_rgb(0x15, 0x16, 0x1a);
pub const BG_VIEWPORT: Color32 = Color32::from_rgb(0x08, 0x09, 0x0b);
pub const FIELD_BG: Color32 = Color32::from_rgb(0x10, 0x11, 0x16);

pub const BORDER: Color32 = Color32::from_rgb(0x23, 0x26, 0x2b);
pub const BORDER_FIELD: Color32 = Color32::from_rgb(0x2c, 0x2f, 0x36);
pub const BORDER_HOVER: Color32 = Color32::from_rgb(0x41, 0x47, 0x4e);

pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xda, 0xdf);
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xec);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x97);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x6b, 0x70, 0x79);

pub const ACCENT: Color32 = Color32::from_rgb(0x4d, 0x84, 0xe6);
pub const SELECTION: Color32 = Color32::from_rgb(0xef, 0x8a, 0x3c);

pub const AXIS_X: Color32 = Color32::from_rgb(0xc0, 0x59, 0x4f);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x5a, 0x9e, 0x5a);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x4f, 0x7f, 0xc0);

/// accent at alpha (0..=255) over the panel background — for soft fills/strokes.
pub const fn accent_soft() -> Color32 {
    Color32::from_rgba_premultiplied(0x4d, 0x84, 0xe6, 36)
}
pub const fn selection_soft() -> Color32 {
    Color32::from_rgba_premultiplied(0xef, 0x8a, 0x3c, 40)
}

/// Field height / single-line interact size used across the inspector (mockup
/// fields are 30px tall).
pub const FIELD_H: f32 = 30.0;

/// Install fonts + style. Call once with the egui context at startup.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    apply_style(ctx);
}

fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "plex_sans".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf")).into(),
    );
    fonts.font_data.insert(
        "plex_sans_semibold".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf")).into(),
    );
    fonts.font_data.insert(
        "plex_mono".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf")).into(),
    );
    fonts.font_data.insert(
        "phosphor".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/Phosphor.ttf")).into(),
    );

    // Proportional = Plex Sans, Monospace = Plex Mono, Phosphor as fallback in
    // both so the icon PUA glyphs still resolve.
    fonts.families.insert(
        FontFamily::Proportional,
        vec!["plex_sans".into(), "phosphor".into()],
    );
    fonts.families.insert(
        FontFamily::Monospace,
        vec!["plex_mono".into(), "phosphor".into()],
    );
    // SemiBold available as a named family for headings/wordmark.
    fonts.families.insert(
        FontFamily::Name("semibold".into()),
        vec!["plex_sans_semibold".into(), "phosphor".into()],
    );

    ctx.set_fonts(fonts);
}

/// SemiBold proportional family, for the few bold bits (wordmark, section
/// headers). Use as `egui::RichText::new(t).family(theme::semibold())`.
pub fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name("semibold".into())
}

fn apply_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = FIELD_BG; // text edit / DragValue background
    v.faint_bg_color = FIELD_BG;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = accent_soft();
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;

    let r = CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
        w.bg_fill = FIELD_BG;
        w.weak_bg_fill = FIELD_BG;
        w.bg_stroke = Stroke::new(1.0, BORDER_FIELD);
        w.fg_stroke = Stroke::new(1.0, TEXT);
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_HOVER);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 8.0);
    s.button_padding = egui::vec2(8.0, 6.0);
    s.interact_size.y = FIELD_H;
    s.window_margin = egui::Margin::same(0);

    ctx.set_style(style);
}
```

- [ ] **Step 3: Wire it up**

In `src/viewer/mod.rs`: add `mod theme;` near the other `mod` lines. In `ViewerApp::new`, replace `icons::install(&cc.egui_ctx);` with `theme::install(&cc.egui_ctx);` (theme installs the same Phosphor font, so `icons::install` is no longer called here — leave `icons.rs` itself in place; its constants are still used).

- [ ] **Step 4: Build + run**

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean.
Run: `cargo run` — the existing UI should now render in IBM Plex with the dark Lumi palette (panels darker, accent-blue selection). Close the window.

- [ ] **Step 5: Wasm check + commit**

Run: `just web-check 2>&1 | tail -5`
Expected: wasm type-checks (fonts are `include_bytes!`, no platform code).

```bash
cargo fmt
git add -A && git commit -m "feat: theme module — IBM Plex fonts + Lumi palette + egui style"
```

---

## Task 4: Icon additions

**Files:**
- Modify: `src/viewer/icons.rs`

**Interfaces:**
- Produces new `pub const` glyphs: `EYE`, `EYE_SLASH`, `COPY`, `FOLDER`, `DOWNLOAD`, `CARET_DOWN`, `PLAY`, `RESET` (arrow-counter-clockwise), `SLIDERS`, `LAYERS`.

- [ ] **Step 1: Add the constants**

The codepoints are Phosphor private-use-area glyphs. Look up the exact `\u{Exxx}` for each name from the `egui-phosphor` crate's regular variant (`egui_phosphor::regular::EYE`, etc.) or the Phosphor cheatsheet — do **not** guess. Add to `src/viewer/icons.rs`:

```rust
pub const EYE: &str = "\u{????}";        // egui_phosphor::regular::EYE
pub const EYE_SLASH: &str = "\u{????}";  // EYE_SLASH
pub const COPY: &str = "\u{????}";       // COPY
pub const FOLDER: &str = "\u{????}";     // FOLDER (or FOLDER_OPEN)
pub const DOWNLOAD: &str = "\u{????}";   // DOWNLOAD_SIMPLE
pub const CARET_DOWN: &str = "\u{????}"; // CARET_DOWN
pub const PLAY: &str = "\u{????}";       // PLAY
pub const RESET: &str = "\u{????}";      // ARROW_COUNTER_CLOCKWISE
pub const SLIDERS: &str = "\u{????}";    // SLIDERS_HORIZONTAL
pub const LAYERS: &str = "\u{????}";     // STACK already exists; reuse it for "layers" if preferred
```

Replace each `????` with the real codepoint. Verify the glyph renders (it will show as a box if the codepoint is absent from `Phosphor.ttf`).

- [ ] **Step 2: Build + visually verify a glyph**

Add a throwaway `ui.label(icons::EYE);` to the existing left panel, `cargo run`, confirm an eye icon (not a tofu box) renders, then remove the throwaway line.

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: add eye/copy/folder/download/caret/play/reset icons"
```

---

## Task 5: Widgets toolkit

**Files:**
- Create: `src/viewer/widgets/mod.rs`, `axis_field.rs`, `prop_row.rs`, `card.rs`, `combo.rs`, `tab_bar.rs`, `buttons.rs`
- Modify: `src/viewer/mod.rs` (`mod widgets;`)

**Interfaces (Produces — later tasks rely on these exact signatures):**
- `axis_field(ui, axis: Axis, value: &mut f32, speed: f32, decimals: Option<usize>, suffix: &str, range: Option<RangeInclusive<f32>>) -> bool` where `pub enum Axis { X, Y, Z, None }`.
- `axis_vec(ui, v: &mut Vec3, speed, suffix, decimals, range) -> bool` — three stacked `axis_field`s (X/Y/Z) in a horizontal row.
- `int_field(ui, value: &mut u32, range: Option<RangeInclusive<u32>>) -> bool` — mono pill, no axis letter.
- `prop_row<R>(ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> R` — right-aligned fixed-width label + content.
- `section_header(ui, icon: &str, title: &str)` — accent icon + uppercase letter-spaced title.
- `card(ui) -> egui::Frame`, `overlay_frame() -> egui::Frame` (translucent dark, rounded — for viewport overlays).
- `styled_combo<T>(ui, id: &str, current: &str, width: f32, body) -> bool`.
- `pill_tabs(ui, current: &mut T, tabs: &[(T, &str, &str)]) -> bool` (icon, label) — the 3-up inspector tabs; returns true if changed. `T: PartialEq + Copy`.
- `segmented(ui, current: &mut M, left: (M,&str,&str), right: (M,&str,&str)) -> bool` — the Render/Edit toggle.
- `icon_button(ui, icon: &str, tooltip: &str, danger: bool) -> bool`, `pill_button(ui, label: &str, accent: bool, enabled: bool) -> egui::Response`.

> These reuse and supersede the private `axis_row`/`prop_row`/`section_header`/`int_row` currently in `controls.rs`. Move the field logic here and restyle; keep behavior (drag-to-scrub, click-to-type) identical.

- [ ] **Step 1: `prop_row.rs`**

Create `src/viewer/widgets/prop_row.rs` — lift `prop_row`/`section_header` from `controls.rs:43-56,221-224`, make them `pub`, restyle `section_header` with the accent icon + `theme::semibold()` + uppercase:

```rust
use eframe::egui::{self, Ui};
use super::super::theme;

/// Right-aligned label in a fixed column, then `content` filling the rest.
pub const LABEL_W: f32 = 84.0;

pub fn prop_row<R>(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let h = theme::FIELD_H;
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_W, h),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED));
            },
        );
        content(ui)
    })
    .inner
}

/// Accent-iconed, uppercase, letter-spaced sub-heading.
pub fn section_header(ui: &mut Ui, icon: &str, title: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).color(theme::ACCENT));
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .family(theme::semibold())
                .color(egui::Color32::from_rgb(0xb9, 0xbd, 0xc4))
                .size(11.5),
        );
    });
    ui.add_space(4.0);
}
```

- [ ] **Step 2: `axis_field.rs`**

Create `src/viewer/widgets/axis_field.rs`. The field paints a colored axis glyph at the left edge of a mono `DragValue` styled as a pill. Implementation approach: a horizontal group with fixed total height `FIELD_H`; draw the axis letter in a 20px-wide cell, then the `DragValue` filling the rest, monospace, no prefix. Use the existing `DragValue` config from `controls.rs:axis_row` for speed/decimals/suffix/range.

```rust
use std::ops::RangeInclusive;
use eframe::egui::{self, Ui};
use crate::vec3::Vec3;
use super::super::theme;

#[derive(Clone, Copy)]
pub enum Axis { X, Y, Z, None }

impl Axis {
    fn glyph(self) -> Option<(&'static str, egui::Color32)> {
        match self {
            Axis::X => Some(("X", theme::AXIS_X)),
            Axis::Y => Some(("Y", theme::AXIS_Y)),
            Axis::Z => Some(("Z", theme::AXIS_Z)),
            Axis::None => None,
        }
    }
}

pub fn axis_field(
    ui: &mut Ui,
    axis: Axis,
    value: &mut f32,
    speed: f32,
    decimals: Option<usize>,
    suffix: &str,
    range: Option<RangeInclusive<f32>>,
) -> bool {
    let mut dv = egui::DragValue::new(value).speed(speed);
    if !suffix.is_empty() { dv = dv.suffix(suffix); }
    if let Some(d) = decimals { dv = dv.fixed_decimals(d); }
    if let Some(r) = range { dv = dv.range(r); }

    let mut changed = false;
    egui::Frame::NONE
        .fill(theme::FIELD_BG)
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(2, 0))
        .show(ui, |ui| {
            ui.set_height(theme::FIELD_H);
            ui.horizontal_centered(|ui| {
                if let Some((g, c)) = axis.glyph() {
                    ui.allocate_ui_with_layout(
                        egui::vec2(18.0, theme::FIELD_H),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| { ui.label(egui::RichText::new(g).monospace().color(c)); },
                    );
                }
                ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
                ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
                ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::NONE;
                ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                changed = ui.add_sized([ui.available_width(), theme::FIELD_H], dv).changed();
            });
        });
    changed
}

pub fn axis_vec(
    ui: &mut Ui, v: &mut Vec3, speed: f32, suffix: &str,
    decimals: Option<usize>, range: Option<RangeInclusive<f32>>,
) -> bool {
    let mut c = false;
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
        for (axis, comp) in [(Axis::X, &mut v.x), (Axis::Y, &mut v.y), (Axis::Z, &mut v.z)] {
            ui.allocate_ui(egui::vec2(w, theme::FIELD_H), |ui| {
                c |= axis_field(ui, axis, comp, speed, decimals, suffix, range.clone());
            });
        }
    });
    c
}

pub fn int_field(ui: &mut Ui, value: &mut u32, range: Option<RangeInclusive<u32>>) -> bool {
    let mut f = *value as f32;
    let changed = axis_field(ui, Axis::None, &mut f, 1.0, Some(0), "",
        range.map(|r| (*r.start() as f32)..=(*r.end() as f32)));
    if changed { *value = f.round().max(0.0) as u32; }
    changed
}
```

> Note: the exact egui method names for transparent inner widgets may need a small tweak at compile time (e.g. `ui.style_mut()`); the intent is "no inner border, mono value, fixed height". Adjust until it compiles and looks like the mockup pill.

- [ ] **Step 3: `card.rs`, `combo.rs`, `buttons.rs`, `tab_bar.rs`**

Create `src/viewer/widgets/card.rs`:

```rust
use eframe::egui::{self};
use super::super::theme;

pub fn card(_ui: &egui::Ui) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme::FIELD_BG)
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(11))
}

/// Translucent dark frame for floating viewport overlays (blur isn't available
/// in egui, so we use an opaque-ish dark fill instead).
pub fn overlay_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_premultiplied(14, 15, 18, 210))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(18)))
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(12, 8))
}
```

Create `src/viewer/widgets/combo.rs` — a thin wrapper over `egui::ComboBox` matching the field pill (used by material/texture/projection/format selectors):

```rust
use eframe::egui::{self, Ui};

/// A styled combo. `body` adds `selectable_label`s and returns whether the
/// selection changed. Returns that flag.
pub fn styled_combo(
    ui: &mut Ui, id: &str, current: &str, width: f32,
    body: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(width)
        .show_ui(ui, |ui| { changed = body(ui); });
    changed
}
```

Create `src/viewer/widgets/buttons.rs`:

```rust
use eframe::egui::{self, Ui};
use super::super::theme;

/// 30px square icon button with a tinted hover; `danger` tints red on hover.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str, danger: bool) -> bool {
    let resp = ui.add_sized(
        [30.0, 30.0],
        egui::Button::new(egui::RichText::new(icon).color(theme::TEXT_MUTED))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD)),
    );
    let resp = resp.on_hover_text(tooltip);
    if danger && resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect, egui::CornerRadius::same(7),
            egui::Stroke::new(1.0, egui::Color32::from_rgb(0x7a, 0x3a, 0x3a)),
            egui::StrokeKind::Inside,
        );
    }
    resp.clicked()
}

/// A text pill button. `accent` fills with the accent colour (primary action).
pub fn pill_button(ui: &mut Ui, label: &str, accent: bool, enabled: bool) -> egui::Response {
    let mut btn = egui::Button::new(egui::RichText::new(label).color(theme::TEXT_STRONG))
        .corner_radius(egui::CornerRadius::same(8))
        .min_size(egui::vec2(0.0, 32.0));
    btn = if accent {
        btn.fill(theme::ACCENT)
    } else {
        btn.fill(egui::Color32::from_rgb(0x22, 0x25, 0x2a))
            .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
    };
    ui.add_enabled(enabled, btn)
}
```

Create `src/viewer/widgets/tab_bar.rs`:

```rust
use eframe::egui::{self, Ui};
use super::super::theme;

/// 3-up pill tab selector. `tabs` is (value, icon, label). Returns true if the
/// selection changed.
pub fn pill_tabs<T: PartialEq + Copy>(
    ui: &mut Ui, current: &mut T, tabs: &[(T, &str, &str)],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let n = tabs.len() as f32;
        let w = (ui.available_width() - (n - 1.0) * ui.spacing().item_spacing.x) / n;
        for (val, icon, label) in tabs {
            let active = *current == *val;
            let text = egui::RichText::new(format!("{icon}  {label}"))
                .color(if active { theme::ACCENT } else { theme::TEXT_MUTED });
            let fill = if active { theme::accent_soft() } else { egui::Color32::TRANSPARENT };
            let btn = egui::Button::new(text)
                .fill(fill)
                .corner_radius(egui::CornerRadius::same(7))
                .min_size(egui::vec2(w, 31.0));
            if ui.add(btn).clicked() && !active {
                *current = *val;
                changed = true;
            }
        }
    });
    changed
}

/// Two-segment toggle (Render / Edit) styled as the mockup centre pill.
pub fn segmented<M: PartialEq + Copy>(
    ui: &mut Ui, current: &mut M,
    left: (M, &str, &str), right: (M, &str, &str),
) -> bool {
    let mut changed = false;
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(0x0f, 0x10, 0x14))
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (val, icon, label) in [left, right] {
                    let active = *current == val;
                    let text = egui::RichText::new(format!("{icon}  {label}"))
                        .color(if active { egui::Color32::WHITE } else { theme::TEXT_MUTED });
                    let fill = if active { theme::ACCENT } else { egui::Color32::TRANSPARENT };
                    let btn = egui::Button::new(text).fill(fill)
                        .corner_radius(egui::CornerRadius::same(7));
                    if ui.add(btn).clicked() && !active { *current = val; changed = true; }
                }
            });
        });
    changed
}
```

- [ ] **Step 4: `widgets/mod.rs`**

```rust
mod axis_field;
mod buttons;
mod card;
mod combo;
mod prop_row;
mod tab_bar;

pub use axis_field::{axis_field, axis_vec, int_field, Axis};
pub use buttons::{icon_button, pill_button};
pub use card::{card, overlay_frame};
pub use combo::styled_combo;
pub use prop_row::{prop_row, section_header, LABEL_W};
pub use tab_bar::{pill_tabs, segmented};
```

Add `mod widgets;` to `src/viewer/mod.rs`. To avoid dead-code warnings before panels consume these, temporarily add `#![allow(dead_code)]` at the top of `widgets/mod.rs` (remove in Task 11 once everything is wired).

- [ ] **Step 5: Build**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean (warnings about unused widgets are fine). Fix any egui-API mismatches (method names, `StrokeKind`, `Margin` int vs f32) until it compiles.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: reusable widgets toolkit (axis field, prop row, card, combo, tabs, buttons)"
```

---

## Task 6: UiState + panels scaffolding

**Files:**
- Create: `src/viewer/state.rs`
- Create: `src/viewer/panels/mod.rs` (+ empty `top_bar.rs`, `outliner.rs`, `viewport.rs`, `inspector/mod.rs`, `inspector/object.rs`, `inspector/camera.rs`, `inspector/output.rs` as stubs)
- Modify: `src/viewer/mod.rs` (`mod state; mod panels;`, move `Mode` into `state.rs`)

**Interfaces (Produces):**
- `pub enum Mode { Render, Edit }` (moved from `mod.rs`), `pub enum Tab { Object, Camera, Output }`.
- `pub struct UiState { pub mode: Mode, pub selected: Option<usize>, pub tab: Tab, pub add_menu_open: bool, pub gizmo_local: bool, pub gizmo_modes: raster::gizmo::GizmoModes, pub last_interact: f64 }` with `Default`.
- `pub enum Action { None, SaveImage, SaveScene, ResetCamera, Restart }` in `panels/mod.rs`.

- [ ] **Step 1: Write `state.rs`**

```rust
use super::raster;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode { Render, Edit }

#[derive(Clone, Copy, PartialEq)]
pub enum Tab { Object, Camera, Output }

pub struct UiState {
    pub mode: Mode,
    pub selected: Option<usize>,
    pub tab: Tab,
    pub add_menu_open: bool,
    pub gizmo_local: bool,
    pub gizmo_modes: raster::gizmo::GizmoModes,
    /// egui time of the last camera motion (preview debounce).
    pub last_interact: f64,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            mode: Mode::Render,
            selected: None,
            tab: Tab::Object,
            add_menu_open: false,
            gizmo_local: false,
            gizmo_modes: raster::gizmo::GizmoModes { translate: true, rotate: true, scale: true },
            last_interact: -1.0,
        }
    }
}
```

- [ ] **Step 2: `panels/mod.rs` + stubs**

```rust
mod top_bar;
mod outliner;
mod viewport;
mod inspector;

pub use top_bar::show_top_bar;
pub use outliner::show_outliner;
pub use viewport::show_viewport;
pub use inspector::show_inspector;

/// One-shot actions a panel asks `ViewerApp` to perform after layout.
#[derive(Clone, Copy, PartialEq)]
pub enum Action { None, SaveImage, SaveScene, ResetCamera, Restart }
```

Create each stub file with a no-op `pub fn` of the right name returning sensible defaults, e.g. `src/viewer/panels/top_bar.rs`:

```rust
use eframe::egui::Ui;
use super::Action;
use crate::scene::Scene;
use super::super::state::UiState;

pub fn show_top_bar(_ui: &mut Ui, _ui_state: &mut UiState, _scene: &mut Scene) -> Action {
    Action::None
}
```

Mirror for `show_outliner(ui, ui_state, scene) -> bool` (dirty), `show_inspector(ui, ui_state, scene) -> bool`, and `show_viewport` (signature finalized in Task 10 — stub it as `pub fn show_viewport() {}` for now, or omit from `mod.rs` until Task 10). `inspector/mod.rs` re-exports `show_inspector`; `object.rs`/`camera.rs`/`output.rs` start empty.

- [ ] **Step 3: Wire modules, keep old UI running**

In `mod.rs` add `mod state;` and `mod panels;`. Remove the local `enum Mode` (now in `state.rs`) and `use state::Mode;`. Do **not** switch `ViewerApp::ui` to the new panels yet — keep the existing layout compiling against `state::Mode`. Add `#![allow(dead_code)]` to `panels/mod.rs` temporarily.

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | tail -10`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: UiState + panels module scaffolding"
```

---

## Task 7: Top bar panel

**Files:**
- Modify: `src/viewer/panels/top_bar.rs`

**Interfaces:**
- Consumes: `widgets::segmented`, `widgets::pill_button`, `theme`, `icons`, `state::{UiState, Mode}`.
- Produces: `pub fn show_top_bar(ui, &mut UiState, &Scene) -> Action` — renders logo, scene chip, Render/Edit toggle, Save scene (disabled), Save image (returns `Action::SaveImage`).

- [ ] **Step 1: Implement**

```rust
use eframe::egui::{self, Ui};
use super::Action;
use crate::scene::Scene;
use super::super::{icons, state::{UiState, Mode}, theme, widgets};

pub fn show_top_bar(ui: &mut Ui, ui_state: &mut UiState, _scene: &Scene) -> Action {
    let mut action = Action::None;
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        // Logo chip + wordmark.
        ui.label(egui::RichText::new(icons::APERTURE).color(theme::ACCENT).size(18.0));
        ui.label(egui::RichText::new("Lumi").family(theme::semibold()).color(theme::TEXT_STRONG).size(15.0));
        ui.add_space(6.0);
        // Scene chip (display-only until save/load lands).
        ui.label(egui::RichText::new(format!("{}  cornell-box.scene", icons::FOLDER))
            .monospace().color(theme::TEXT));

        // Centre: Render / Edit toggle. Use a centered layout region.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Save image (primary) on the far right, then Save scene (disabled).
            if widgets::pill_button(ui, &format!("{}  Save image", icons::DOWNLOAD), true, true).clicked() {
                action = Action::SaveImage;
            }
            let _ = widgets::pill_button(ui, &format!("{}  Save scene", icons::FLOPPY), false, false)
                .on_hover_text("Scene save/load is coming soon");
            ui.add_space(8.0);
            // The mode toggle, pushed toward centre.
            widgets::segmented(ui, &mut ui_state.mode,
                (Mode::Render, icons::PLAY, "Render"),
                (Mode::Edit, icons::ARROWS_OUT_CARDINAL, "Edit"));
        });
    });
    action
}
```

> The mockup centers the toggle absolutely; egui can't absolutely-position inside a row easily, so right-aligning the toggle next to the Save buttons is the pragmatic match. If you want it truly centered, allocate the row as three equal `columns` (left logo, center toggle, right buttons) instead — acceptable either way.

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -10`
Expected: builds clean (still unused until Task 11 — `#![allow(dead_code)]` covers it).

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: top bar panel (logo, scene chip, mode toggle, save buttons)"
```

---

## Task 8: Outliner panel

**Files:**
- Modify: `src/viewer/panels/outliner.rs`
- Modify: `src/viewer/controls.rs` (make `default_sphere`, `default_box`, and `shape_icon` `pub(crate)` so the outliner reuses them; move the OBJ-import block into a `pub(crate) fn import_obj(objects, selected) -> bool` here or keep it callable)

**Interfaces:**
- Consumes: `theme`, `icons`, `widgets`, `controls::{default_sphere, default_box, shape_icon}`, `scene::ObjectSpec`.
- Produces: `pub fn show_outliner(ui, &mut UiState, &mut Scene) -> bool` (dirty) — Scene header + count, Add-object popup, object rows with eye toggle. Selecting a row sets `ui_state.selected` and `ui_state.tab = Tab::Object`. Toggling eye flips `obj.hidden` and returns dirty.

- [ ] **Step 1: Make reused helpers visible**

In `controls.rs`, change `fn default_sphere`/`fn default_box` to `pub(crate) fn`, and `fn shape_icon` to `pub(crate) fn shape_icon(s: &Shape) -> &'static str`. Extract the existing OBJ-import logic (`object_list`'s import button body, `controls.rs:180-215`) into `pub(crate) fn import_obj(objects: &mut Vec<ObjectSpec>, selected: &mut Option<usize>) -> bool` in `controls.rs`, preserving the `#[cfg]` split.

- [ ] **Step 2: Implement the outliner**

```rust
use eframe::egui::{self, Ui};
use crate::scene::Scene;
use super::super::{controls, icons, state::{Tab, UiState}, theme, widgets};

pub fn show_outliner(ui: &mut Ui, ui_state: &mut UiState, scene: &mut Scene) -> bool {
    let mut dirty = false;

    // Header.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icons::STACK).color(theme::TEXT_MUTED));
        ui.label(egui::RichText::new("SCENE").family(theme::semibold())
            .color(theme::TEXT_MUTED).size(11.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("{}", scene.objects.len()))
                .monospace().color(theme::TEXT_DIM));
        });
    });
    ui.add_space(4.0);

    // Add object (popup menu).
    let add = ui.add_sized([ui.available_width(), 33.0],
        egui::Button::new(egui::RichText::new(format!("{}  Add object", icons::PLUS)).color(theme::TEXT_STRONG))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD)));
    let popup_id = ui.make_persistent_id("add_object_menu");
    if add.clicked() { ui.memory_mut(|m| m.toggle_popup(popup_id)); }
    egui::popup_below_widget(ui, popup_id, &add, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(220.0);
        ui.label(egui::RichText::new("PRIMITIVES").size(10.0).color(theme::TEXT_DIM));
        if ui.button(format!("{}  Plane", icons::RECTANGLE)).clicked() {
            scene.objects.push(controls::default_box(scene.objects.len())); // see note
        }
        if ui.button(format!("{}  Box", icons::CUBE)).clicked() {
            scene.objects.push(controls::default_box(scene.objects.len()));
            ui_state.selected = Some(scene.objects.len() - 1); ui_state.tab = Tab::Object; dirty = true;
        }
        if ui.button(format!("{}  Sphere", icons::SPHERE)).clicked() {
            scene.objects.push(controls::default_sphere(scene.objects.len()));
            ui_state.selected = Some(scene.objects.len() - 1); ui_state.tab = Tab::Object; dirty = true;
        }
        ui.separator();
        ui.label(egui::RichText::new("SAMPLE MESHES").size(10.0).color(theme::TEXT_DIM));
        for m in ["Suzanne", "Stanford Bunny", "Utah Teapot", "Stanford Dragon"] {
            ui.add_enabled(false, egui::Button::new(format!("{}  {}", icons::POLYGON, m)))
                .on_disabled_hover_text("Bundled sample meshes are coming soon");
        }
        ui.separator();
        if controls::import_obj(&mut scene.objects, &mut ui_state.selected) {
            ui_state.tab = Tab::Object; dirty = true;
        }
    });
    ui.add_space(6.0);

    // Object rows.
    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut toggle_hidden: Option<usize> = None;
        for (i, obj) in scene.objects.iter().enumerate() {
            let selected = ui_state.selected == Some(i);
            let row = ui.horizontal(|ui| {
                let icon_col = if selected { theme::SELECTION } else { theme::TEXT_MUTED };
                ui.label(egui::RichText::new(controls::shape_icon(&obj.shape)).color(icon_col));
                let name_col = if selected { theme::SELECTION } else { theme::TEXT };
                ui.label(egui::RichText::new(&obj.name).color(name_col));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let eye = if obj.hidden { icons::EYE_SLASH } else { icons::EYE };
                    let col = if obj.hidden { theme::TEXT_DIM } else if selected { theme::SELECTION } else { theme::TEXT_DIM };
                    if ui.add(egui::Button::new(egui::RichText::new(eye).color(col))
                        .fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)).clicked() {
                        toggle_hidden = Some(i);
                    }
                });
            });
            // Selection background.
            if selected {
                ui.painter().rect_filled(row.response.rect, egui::CornerRadius::same(7), theme::selection_soft());
            }
            if row.response.interact(egui::Sense::click()).clicked() {
                ui_state.selected = Some(i);
                ui_state.tab = Tab::Object;
            }
        }
        if let Some(i) = toggle_hidden {
            scene.objects[i].hidden = !scene.objects[i].hidden;
            dirty = true;
        }
    });
    dirty
}
```

> Notes: (a) "Plane" should use a quad primitive — if no `default_plane` exists, add a `pub(crate) fn default_plane(n)` in `controls.rs` building a `Shape::Quad` (mirror `default_box`), and call it for the Plane entry instead of `default_box`. (b) The selection-background-then-click ordering may need the rect painted *before* the row content via `ui.scope`/pre-allocation; adjust so the highlight sits behind the row. Keep behaviour: click selects, eye toggles visibility (dirty).

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -15`
Expected: builds clean. Resolve any egui popup API differences (`popup_below_widget` signature / `PopupCloseBehavior`) for 0.34.

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: outliner panel (scene header, add-object menu, rows with visibility)"
```

---

## Task 9: Inspector panels (Object / Camera / Output tabs)

**Files:**
- Modify: `src/viewer/panels/inspector/mod.rs`, `object.rs`, `camera.rs`, `output.rs`
- Modify: `src/viewer/controls.rs` (make `material_controls`, `texture_controls`, `shape_controls`, `transform_controls` `pub(crate)`; rewrite the camera/output bodies to live in the new tabs, reusing `widgets::axis_field`/`axis_vec`/`int_field`/`prop_row`/`section_header`)

**Interfaces:**
- Consumes: `widgets::*`, `controls::{material_controls, texture_controls, shape_controls, transform_controls, shape_icon}`, `scene::{Scene, ObjectSpec, Shape}`, `camera::CameraConfig`, `state::{UiState, Tab}`, `scene::duplicate_object`.
- Produces:
  - `pub fn show_inspector(ui, &mut UiState, &mut Scene) -> bool` — `pill_tabs` then dispatch.
  - `object_tab(ui, &mut UiState, &mut Scene) -> bool`, `camera_tab(ui, &mut CameraConfig) -> bool`, `output_tab(ui, &mut CameraConfig) -> bool`.

- [ ] **Step 1: Expose the shared editors**

In `controls.rs`, change `fn material_controls`, `fn texture_controls`, `fn shape_controls`, `fn transform_controls` to `pub(crate) fn`. Update their internal `axis_row`/`prop_row`/`color_prop` calls to use the new `widgets::` equivalents (replace `axis_row(ui, l, v, …)` → `widgets::axis_field(ui, widgets::Axis::None, v, …)`; `prop_row` → `widgets::prop_row`; keep `color_prop` local or move it to widgets). Delete the now-duplicated private `axis_row`/`axis_vec`/`prop_row`/`section_header`/`int_row` from `controls.rs` once the tabs and these editors reference `widgets::` instead. The old `camera_controls`, `object_list`, `object_settings` functions are superseded — delete them after the tabs below replace their callers (the deletion lands in Task 11 when `mod.rs` stops calling them; until then `#[allow(dead_code)]`).

- [ ] **Step 2: `inspector/mod.rs`**

```rust
mod object;
mod camera;
mod output;

use eframe::egui::Ui;
use crate::scene::Scene;
use super::super::{icons, state::{Tab, UiState}, widgets};

pub fn show_inspector(ui: &mut Ui, ui_state: &mut UiState, scene: &mut Scene) -> bool {
    let mut dirty = false;
    widgets::pill_tabs(ui, &mut ui_state.tab, &[
        (Tab::Object, icons::CUBE, "Object"),
        (Tab::Camera, icons::CAMERA, "Camera"),
        (Tab::Output, icons::IMAGE, "Output"),
    ]);
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        dirty = match ui_state.tab {
            Tab::Object => object::object_tab(ui, ui_state, scene),
            Tab::Camera => camera::camera_tab(ui, &mut scene.camera),
            Tab::Output => output::output_tab(ui, &mut scene.camera),
        };
    });
    dirty
}
```

(Add `use eframe::egui;` as needed.)

- [ ] **Step 3: `object.rs`**

```rust
use eframe::egui::{self, Ui};
use crate::scene::{self, Scene, Shape};
use super::super::super::{controls, icons, state::UiState, theme, widgets};

pub fn object_tab(ui: &mut Ui, ui_state: &mut UiState, scene: &mut Scene) -> bool {
    let mut dirty = false;
    let Some(i) = ui_state.selected.filter(|&i| i < scene.objects.len()) else {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.label(egui::RichText::new(icons::CUBE).color(theme::TEXT_DIM).size(28.0));
            ui.label(egui::RichText::new("No object selected").color(theme::TEXT_MUTED));
            ui.label(egui::RichText::new("Pick something in the scene, or add a new object.")
                .color(theme::TEXT_DIM).size(12.0));
        });
        return false;
    };

    // Header: icon + name + type badge + duplicate + delete.
    let mut do_dup = false;
    let mut do_del = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(controls::shape_icon(&scene.objects[i].shape)).color(theme::SELECTION));
        ui.add(egui::TextEdit::singleline(&mut scene.objects[i].name).desired_width(140.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::TRASH, "Delete object", true) { do_del = true; }
            if widgets::icon_button(ui, icons::COPY, "Duplicate object", false) { do_dup = true; }
        });
    });

    if do_dup {
        if let Some(n) = scene::duplicate_object(&mut scene.objects, i) {
            ui_state.selected = Some(n);
        }
        return true;
    }
    if do_del {
        scene.objects.remove(i);
        ui_state.selected = None;
        return true;
    }

    let is_mesh = matches!(scene.objects[i].shape, Shape::Mesh { .. });

    widgets::section_header(ui, icons::PALETTE, "Material");
    if is_mesh {
        ui.label(egui::RichText::new("baked at import").color(theme::TEXT_DIM));
    } else {
        dirty |= controls::material_controls(ui, &mut scene.objects[i].material);
    }

    if matches!(scene.objects[i].shape, Shape::Sphere { .. } | Shape::Box { .. }) {
        widgets::section_header(ui, icons::SHAPES, "Geometry");
        dirty |= controls::shape_controls(ui, &mut scene.objects[i].shape);
    }

    widgets::section_header(ui, icons::ARROWS_OUT_CARDINAL, "Transform");
    dirty |= controls::transform_controls(ui, &mut scene.objects[i].transform);

    dirty
}
```

- [ ] **Step 4: `camera.rs`**

```rust
use eframe::egui::Ui;
use crate::camera::CameraConfig;
use super::super::super::{icons, widgets};
use widgets::Axis;

pub fn camera_tab(ui: &mut Ui, cam: &mut CameraConfig) -> bool {
    let mut c = false;
    widgets::section_header(ui, icons::CROSSHAIR, "View");
    ui.label("Position");
    c |= widgets::axis_vec(ui, &mut cam.look_from, 1.0, "", None, None);
    ui.label("Target");
    c |= widgets::axis_vec(ui, &mut cam.look_at, 1.0, "", None, None);
    c |= widgets::prop_row(ui, "Roll", |ui| widgets::axis_field(ui, Axis::None, &mut cam.roll, 0.5, Some(1), "°", Some(-180.0..=180.0)));

    widgets::section_header(ui, icons::APERTURE, "Lens");
    c |= widgets::prop_row(ui, "FOV",   |ui| widgets::axis_field(ui, Axis::None, &mut cam.fov, 0.2, Some(1), "°", Some(1.0..=179.0)));
    c |= widgets::prop_row(ui, "DoF",   |ui| widgets::axis_field(ui, Axis::None, &mut cam.dof_angle, 0.05, Some(2), "°", Some(0.0..=180.0)));
    c |= widgets::prop_row(ui, "Focus", |ui| widgets::axis_field(ui, Axis::None, &mut cam.focus_dist, 1.0, Some(1), "", Some(0.001..=1.0e6)));
    c
}
```

- [ ] **Step 5: `output.rs`**

Port the width/height aspect logic from `controls.rs:camera_controls` Output block (the `cur_h` / `aspect_ratio` math) into `output_tab`, plus Samples/Max bounces via `widgets::int_field` and a display-only Format row:

```rust
use eframe::egui::Ui;
use crate::camera::CameraConfig;
use super::super::super::{icons, theme, widgets};

pub fn output_tab(ui: &mut Ui, cam: &mut CameraConfig) -> bool {
    let mut c = false;
    widgets::section_header(ui, icons::IMAGE, "Resolution");
    let cur_h = ((cam.image_width as f64 / cam.aspect_ratio).round().max(1.0)) as u32;
    c |= widgets::prop_row(ui, "Width", |ui| {
        let mut w = cam.image_width;
        if widgets::int_field(ui, &mut w, Some(1..=8192)) {
            cam.image_width = w.max(1);
            cam.aspect_ratio = cam.image_width as f64 / cur_h as f64;
            true
        } else { false }
    });
    c |= widgets::prop_row(ui, "Height", |ui| {
        let mut h = cur_h;
        if widgets::int_field(ui, &mut h, Some(1..=8192)) {
            cam.aspect_ratio = cam.image_width as f64 / h.max(1) as f64;
            true
        } else { false }
    });

    widgets::section_header(ui, icons::SLIDERS, "Quality");
    c |= widgets::prop_row(ui, "Samples", |ui| widgets::int_field(ui, &mut cam.samples, Some(1..=100_000)));
    c |= widgets::prop_row(ui, "Max bounces", |ui| widgets::int_field(ui, &mut cam.max_depth, Some(1..=1_000)));

    widgets::prop_row(ui, "Format", |ui| { ui.label(egui::RichText::new("PNG · 16-bit").color(theme::TEXT)); });
    c
}
```

(Add `use eframe::egui;` where `egui::RichText` is used.)

- [ ] **Step 6: Build**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean (dead-code-allowed). Fix API mismatches.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: inspector tabs (object/camera/output) reusing shared editors + widgets"
```

---

## Task 10: Viewport panel (image/GL host + overlays + status dock)

**Files:**
- Modify: `src/viewer/panels/viewport.rs`
- Modify: `src/viewer/panels/mod.rs` (export `show_viewport`)

**Interfaces:**
- This panel needs renderer state that lives on `ViewerApp` (texture, view-transform, render-task handle, gl renderer, gizmo). Rather than pass all of it, **keep the viewport's heavy logic in `mod.rs`** (Task 11) and put only the **overlays + status dock** drawing in `viewport.rs` as helpers that take plain data.
- Produces:
  - `pub fn overlays(ui, rect, mode, gizmo_modes: &mut GizmoModes, gizmo_local: &mut bool, res: (u32,u32)) -> bool` — draws the resolution badge, the Edit toolbar (Move/Rotate/Scale/Local), and the Reset-camera chip on top of `rect`; returns true if Reset was clicked.
  - `pub fn status_dock(ui, mode, done: bool, passes: u32, total: u32, elapsed: f32, cam: &mut CameraConfig) -> StatusOut` where `pub struct StatusOut { pub restart: bool, pub dirty: bool }`. Renders the thin progress line + status row with editable Samples/Bounces (mutating `cam`, returning `dirty`) + restart button.

- [ ] **Step 1: Implement overlays + status dock**

```rust
use eframe::egui::{self, Ui, Rect};
use crate::camera::CameraConfig;
use super::super::{icons, raster::gizmo::GizmoModes, state::Mode, theme, widgets};

pub fn overlays(
    ui: &Ui, rect: Rect, mode: Mode,
    gizmo_modes: &mut GizmoModes, gizmo_local: &mut bool, res: (u32, u32),
) -> bool {
    let mut reset = false;
    // Resolution badge (top-left).
    egui::Area::new("vp_res".into()).fixed_pos(rect.left_top() + egui::vec2(16.0, 14.0))
        .order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            widgets::overlay_frame().show(ui, |ui| {
                ui.label(egui::RichText::new(format!("{}  {} × {}", icons::IMAGE, res.0, res.1))
                    .monospace().color(theme::TEXT));
            });
        });
    // Reset-camera chip (bottom-left).
    egui::Area::new("vp_reset".into()).fixed_pos(rect.left_bottom() + egui::vec2(16.0, -44.0))
        .order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            widgets::overlay_frame().show(ui, |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(format!("{}  Reset camera", icons::RESET)).color(theme::TEXT))
                    .fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)).clicked() { reset = true; }
            });
        });
    // Edit toolbar (top-center).
    if mode == Mode::Edit {
        egui::Area::new("vp_tools".into())
            .fixed_pos(rect.center_top() + egui::vec2(-150.0, 14.0))
            .order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                widgets::overlay_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        tool(ui, &mut gizmo_modes.translate, icons::ARROWS_OUT_CARDINAL, "Move");
                        tool(ui, &mut gizmo_modes.rotate, icons::ARROWS_CLOCKWISE, "Rotate");
                        tool(ui, &mut gizmo_modes.scale, icons::RESIZE, "Scale");
                        ui.separator();
                        ui.checkbox(gizmo_local, "Local axes");
                    });
                });
            });
    }
    reset
}

fn tool(ui: &mut Ui, on: &mut bool, icon: &str, label: &str) {
    let col = if *on { theme::ACCENT } else { theme::TEXT_MUTED };
    if ui.add(egui::Button::new(egui::RichText::new(format!("{icon}  {label}")).color(col))
        .fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)).clicked() { *on ^= true; }
}

pub struct StatusOut { pub restart: bool, pub dirty: bool }

pub fn status_dock(
    ui: &mut Ui, mode: Mode, done: bool, passes: u32, total: u32, elapsed: f32,
    cam: &mut CameraConfig,
) -> StatusOut {
    let mut out = StatusOut { restart: false, dirty: false };
    // Thin progress line.
    let frac = if total > 0 { passes as f32 / total as f32 } else { 0.0 };
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), egui::Sense::hover());
    ui.painter().rect_filled(line, 0.0, egui::Color32::from_rgb(0x1c, 0x1f, 0x24));
    let mut fill = line; fill.set_width(line.width() * frac);
    ui.painter().rect_filled(fill, 0.0, theme::ACCENT);

    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let (dot, text) = match (mode, done) {
            (Mode::Edit, _) => (theme::TEXT_DIM, "Editing".to_string()),
            (Mode::Render, true) => (egui::Color32::from_rgb(0x54, 0xc9, 0x8a), "Done".to_string()),
            (Mode::Render, false) => (theme::ACCENT, "Rendering…".to_string()),
        };
        ui.label(egui::RichText::new("●").color(dot).size(10.0));
        ui.label(egui::RichText::new(text).color(theme::TEXT_STRONG));
        if mode == Mode::Render {
            ui.separator();
            ui.label(egui::RichText::new(format!("{passes} / {total} passes · {elapsed:.1}s"))
                .monospace().color(theme::TEXT));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::RESET, "Restart render", false) { out.restart = true; }
            ui.label(egui::RichText::new("Bounces").color(theme::TEXT_DIM).size(11.0));
            out.dirty |= widgets::int_field(ui, &mut cam.max_depth, Some(1..=1_000));
            ui.label(egui::RichText::new("Samples").color(theme::TEXT_DIM).size(11.0));
            out.dirty |= widgets::int_field(ui, &mut cam.samples, Some(1..=100_000));
        });
    });
    out
}
```

Export `pub use viewport::{overlays, status_dock, StatusOut};` from `panels/mod.rs` (these are helpers, not a single `show_viewport`).

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean. Fix `egui::Area` / `Order` API for 0.34 as needed.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: viewport overlays + status dock helpers"
```

---

## Task 11: Wire ViewerApp orchestration; remove old UI

**Files:**
- Modify: `src/viewer/mod.rs` (rewrite `ui`), move `Mode` usage to `state::Mode`, hold `UiState`
- Modify: `src/viewer/controls.rs` (delete superseded `camera_controls`, `object_list`, `object_settings`, the old private row helpers, and the local `card`/`section_header` now in widgets)
- Remove the temporary `#![allow(dead_code)]` from `widgets/mod.rs` and `panels/mod.rs`

**Interfaces:**
- Consumes everything produced in Tasks 5–10.

- [ ] **Step 1: Replace `ViewerApp` UI state**

In `ViewerApp`, replace the scattered fields (`selected`, `mode`, `gizmo_local`, `gizmo_modes`, `last_interact`) with `ui_state: UiState`. Keep `scene`, `render`, `texture`, `shown_pass`, `view`, `initial_camera`, `gl_renderer`, `gizmo`. Update `new` to set `ui_state: UiState::default()`.

- [ ] **Step 2: Rewrite `ui` to lay out the new panels**

Structure (pseudocode → real code; preserve the existing render-pump, texture upload, mode-transition pause/resume, and the Edit-mode gizmo/orbit/pick logic verbatim — only the layout containers change):

```rust
fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
    let ctx = ui.ctx().clone();
    self.render.pump();
    // … pull frame + upload texture (unchanged) …

    let mut dirty = false;
    let mut actions: Vec<panels::Action> = Vec::new();
    let mode_before = self.ui_state.mode;

    // Top bar.
    egui::TopBottomPanel::top("top_bar").exact_height(54.0)
        .frame(egui::Frame::NONE.fill(theme::BG_TOPBAR))
        .show_inside(ui, |ui| {
            let mut scene = self.scene.lock().unwrap();
            actions.push(panels::show_top_bar(ui, &mut self.ui_state, &scene));
        });

    // Left outliner.
    egui::SidePanel::left("outliner").exact_width(286.0).resizable(false)
        .frame(egui::Frame::NONE.fill(theme::BG_PANEL).inner_margin(egui::Margin::same(12)))
        .show_inside(ui, |ui| {
            let mut scene = self.scene.lock().unwrap();
            dirty |= panels::show_outliner(ui, &mut self.ui_state, &mut scene);
        });

    // Right inspector.
    egui::SidePanel::right("inspector").exact_width(342.0).resizable(false)
        .frame(egui::Frame::NONE.fill(theme::BG_PANEL).inner_margin(egui::Margin::same(12)))
        .show_inside(ui, |ui| {
            let mut scene = self.scene.lock().unwrap();
            dirty |= panels::show_inspector(ui, &mut self.ui_state, &mut scene);
        });

    // Status dock at the bottom of the central area.
    egui::TopBottomPanel::bottom("status_dock").exact_height(63.0)
        .frame(egui::Frame::NONE.fill(theme::BG_TOPBAR))
        .show_inside(ui, |ui| {
            let mut scene = self.scene.lock().unwrap();
            let out = panels::status_dock(ui, self.ui_state.mode, done, passes, total, elapsed, &mut scene.camera);
            if out.restart { actions.push(panels::Action::Restart); }
            dirty |= out.dirty;
        });

    // Central viewport: image/GL + overlays (existing logic, rehosted).
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme::BG_VIEWPORT))
        .show_inside(ui, |ui| {
            let vp = ui.available_rect_before_wrap();
            // … existing Render/Edit rendering, pan/zoom, gizmo, orbit, pick …
            // After painting, draw overlays:
            let (mut gm, mut gl) = (self.ui_state.gizmo_modes, self.ui_state.gizmo_local);
            if panels::overlays(ui, image_rect, self.ui_state.mode, &mut gm, &mut gl, (img_w, img_h)) {
                actions.push(panels::Action::ResetCamera);
            }
            self.ui_state.gizmo_modes = gm; self.ui_state.gizmo_local = gl;
        });

    // Apply actions + dirty centrally (after all locks released).
    for a in actions {
        match a {
            panels::Action::SaveImage => { /* existing PNG save */ }
            panels::Action::ResetCamera => {
                let mut scene = self.scene.lock().unwrap();
                scene.camera = self.initial_camera.clone();
                self.render.invalidate();
            }
            panels::Action::Restart => self.render.invalidate(),
            _ => {}
        }
    }
    if dirty { self.render.invalidate(); }
    if mode_before != self.ui_state.mode {
        match self.ui_state.mode {
            state::Mode::Edit => self.render.pause(),
            state::Mode::Render => self.render.resume(),
        }
    }
}
```

> Keep the existing Edit-mode interaction block (gizmo `interact`, orbit/pan/dolly, click-to-pick, preview-scale debounce) exactly as it is today — it just moves inside the `CentralPanel::show_inside` closure and reads/writes `self.ui_state.selected`, `self.ui_state.last_interact`, `self.ui_state.gizmo_*` instead of the old fields. The old top `Panel::top("gizmo_toolbar")` block is **deleted** (replaced by `panels::overlays`).

- [ ] **Step 3: Delete superseded code**

Remove from `controls.rs`: `camera_controls`, `object_list`, `object_settings`, and the now-unused private `axis_row`, `axis_vec`, `prop_row`, `section_header`, `int_row`, `card` (use `widgets::card`). Keep `material_controls`, `texture_controls`, `cell_texture_controls`, `image_picker_row`, `shape_controls`, `transform_controls`, `pick`, `shared_color`, `shared_roughness`, `color_prop`, `default_sphere`, `default_box`, `default_plane`, `import_obj`, `shape_icon`. Remove the `card` fn from `mod.rs` too. Remove both temporary `#![allow(dead_code)]`.

- [ ] **Step 4: Build, fix warnings, run**

Run: `cargo build 2>&1 | tail -30`
Expected: builds clean, **no dead-code warnings** (everything is wired). Fix any leftover references.
Run: `cargo run` — verify against the mockup: top bar with Lumi + mode toggle; left outliner with object rows, eye toggles, Add-object menu; center viewport renders; status dock shows passes + editable Samples/Bounces + restart; right inspector tabs switch Object/Camera/Output and edit live. Toggle an eye → object disappears from the render. Duplicate → a "… copy" appears and is selected. Close.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add -A && git commit -m "feat: assemble Lumi editor layout; remove superseded controls UI"
```

---

## Task 12: Wasm parity + final verification

**Files:**
- Modify: any panel with native-only actions (confirm `#[cfg]` fallbacks) — primarily `controls::import_obj`, `image_picker_row`, and the Save-image action.

- [ ] **Step 1: Wasm type-check**

Run: `just web-check 2>&1 | tail -20`
Expected: wasm type-checks. If it fails, the cause is almost certainly a native-only call (`rfd`, `std::fs`, PNG save) reached from a panel without a `#[cfg(target_arch = "wasm32")]` disabled fallback. Add the fallback (disabled button + `on_disabled_hover_text`) mirroring the existing pattern in `controls.rs`.

- [ ] **Step 2: Full test + build sweep**

Run: `cargo test 2>&1 | tail -15`
Expected: all pass (visibility + duplicate tests + existing suite).
Run: `cargo build --release 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 3: Manual verification checklist (native)**

`cargo run` and confirm each against `ui-design-mockup.html`:
- [ ] Top bar: Lumi wordmark, scene chip, centered-ish Render/Edit toggle, Save image works (writes PNG), Save scene disabled with tooltip.
- [ ] Outliner: SCENE header + count; Add-object menu (Plane/Box/Sphere add + select; sample meshes disabled; Import .obj works natively); rows show type icon + name + eye; selected row highlighted in selection-orange; eye toggles visibility and the render updates.
- [ ] Viewport: image in Render, GL preview in Edit; resolution badge; Edit toolbar (Move/Rotate/Scale/Local) drives the gizmo; Reset-camera chip works.
- [ ] Status dock: status dot/text reflect mode + done; passes/elapsed in mono; Samples/Bounces editable and invalidate the render; restart works.
- [ ] Inspector: tabs switch; Object tab shows name/duplicate/delete + Material/Geometry/Transform and edits live; Camera tab edits view/lens; Output tab edits resolution/quality.

- [ ] **Step 4: Update memory + commit**

Update `MEMORY.md`: note the editor UI was rebuilt to the Lumi design with `theme`/`widgets`/`panels` modules.

```bash
cargo fmt
git add -A && git commit -m "chore: wasm parity fallbacks + final verification for Lumi editor UI"
```

---

## Self-Review

**Spec coverage:**
- Theme/fonts/palette/style → Task 3. ✓
- Module layout (theme/widgets/panels/state) → Tasks 3,5,6,7–10. ✓
- Number field (axis pill, drag-scrub) → Task 5. ✓
- Top bar / outliner / viewport+dock / inspector tabs → Tasks 7,8,10,9. ✓
- Visibility + duplicate (+ tests) → Tasks 1,2 (GL skip in Task 1). ✓
- Status-dock editable Samples/Bounces → Task 10. ✓
- Selection shared + outliner switches to Object tab → Tasks 8,9. ✓
- Dirty tracking via returned flags applied centrally → Task 11. ✓
- Gizmo toolbar moved to overlay → Tasks 10,11. ✓
- Fixed 286/342 widths, flexing center → Task 11. ✓
- WASM parity fallbacks → Task 12. ✓
- Deferred-but-visible (sample meshes disabled, scene chip display-only, Save scene disabled, Format display-only) → Tasks 7,8,9. ✓

**Type consistency:** `UiState`/`Mode`/`Tab`/`Action` defined in Task 6 and used consistently in 7–11. Widget signatures defined in Task 5 match their call sites in 8–10. `duplicate_object`/`hidden` from Tasks 1–2 used in Tasks 8–9.

**Placeholder scan:** The only deliberate fill-ins are the Phosphor codepoints in Task 4 (engineer must look up real values — flagged explicitly, not guessable) and the "adjust until egui 0.34 API matches" notes (egui has minor signature churn across point releases; these are real integration steps, not vague hand-waving). All code steps include concrete code.
