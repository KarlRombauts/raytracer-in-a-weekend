# Editor UI Redesign — Design

**Date:** 2026-06-29
**Status:** Approved (pending implementation plan)

## Goal

Rebuild the egui viewer's **editor screen** to match the visual design in
`ui-design-mockup.html`, and reorganize the UI code into clean, reusable
components. The mockup is a polished 3-zone editor: a top bar, a left outliner,
a center viewport with floating overlays and a bottom status dock, and a right
inspector with Object / Camera / Output tabs.

The app is named **Lumi**.

## Scope

**In scope (chosen option B):**
- Full editor-screen redesign to match the mockup layout, colors, fonts, and spacing.
- Reorganize `viewer/mod.rs` + `viewer/controls.rs` into focused modules (theme,
  widgets, panels, ui-state).
- Two small backend additions the UI visibly needs:
  - Object **visibility** (`hidden` flag) with an outliner eye toggle.
  - Object **duplicate**.

**Out of scope (deferred):**
- The **Welcome screen** and recent-scenes gallery.
- **Scene save/load** (designed separately in
  `2026-06-29-scene-file-save-load-design.md`). The top-bar scene-name chip is
  display-only; "Save scene" is shown disabled.
- **Bundled sample meshes** (Suzanne/Bunny/Teapot/Dragon) and the **texture
  library** preset grid. Their slots appear in the UI but are disabled with a
  "coming soon" tooltip so the visual structure is preserved.

## Visual language — `viewer/theme.rs`

A single module owns the entire look, installed once at startup (extending the
existing `icons::install`).

### Palette (constants, from the mockup hexes)
- Backgrounds: `BG_APP #0d0e11`, `BG_PANEL #16171b`, `BG_TOPBAR #15161a`,
  `BG_VIEWPORT #08090b`, `FIELD_BG #101116`.
- Borders: `BORDER #23262b`, `BORDER_FIELD #2c2f36`, `BORDER_HOVER #41474e`.
- Text: `TEXT #d8dadf`, `TEXT_STRONG #e7e9ec`, `TEXT_MUTED #8a8f97`,
  `TEXT_DIM #6b7079`.
- Accent `#4d84e6` (+ soft/glow/border alphas); Selection `#ef8a3c`
  (+ soft/border alphas).
- Axis colors: `AXIS_X #c0594f`, `AXIS_Y #5a9e5a`, `AXIS_Z #4f7fc0`.

### Fonts
- Vendor IBM Plex `.ttf` files into `assets/fonts/` (OFL-licensed, committed):
  IBM Plex Sans (Medium + SemiBold) and IBM Plex Mono (Medium).
- Register Sans as the proportional family, Mono as the monospace family, via
  `FontDefinitions`, keeping the Phosphor icon font in the fallback chain so
  icon glyphs still resolve.
- One weight maps per family in egui; use Medium as the workhorse and SemiBold
  where the mockup bolds.

### Style
- One `apply_style(ctx)` sets a dark `Visuals` with the panel fills above,
  ~6–7px widget rounding, 1px borders, selection color = accent, hovered/active
  strokes, and `spacing` (item spacing, button padding, interact size ~30px tall
  to match the mockup field height).
- Net effect: widgets inherit the mockup look for free; components override only
  where they need special treatment (axis letters, mono values).

## Module layout

```
src/viewer/
  mod.rs            // ViewerApp: state + orchestration (much smaller)
  theme.rs          // palette, fonts, style
  icons.rs          // unchanged + a few new glyphs (eye, copy, folder, download, …)
  state.rs          // UiState: pure UI state (not scene data)
  widgets/
    mod.rs          // re-exports
    axis_field.rs   // colored-axis DragValue pill + axis_vec stack
    prop_row.rs     // label | content row, section_header
    card.rs         // card frame, floating-overlay frame
    combo.rs        // styled dropdown matching the mockup pills
    tab_bar.rs      // 3-up pill tab selector + Render/Edit segmented toggle
    buttons.rs      // icon_button, pill_button, swatch
  panels/
    mod.rs
    top_bar.rs      // logo, scene chip, Render/Edit toggle, Save buttons
    outliner.rs     // Scene header, Add-object menu, object rows w/ eye
    viewport.rs     // image/GL host + floating overlays + status dock
    inspector/
      mod.rs        // tab bar + dispatch
      object.rs     // selected object: material, transform, geometry
      camera.rs     // View + Lens
      output.rs     // Resolution + Quality
```

### `UiState` (`state.rs`)
Groups everything that is UI, not scene: `mode: Mode`, `selected: Option<usize>`,
`inspector_tab: Tab`, `add_menu_open: bool`, gizmo settings (`gizmo_local`,
`gizmo_modes`), `last_interact`, and any texture-library UI state. This is the
key cleanup — today these are scattered as `ViewerApp` fields.

### Panel-as-function contract
Each panel is a free function taking `&mut Ui`, the `&mut UiState` it needs, and
`&mut Scene` (or a narrower slice). It returns a small result — typically a
`bool dirty` (render needs restarting) or a tiny action enum (e.g. `SaveImage`,
`ResetCamera`) that `mod.rs` applies centrally.

`ViewerApp::ui` becomes: pump render → pull frame/texture → lay out panels in
order → apply returned actions/dirty. No panel reaches into render-task
internals; `mod.rs` stays the single owner of `RenderTask`, the texture, and the
GL renderer. Each file stays focused (~50–150 lines).

## The four panel zones

Layout order inside `ViewerApp::ui`: top bar (`TopBottomPanel::top`), left
outliner (`SidePanel::left`, 286px fixed), right inspector (`SidePanel::right`,
342px fixed), then `CentralPanel` for the viewport.

### Top bar (height 54, fill `BG_TOPBAR`, bottom border)
- Left: accent rounded-square logo glyph + "Lumi" wordmark (Sans).
- Scene-name chip: folder icon + scene name in mono + chevron — **display-only**
  (save/load not built).
- Center: **Render / Edit** segmented pill toggle (reuses today's mode switch,
  restyled; accent fill on the active half).
- Right: "Save scene" (disabled, "coming soon" tooltip) + "Save image" (wired to
  the existing PNG save).

### Left outliner (`SidePanel::left`, fill `BG_PANEL`)
- Header: layers icon + "SCENE" (uppercase, letter-spaced, muted) + object count
  in mono.
- **Add object** button → popup menu (egui `popup`/`Area`): Primitives
  (Plane/Box/Sphere), Sample meshes (disabled w/ tooltip), "Import .obj…" (wired,
  native only).
- Scrollable object rows: tinted type icon + name + **eye** visibility toggle.
  Selected row gets selection-soft fill + inset border, name turns
  selection-orange. Click selects (and switches the inspector to the Object tab).

### Center viewport (`CentralPanel`, fill `BG_VIEWPORT`)
- Image (Render) or GL preview (Edit) — unchanged rendering logic, rehosted here.
- **Floating overlays** via painter/Area with the translucent dark frame:
  resolution badge (top-left), Move/Rotate/Scale + Local-axes toolbar
  (top-center, Edit only — replaces today's top gizmo panel), Reset-camera chip
  (bottom-left).
- **Status dock** at the bottom of the central panel (thin progress line + ~60px
  row): status dot + "Done / Rendering… / Editing" + passes/elapsed in mono, and
  right-aligned **editable** Samples / Bounces fields + restart button. Replaces
  the progress bar currently in the left panel.

### Right inspector (`SidePanel::right`, fill `BG_PANEL`)
- Top: 3-up pill **tab bar** — Object / Camera / Output.
- **Object tab**: header (icon + editable name + type badge + duplicate + delete)
  → Material → Transform → Geometry (primitives only). Empty state ("No object
  selected") when nothing is picked.
- **Camera tab**: View (Position/Target/Roll) + Lens (FOV/DoF/Focus) + Reset
  camera.
- **Output tab**: Resolution (Width/Height) + Quality (Samples/Max bounces) +
  Format (display-only "PNG · 16-bit").

The actual editing widgets reuse the logic already in `controls.rs`
(material/texture/transform/camera editors), reorganized into the tab modules and
restyled via the new widgets.

## Reusable number field

The most-repeated atom: a styled "pill" with a colored axis letter (X red /
Y green / Z blue) on the left and the value centered in mono, a 1px border, no
visible spinner arrows. Built as `axis_field`, wrapping egui `DragValue` so it
**keeps drag-to-scrub** and click-to-type. The Samples/Bounces dock fields and
the Output/Camera scalar fields use the same widget (without an axis letter where
not applicable). Both the dock and Output tab edit the same
`camera.samples`/`camera.max_depth`, staying in sync.

## Model changes

### Visibility
- Add `pub hidden: bool` to `ObjectSpec` (default `false`).
- `build_world` skips objects where `hidden`, and skips registering them as
  lights.
- The GL `SceneRenderer::paint` also skips hidden objects (Edit preview matches).
- The outliner eye toggle flips it; toggling **invalidates** the render.
- Every `ObjectSpec` constructor sets `hidden: false` — `default_sphere`,
  `default_box`, `from_obj`, and all `src/scenes/*` builders. Grep to confirm none
  are missed.

### Duplicate
- `ObjectSpec` already derives `Clone` and meshes hold `Arc`s, so duplicate is
  cheap (shares the BVH).
- Insert `objects[i].clone()` after index `i`, append " copy" to its name, select
  the new one, mark dirty. Lives behind the inspector duplicate button.

## Behavior & interaction

- **Render/Edit modes** keep today's semantics: path trace runs in Render, pauses
  in Edit (GL preview), resumes on switch-back. The toggle moves to the top bar.
  The dock's status text reflects mode + render state.
- **Auto-render**: no manual Render button; switching to Render resumes/restarts
  the trace as today. The dock restart button = today's `invalidate()`.
- **Selection** is shared `UiState.selected`, driven by both the outliner (click
  row) and the viewport (click-to-pick in Edit); the Object tab follows it.
  Selecting in the outliner also switches the inspector to the Object tab.
- **Dirty tracking** unchanged: panel functions return `dirty`; `mod.rs` calls
  `render.invalidate()` once per frame if anything changed. Visibility toggle and
  duplicate set dirty.
- **Gizmo toolbar** (Move/Rotate/Scale/Local) moves from the old top panel into
  the viewport floating overlay (Edit only), driving the same
  `gizmo_modes`/`gizmo_local` state.
- **Responsiveness**: side panels fixed width (286 / 342), center flexes — resize
  grows the viewport. No fixed-size scaler (that's a mockup-rendering artifact).
- **WASM parity**: native-only bits (OBJ import, image file dialog, Save image /
  scene) render disabled with a tooltip in the browser, as today.

## Testing

- No tests for pure layout.
- The two model changes get coverage:
  - `build_world` excludes a `hidden` object (and doesn't register it as a light).
  - Duplicate inserts the clone after the original with a suffixed name.
- Existing render/scene tests must still pass.
```
