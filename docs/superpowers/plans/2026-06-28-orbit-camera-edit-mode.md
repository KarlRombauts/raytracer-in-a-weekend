# Orbit Camera & Edit Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an interactive Edit mode to the viewer where dragging orbits/pans/dollies the camera, plus a roll slider in the side panel.

**Architecture:** A new `orbit.rs` holds pure camera math operating on `CameraConfig`. `ViewerApp` gains a Render/Edit mode; Edit-mode gestures mutate `scene.camera` and call the existing `render.invalidate()`. Roll is a new `CameraConfig` field applied when `Camera` builds its basis, using a Rodrigues helper on `Vec3`.

**Tech Stack:** Rust, eframe/egui 0.35, existing `Vec3` math and `typed-builder` `CameraConfig`.

## Global Constraints

- Orbit state is **stateless / derived from the live camera** each gesture — no stored yaw/pitch/radius. (Spec: Camera math.)
- Elevation clamped to ±89°; `v_up` stays the world-up reference and is never rewritten by orbit. (Spec.)
- Reset is a **button**, never a double-click. (Spec.)
- Roll is an absolute `roll: f32` (degrees) on `CameraConfig`, default `0.0`, applied in `Camera::from`. (Spec.)
- Follow existing viewer patterns: side panel built via the existing `egui::Panel::left(...)` closure; central panel via `egui::CentralPanel::default()`. Do not restructure panel construction.
- All `CameraConfig` construction is via the builder; the new field MUST have a builder default so existing scenes keep compiling.
- Tests run with `cargo test`; the crate is a binary, so unit tests live in `#[cfg(test)] mod tests` inside each module file.

---

### Task 1: Rodrigues axis rotation on `Vec3`

**Files:**
- Modify: `src/vec3.rs` (add method in the `impl Vec3` block, ~line 130 area)
- Test: `src/vec3.rs` (`#[cfg(test)] mod` at end of file)

**Interfaces:**
- Produces: `Vec3::rotate_about_axis(&self, axis: &Vec3, angle_rad: f32) -> Vec3` — rotates `self` around `axis` (assumed unit length) by `angle_rad`, right-handed.

- [ ] **Step 1: Write the failing test**

Add at the end of `src/vec3.rs`:

```rust
#[cfg(test)]
mod orbit_rotate_tests {
    use super::Vec3;
    use std::f32::consts::PI;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn rotates_x_to_y_about_z() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        let r = x.rotate_about_axis(&z, PI / 2.0);
        assert!(close(r, Vec3::new(0.0, 1.0, 0.0)), "got {:?}", r);
    }

    #[test]
    fn rotation_about_own_axis_is_noop() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let r = z.rotate_about_axis(&z, 1.234);
        assert!(close(r, z), "got {:?}", r);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test orbit_rotate_tests 2>&1 | tail -20`
Expected: FAIL — `no method named rotate_about_axis found`.

- [ ] **Step 3: Write minimal implementation**

Inside `impl Vec3` (e.g. just after `unit`, around line 137), add:

```rust
    /// Rotate `self` about `axis` (assumed unit length) by `angle_rad`,
    /// right-handed (Rodrigues' rotation formula).
    pub fn rotate_about_axis(&self, axis: &Vec3, angle_rad: f32) -> Vec3 {
        let (sin, cos) = angle_rad.sin_cos();
        *self * cos + axis.cross(self) * sin + *axis * (axis.dot(self) * (1.0 - cos))
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test orbit_rotate_tests 2>&1 | tail -20`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/vec3.rs
git commit -m "feat: add Vec3::rotate_about_axis (Rodrigues)"
```

---

### Task 2: Camera roll

**Files:**
- Modify: `src/camera/config.rs` (add field, ~line 18 area)
- Modify: `src/camera/camera.rs:52-54` (apply roll in basis)
- Modify: `src/viewer/controls.rs` (add roll slider in `camera_controls`, ~line 84)
- Test: `src/camera/camera.rs` (`#[cfg(test)] mod` at end of file)

**Interfaces:**
- Consumes: `Vec3::rotate_about_axis` (Task 1).
- Produces: `CameraConfig.roll: f32` (degrees, default `0.0`). `Camera::from` rotates the up reference about the forward axis by `roll` before deriving the right vector.

- [ ] **Step 1: Write the failing test**

Add at the end of `src/camera/camera.rs`:

```rust
#[cfg(test)]
mod roll_tests {
    use super::Camera;
    use crate::camera::CameraConfig;
    use crate::vec3::Vec3;

    fn cfg(roll: f32) -> CameraConfig {
        CameraConfig::builder()
            .look_from(Vec3::new(0.0, 0.0, 0.0))
            .look_at(Vec3::new(0.0, 0.0, -1.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .roll(roll)
            .build()
    }

    #[test]
    fn zero_roll_keeps_upright_basis() {
        // With no roll, the right axis u should be world +x (within sign tol).
        let cam = Camera::from(cfg(0.0));
        assert!((cam.basis_u().x.abs() - 1.0).abs() < 1e-5, "u={:?}", cam.basis_u());
        assert!(cam.basis_u().y.abs() < 1e-5);
    }

    #[test]
    fn ninety_roll_tilts_right_axis_to_vertical() {
        // Rolling 90° should swing the right axis onto the world vertical.
        let cam = Camera::from(cfg(90.0));
        assert!(cam.basis_u().y.abs() > 0.99, "u={:?}", cam.basis_u());
    }
}
```

This test needs read access to the basis. Add a tiny test-only accessor in the same file (outside the test module), inside `impl Camera` — it reads a `basis_u` field that Step 3 adds:

```rust
    #[cfg(test)]
    pub(crate) fn basis_u(&self) -> crate::vec3::Vec3 {
        self.basis_u
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test roll_tests 2>&1 | tail -25`
Expected: FAIL to compile — `no method named roll` on the builder and `no field basis_u`; both are added in Step 3.

- [ ] **Step 3: Implement roll**

In `src/camera/config.rs`, add the field after `fov` (keep the existing `#[derive(TypedBuilder, Clone)]`):

```rust
    #[builder(default = 0.0)]
    pub roll: f32,
```

In `src/camera/camera.rs`, replace the basis lines `52-54`:

```rust
        let w = (config.look_from - config.look_at).unit();
        let u = config.v_up.cross(&w).unit();
        let v = w.cross(&u);
```

with:

```rust
        let w = (config.look_from - config.look_at).unit();
        // Roll spins the up reference about the view axis before deriving right.
        let up = config.v_up.rotate_about_axis(&w, config.roll.to_radians());
        let u = up.cross(&w).unit();
        let v = w.cross(&u);
```

Add a `basis_u` field so the test (and future code) can read the right axis. In the `Camera` struct definition add `basis_u: Vec3,` near `dof_disk_u`; in the returned `Camera { .. }` literal add `basis_u: u,`. The `basis_u()` accessor from Step 1 now resolves.

> The field is otherwise unused for now, which will emit a dead-code warning. That's acceptable (it's read by tests); the future rasterizer will use the basis too. Annotate with `#[allow(dead_code)]` if you want it silent.

- [ ] **Step 4: Add the roll slider**

In `src/viewer/controls.rs`, inside `camera_controls`'s `egui::Grid` closure, after the `fov` row, add:

```rust
            ui.label("roll");
            c |= ui
                .add(egui::Slider::new(&mut cam.roll, -180.0..=180.0).suffix("°"))
                .changed();
            ui.end_row();
```

- [ ] **Step 5: Run tests + build**

Run: `cargo test roll_tests 2>&1 | tail -25`
Expected: PASS (2 tests).
Run: `cargo build 2>&1 | grep -E "error" | grep -v "scenes/(cornell_box|new_bvh)" || echo "no new errors"`
Expected: `no new errors` (the two pre-existing `crate::scene` errors in WIP scene files are unrelated).

- [ ] **Step 6: Commit**

```bash
git add src/camera/config.rs src/camera/camera.rs src/viewer/controls.rs
git commit -m "feat: add camera roll field, basis application, and slider"
```

---

### Task 3: Orbit / pan / dolly math (`orbit.rs`)

**Files:**
- Create: `src/viewer/orbit.rs`
- Modify: `src/viewer/mod.rs` (add `mod orbit;` near the other `mod` lines)
- Test: `src/viewer/orbit.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces, all operating in place on the live camera:
  - `orbit::orbit(cam: &mut CameraConfig, delta: egui::Vec2)`
  - `orbit::pan(cam: &mut CameraConfig, delta: egui::Vec2)`
  - `orbit::dolly(cam: &mut CameraConfig, scroll: f32)`
- Consumes: `CameraConfig` fields `look_from`, `look_at`, `v_up` (`Vec3`).

- [ ] **Step 1: Write the failing tests**

Create `src/viewer/orbit.rs`:

```rust
//! Pure camera-manipulation math for Edit mode. Each function mutates the live
//! `CameraConfig` in place, deriving its working state from `look_from`/
//! `look_at` each call — no stored orbit state to drift out of sync.

use eframe::egui;

use crate::camera::CameraConfig;
use crate::vec3::Vec3;

const ORBIT_SENS: f32 = 0.005; // radians per pixel
const PAN_SENS: f32 = 0.0015; // world units per pixel, per unit distance
const DOLLY_SENS: f32 = 0.001; // per scroll unit
const MIN_RADIUS: f32 = 0.05;
const MAX_ELEVATION: f32 = 89.0; // degrees

/// Orbit `look_from` around `look_at`. Horizontal drag changes azimuth,
/// vertical drag changes elevation (clamped to ±89°). `v_up` is untouched.
pub fn orbit(cam: &mut CameraConfig, delta: egui::Vec2) {
    let offset = cam.look_from - cam.look_at;
    let radius = offset.length();
    if radius < 1e-6 {
        return;
    }
    let mut azimuth = offset.z.atan2(offset.x);
    let mut elevation = (offset.y / radius).clamp(-1.0, 1.0).asin();

    azimuth -= delta.x * ORBIT_SENS;
    elevation += delta.y * ORBIT_SENS;
    let max_el = MAX_ELEVATION.to_radians();
    elevation = elevation.clamp(-max_el, max_el);

    let new = Vec3::new(
        radius * elevation.cos() * azimuth.cos(),
        radius * elevation.sin(),
        radius * elevation.cos() * azimuth.sin(),
    );
    cam.look_from = cam.look_at + new;
}

/// Pan: slide both `look_from` and `look_at` along the camera's right/up axes,
/// scaled by distance so it feels roughly 1:1. View direction is preserved.
pub fn pan(cam: &mut CameraConfig, delta: egui::Vec2) {
    let forward = cam.look_at - cam.look_from;
    let dist = forward.length();
    if dist < 1e-6 {
        return;
    }
    let w = forward / dist;
    let right = w.cross(&cam.v_up).unit();
    let up = right.cross(&w);
    let scale = dist * PAN_SENS;
    let translate = right * (-delta.x * scale) + up * (delta.y * scale);
    cam.look_from = cam.look_from + translate;
    cam.look_at = cam.look_at + translate;
}

/// Dolly: move `look_from` toward/away from `look_at`. Positive scroll moves in.
pub fn dolly(cam: &mut CameraConfig, scroll: f32) {
    let offset = cam.look_from - cam.look_at;
    let radius = offset.length();
    if radius < 1e-6 {
        return;
    }
    let new_radius = (radius * (-scroll * DOLLY_SENS).exp()).max(MIN_RADIUS);
    cam.look_from = cam.look_at + offset * (new_radius / radius);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> CameraConfig {
        CameraConfig::builder()
            .look_from(Vec3::new(0.0, 0.0, 5.0))
            .look_at(Vec3::new(0.0, 0.0, 0.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .build()
    }

    fn radius(c: &CameraConfig) -> f32 {
        (c.look_from - c.look_at).length()
    }

    #[test]
    fn orbit_zero_delta_is_noop() {
        let mut c = cam();
        let before = c.look_from;
        orbit(&mut c, egui::Vec2::ZERO);
        assert!((c.look_from - before).length() < 1e-4);
    }

    #[test]
    fn orbit_preserves_radius() {
        let mut c = cam();
        orbit(&mut c, egui::vec2(120.0, 40.0));
        assert!((radius(&c) - 5.0).abs() < 1e-3, "radius={}", radius(&c));
    }

    #[test]
    fn orbit_clamps_elevation() {
        let mut c = cam();
        // Huge upward drag must not flip past the pole.
        orbit(&mut c, egui::vec2(0.0, 100_000.0));
        let offset = c.look_from - c.look_at;
        let elevation = (offset.y / radius(&c)).asin().to_degrees();
        assert!(elevation <= 89.0 + 1e-2 && elevation >= 88.9, "elev={}", elevation);
    }

    #[test]
    fn dolly_in_shrinks_radius_and_respects_min() {
        let mut c = cam();
        dolly(&mut c, 500.0); // strong zoom-in
        assert!(radius(&c) < 5.0);
        assert!(radius(&c) >= 0.05 - 1e-6, "radius={}", radius(&c));
    }

    #[test]
    fn pan_is_perpendicular_and_preserves_view_direction() {
        let mut c = cam();
        let dir_before = (c.look_at - c.look_from).unit();
        let from_before = c.look_from;
        pan(&mut c, egui::vec2(50.0, 0.0));
        let dir_after = (c.look_at - c.look_from).unit();
        // View direction unchanged.
        assert!((dir_after - dir_before).length() < 1e-4);
        // Movement perpendicular to the view direction.
        let moved = c.look_from - from_before;
        assert!(moved.dot(&dir_before).abs() < 1e-4, "dot={}", moved.dot(&dir_before));
        assert!(moved.length() > 1e-4);
    }
}
```

- [ ] **Step 2: Register the module and run tests to verify they fail**

In `src/viewer/mod.rs`, add `mod orbit;` alongside the existing `mod controls;` / `mod render_task;` / `mod view_transform;` lines.

Run: `cargo test viewer::orbit 2>&1 | tail -30`
Expected: PASS actually — the implementation is already in the file from Step 1. (TDD note: code and tests were written together here because they live in one new file; if you want a true red phase, comment out the three function bodies, run to see failures, then restore.)

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test viewer::orbit 2>&1 | tail -30`
Expected: PASS (5 tests).

- [ ] **Step 4: Commit**

```bash
git add src/viewer/orbit.rs src/viewer/mod.rs
git commit -m "feat: add orbit/pan/dolly camera math"
```

---

### Task 4: Wire Render/Edit mode into the viewer

**Files:**
- Modify: `src/viewer/mod.rs` (`ViewerApp` struct + `new` + side panel + central panel)
- Verify: build + manual interaction (no unit test — egui wiring).

**Interfaces:**
- Consumes: `orbit::{orbit, pan, dolly}` (Task 3); `CameraConfig.roll` slider (Task 2, already wired in `controls`).
- Produces: a `Mode { Render, Edit }` toggle, an `initial_camera` snapshot, a `Reset camera` button, and Edit-mode gesture dispatch.

- [ ] **Step 1: Add the mode enum and fields**

In `src/viewer/mod.rs`, above `struct ViewerApp`, add:

```rust
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Render,
    Edit,
}
```

Add two fields to `struct ViewerApp`:

```rust
    mode: Mode,
    initial_camera: crate::camera::CameraConfig,
```

In `ViewerApp::new`, capture the startup camera before moving `scene` into the `Arc` and set the default mode. Replace the body so it reads:

```rust
    fn new(cc: &eframe::CreationContext<'_>, scene: Scene, width: u32, height: u32) -> Self {
        let total = scene.camera.samples;
        let initial_camera = scene.camera.clone();
        let scene = Arc::new(Mutex::new(scene));
        let render = RenderTask::spawn(cc.egui_ctx.clone(), scene.clone(), width, height, total);

        ViewerApp {
            scene,
            render,
            width,
            height,
            texture: None,
            shown_pass: 0,
            view: ViewTransform::new(),
            mode: Mode::Render,
            initial_camera,
        }
    }
```

- [ ] **Step 2: Add the mode toggle + reset button to the side panel**

In the side panel closure (the `egui::Panel::left("controls").show(ui, |ui| { ... })` block), immediately after the status `ui.horizontal(...)` and its following `ui.separator()`, insert:

```rust
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.mode, Mode::Render, "Render");
                        ui.selectable_value(&mut self.mode, Mode::Edit, "Edit");
                        if ui.button("Reset camera").clicked() {
                            scene.camera = self.initial_camera.clone();
                            self.render.invalidate();
                        }
                    });
                    ui.separator();
```

> This compiles because `scene` is a guard from `self.scene.clone()` (a separate Arc), leaving `self.mode` / `self.render` / `self.initial_camera` free to borrow inside the same closure.

- [ ] **Step 3: Branch the central panel on mode**

In the `egui::CentralPanel::default().show(ui, |ui| { ... })` closure, after `let response = ui.allocate_rect(vp, egui::Sense::click_and_drag());` and `let aspect = ...;`, replace the existing drag/double-click/scroll block with a match on mode. The Render arm keeps today's behavior; the Edit arm drives the camera:

```rust
            match self.mode {
                Mode::Render => {
                    // Drag to pan; double-click to reset the 2D view.
                    if response.dragged() {
                        self.view.pan_by(response.drag_delta());
                    }
                    if response.double_clicked() {
                        self.view.reset();
                    }
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if response.hovered() && scroll != 0.0 {
                        let cursor = ui
                            .input(|i| i.pointer.hover_pos())
                            .unwrap_or_else(|| vp.center());
                        self.view.zoom_at(vp, aspect, cursor, scroll);
                    }
                }
                Mode::Edit => {
                    let mut changed = false;
                    if response.dragged() {
                        let shift = ui.input(|i| i.modifiers.shift);
                        let mut scene = self.scene.lock().unwrap();
                        if shift {
                            orbit::pan(&mut scene.camera, response.drag_delta());
                        } else {
                            orbit::orbit(&mut scene.camera, response.drag_delta());
                        }
                        changed = true;
                    }
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if response.hovered() && scroll != 0.0 {
                        orbit::dolly(&mut self.scene.lock().unwrap().camera, scroll);
                        changed = true;
                    }
                    if changed {
                        self.render.invalidate();
                    }
                }
            }
```

The texture paint (`let rect = self.view.image_rect(vp, aspect);` … `ui.painter_at(vp).image(...)`) stays unchanged below the match — both modes show the path-traced texture.

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | grep -E "error" | grep -v "scenes/(cornell_box|new_bvh)" || echo "no new errors"`
Expected: `no new errors`.

- [ ] **Step 5: Manual verification**

Run the viewer on a working scene (the existing `main.rs` entry point):

Run: `cargo run --release 2>&1 | tail -5`

Check:
- Side panel shows a **Render / Edit** toggle and a **Reset camera** button.
- In **Edit**: drag orbits the view; shift+drag pans; scroll dollies in/out; the image re-renders (noisy then converging) when you pause.
- **Reset camera** snaps back to the startup view.
- The **roll** slider tilts the rendered image.
- In **Render**: drag/zoom still pan/zoom the 2D image as before.

- [ ] **Step 6: Commit**

```bash
git add src/viewer/mod.rs
git commit -m "feat: add Render/Edit mode toggle with orbit camera controls"
```

---

## Self-Review

- **Spec coverage:** Mode toggle (Task 4) ✓; orbit/shift-pan/dolly (Tasks 3–4) ✓; Reset camera button (Task 4) ✓; roll slider + `CameraConfig.roll` + basis application (Tasks 1–2) ✓; stateless orbit / ±89° clamp / `v_up` untouched (Task 3) ✓; Edit shows path-traced texture, invalidate-driven refresh (Task 4) ✓; out-of-scope items (rasterizer, live-while-drag, arcball) correctly omitted ✓.
- **Placeholder scan:** the only "placeholder" is the deliberate `dof_disk_u` stand-in in Task 2 Step 1, explicitly replaced in Step 3 — flagged in-line, not a loose end.
- **Type consistency:** `orbit`/`pan`/`dolly` take `(&mut CameraConfig, egui::Vec2 | f32)` consistently across Tasks 3 and 4; `rotate_about_axis(&self, axis: &Vec3, angle_rad: f32)` used identically in Tasks 1 and 2; `Mode::{Render, Edit}` consistent in Task 4; `initial_camera: CameraConfig` matches `scene.camera` type.
```
