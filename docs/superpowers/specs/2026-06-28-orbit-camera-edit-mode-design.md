# Orbit Camera & Edit Mode — Design

Date: 2026-06-28

## Goal

Add an interactive **Edit mode** to the viewer that lets you reposition the
camera by dragging in the viewport — orbit, pan, and dolly around the scene —
instead of only typing numbers into the side panel. Edit mode is deliberately
built as the seam where a real-time rasterized preview will later live; for now
it drives the existing path tracer.

## Scope

In scope:

- A **Render / Edit** mode toggle in the viewer.
- Edit-mode mouse gestures: drag = orbit, shift+drag = pan, scroll = dolly.
- A **Reset camera** button (side panel) restoring the camera to its startup state.
- A **roll** slider in the side-panel camera controls, backed by a new
  `roll` field on `CameraConfig`.

Out of scope (YAGNI, easy to add later):

- The rasterized preview itself — Edit mode keeps showing the path-traced texture.
- Live-while-dragging refresh (the rasterizer is what fixes that gap).
- Trackball/arcball free rotation.

## Behavior

### Modes

- **Render mode** — unchanged from today. Path-traced image with 2D pan / zoom;
  double-click resets the 2D *view* (zoom/pan), not the camera. Converges and
  saves `test.png`.
- **Edit mode** — dragging manipulates `scene.camera`:
  - **drag** → orbit `look_from` around `look_at`
  - **shift + drag** → pan (translate `look_from` and `look_at` together along
    the camera's right/up axes)
  - **scroll** → dolly (change `|look_from − look_at|`)

  Each gesture mutates `scene.camera` and calls `render.invalidate()`. The
  displayed texture is still the path-traced output, so it refreshes once a pass
  completes (noisy-then-converging when you pause). There is **no** double-click
  reset in Edit mode.

A mode toggle (Render / Edit) sits at the top of the side panel near the status
line. Mode lives in `ViewerApp`.

### Camera math

Orbit uses the **stateless / derived-from-camera** approach: each gesture reads
the live `look_from`/`look_at`, converts the offset to spherical coordinates
(azimuth, elevation, radius) relative to `look_at`, applies the delta, and
writes `look_from` back. No duplicate orbit state to keep in sync with
side-panel edits.

- Elevation is clamped to ±89° so the view never flips through the pole.
- `v_up` stays the world-up reference (0, 1, 0); orbit does not rewrite it.
- Pan translates both `look_from` and `look_at` along the camera's right/up
  axes, scaled by the current distance so it feels roughly 1:1.
- Dolly scales the radius (multiplicative, e.g. `radius *= exp(scroll * k)`),
  with a sensible minimum so you can't pass through / invert at the target.

### Reset camera

A **Reset camera** button in the side panel sets `scene.camera =
initial_camera` (captured once at startup in `ViewerApp::new`) and invalidates
the render.

### Roll

- New field `roll: f32` (degrees, default `0.0`) on `CameraConfig`.
- Applied in `Camera::from` basis construction: rotate `v_up` about the forward
  axis `w` by `roll` before computing `u`:

  ```rust
  let w = (look_from - look_at).unit();
  let up = rotate_about_axis(v_up, w, roll.to_radians());
  let u = up.cross(&w).unit();
  let v = w.cross(&u);
  ```

- A roll slider in `controls::camera_controls` edits `scene.camera.roll` and
  returns `changed`, flowing through the existing `dirty → invalidate` path. No
  new plumbing.

## Components

- **`src/viewer/orbit.rs`** (new) — pure camera math, no egui state:
  - `orbit(cam: &mut CameraConfig, drag_delta: egui::Vec2)`
  - `pan(cam: &mut CameraConfig, drag_delta: egui::Vec2)`
  - `dolly(cam: &mut CameraConfig, scroll: f32)`

  These are unit-testable in isolation (spherical round-trip, elevation clamp,
  dolly minimum, pan direction). egui's `Vec2` is the only UI type they touch.

- **`src/viewer/mod.rs`** — `ViewerApp` gains `mode: Mode { Render, Edit }` and
  `initial_camera: CameraConfig`. The central panel branches on `mode`: Render
  keeps today's pan/zoom/paint path; Edit dispatches drag/shift-drag/scroll to
  the `orbit` functions and invalidates. The side panel renders the mode toggle
  and the Reset camera button.

- **`src/viewer/controls.rs`** — add a roll slider to `camera_controls`.

- **`src/camera/config.rs`** — add `roll: f32` (default `0.0`).

- **`src/camera/camera.rs`** — apply roll in `Camera::from`. Add a small
  rotate-about-axis helper (Rodrigues) if one isn't already available in `vec3`.

## Data flow

```
mouse gesture (Edit mode)
  → orbit::{orbit,pan,dolly}(&mut scene.camera, …)
  → render.invalidate()
  → render thread cancels in-flight pass, rebuilds world from scene snapshot,
    restarts the progressive render
  → UI paints the latest completed pass

roll slider / Reset camera button
  → mutate scene.camera (changed) → dirty → render.invalidate()
```

## Testing

- Unit tests for `orbit.rs`: orbiting by (0,0) is a no-op; elevation clamps at
  ±89°; a full-circle azimuth returns near the start; dolly respects its
  minimum radius; pan moves perpendicular to the view direction.
- Unit test for roll: `Camera::from` with `roll = 0` matches current basis;
  `roll = 90°` rotates `u`/`v` about `w` as expected.
- Manual: toggle to Edit, drag to orbit, shift-drag to pan, scroll to dolly,
  Reset camera button restores the startup view, roll slider tilts the image.
```
