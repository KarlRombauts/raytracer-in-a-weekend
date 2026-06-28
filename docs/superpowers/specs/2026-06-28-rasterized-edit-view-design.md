# Rasterized Edit-Mode View + Transform Gizmo — Design

Date: 2026-06-28

## Goal

Replace Edit mode's path-traced preview with a real-time **rasterized 3D view**
rendered in the egui viewport (via the glow/OpenGL backend), and overlay a
**transform gizmo** so the selected object can be translated / rotated / scaled
directly in the view. Posing the scene becomes instant; switching to Render
path-traces the final image. Designed to keep working under WASM (WebGL2).

## Background / constraints

- App is **eframe/egui** on the **glow (OpenGL)** backend.
- `transform-gizmo-egui` (the chosen gizmo crate) has no released version for
  egui 0.35; its latest (0.9.0) targets **egui 0.34**. The whole 3D ecosystem
  is still on 0.34. Therefore this work **downgrades the app to eframe/egui
  0.34** as a prerequisite (see Phase 0).
- 3D-in-egui is done with **raw `egui_glow::CallbackFn` paint callbacks** plus a
  small hand-written shader — first-party, no heavy deps, WASM-friendly. `three-d`
  was rejected (manages its own window, targets egui 0.34, no paint-callback path).
- WASM: the glow backend targets **WebGL2** on web, and paint callbacks work
  there; shaders must carry per-target `#version` headers (`300 es` for WebGL2)
  and stay within the GLES-3.0 feature set. The existing path tracer (rayon) is
  the wasm-hard part and is out of scope here.

## Scope

In scope:

- Rasterized preview of **all** objects: editable primitives (sphere, quad,
  box) **and** imported meshes (the dragon) — scope B.
- Lit shading: headlight Lambert with an ambient floor.
- Transform gizmo editing the selected object's transform.
- The eframe/egui 0.34 downgrade (prerequisite).

Out of scope (YAGNI / later):

- Path tracing on WASM (rayon threads).
- Textures/materials beyond flat albedo + emissive color in the preview.
- Shadows, reflections, or any global illumination in the preview.
- Editing camera via the gizmo (camera stays driven by orbit/pan/dolly).

## Phase 0 — Prerequisite: eframe/egui 0.34 downgrade

Independent, landed and verified before the feature work.

- Pin `eframe`/`egui` (and any egui-adjacent deps) to `0.34` in `Cargo.toml`.
- Revert the viewer's `impl eframe::App` from 0.35's
  `fn ui(&mut self, ui: &mut egui::Ui, _frame)` back to 0.34's
  `fn update(&mut self, ctx: &egui::Context, _frame)`.
- Change panel construction from `Panel::…show(ui, …)` to
  `egui::SidePanel::left(…)/CentralPanel::default()…show(ctx, …)`.
- Anywhere using the root `ui` (status bar, central image, mode arms) moves
  inside the panel closures driven by `ctx`.
- Verify: app builds and runs, orbit/roll/reset and the reduced-res preview
  still work.
- Update the `eframe-035-api-notes` memory to reflect the revert to 0.34.

## Components

### RenderMesh — render geometry retention

```rust
/// Flat-shaded triangle soup for the rasterizer: 3 vertices per triangle, each
/// carrying the triangle's face normal. Separate from the `dyn Intersect` BVH
/// used for tracing.
pub struct RenderMesh {
    pub positions: Vec<[f32; 3]>, // 3 * num_triangles, object-local space
    pub normals: Vec<[f32; 3]>,   // parallel to positions; face normal per vert
}
```

- `Shape::Mesh` retains an `Arc<RenderMesh>` alongside its `Arc<dyn Intersect>`.
  The OBJ loader builds it from its existing `verts`/`faces` (face normal =
  normalized cross of the two edges).
- Primitives produce a `RenderMesh` on demand: sphere → UV sphere (configurable
  rings/segments), box → 12 triangles, quad → 2 triangles.
- Dragon (~100k tris → ~300k verts) is uploaded once; not regenerated per frame.

### Camera matrices — `viewer/raster/camera_gl.rs`

- Build glam `Mat4` `view` and `projection` from `CameraConfig`:
  - `view = Mat4::look_at_rh(look_from, look_at, up)` where `up` is `v_up`
    rotated about the forward axis by `roll` (same convention as `Camera::from`).
  - `projection = Mat4::perspective_rh_gl(fov_y, aspect, near, far)` (GL/WebGL2
    NDC z ∈ [−1, 1]) with
    `fov_y = fov.to_radians()` (CameraConfig `fov` is vertical), `aspect =
    image_width / image_height`. `near`/`far` bound the scene.
- Must match the path tracer's framing so the view doesn't jump when toggling
  Render/Edit.

### GL renderer — `viewer/raster/renderer.rs`

- Created from eframe's `glow` context (`cc.gl`), stored in the app.
- Owns: one shader program; a per-object cache of `glow` VAO/VBO built from each
  object's `RenderMesh`. The cache is keyed to a geometry generation counter and
  rebuilt only when the object set / mesh geometry changes (reuse the existing
  scene-dirty signal).
- Shader: vertex transforms position by `proj * view * model` and passes the
  world-space normal; fragment computes
  `albedo * (AMBIENT + (1.0 - AMBIENT) * max(dot(N, V), 0.0))`, with emissive
  objects drawn at full emit color. `AMBIENT ≈ 0.25`. Per-target `#version`
  header (`330` desktop / `300 es` WebGL2).
- `paint(gl, &scene, &camera, viewport)` — set per-object `model` + `color`
  uniforms and `view`/`projection`; depth test on; draw each object. Runs inside
  the `egui_glow::CallbackFn`.

### Edit-mode integration — `viewer/mod.rs`

- In Edit mode, paint an `egui::PaintCallback` wrapping
  `egui_glow::CallbackFn` that invokes the renderer over the viewport rect,
  instead of the path-traced texture.
- Orbit/pan/dolly drive the shared `scene.camera` and request a repaint; the GL
  view is instant, so Edit mode **does not restart the path trace** while
  interacting. The reduced-resolution path-trace preview becomes vestigial in
  Edit mode (kept or removed as a cleanup; it still has no role in Render mode).
- Render mode is unchanged: path-traced texture with 2D pan/zoom.
- Switching Edit → Render triggers a path-trace render at the current camera.

### Transform gizmo — `viewer/raster/gizmo.rs`

- When an object is selected in Edit mode, overlay `transform-gizmo-egui` fed
  the same `view`/`projection` (converted to the gizmo's `mint` types) and the
  selected object's current transform.
- Translate / rotate / scale; the gizmo result is written back to
  `scene.objects[selected].transform`, marking the scene dirty so the next path
  trace reflects it.
- The gizmo yields a TRS with a **quaternion**; `ObjectSpec.transform` stores
  **Euler degrees + scale + translate**. Convert quat → Euler (fixed order) and
  reconcile the gizmo pivot with the existing "rotate/scale about the object's
  bbox center, then translate" convention. This is the fiddliest part and is
  prototyped/tested carefully.

## Data flow

```
Edit frame:
  scene.camera --> camera_gl: view, projection (glam)
  PaintCallback(egui_glow) --> renderer.paint:
      for each object: model matrix + color uniforms; draw cached VAO
  gizmo overlay on selected object (view/projection as mint, current transform)
      --> on interact: updated transform --> scene.objects[selected].transform
                                          --> scene dirty (for next path trace)
Geometry buffers rebuild only when the object set / mesh geometry changes.
```

## Dependencies

- `glam` — matrices and gizmo interop (enable its `mint` feature).
- `transform-gizmo-egui = "0.9"` — gizmo (egui 0.34).
- `mint` — gizmo math interop (via glam's mint conversions).
- `egui_glow` / `glow` — already provided by eframe's glow backend.

## Testing

Unit-testable (pure logic):

- Camera matrices: a known world point projects to the expected NDC/clip
  coordinates; `view` places `look_from` at the origin looking down −Z.
- Tessellation: UV sphere vertex/triangle counts; box = 12 triangles; all
  generated normals are unit length; quad = 2 triangles.
- Gizmo transform reconciliation: quat → Euler-degrees round-trips for
  representative rotations; a gizmo translate/scale maps to the expected
  `ObjectSpec.transform` fields.

Manual / visual:

- GL preview renders primitives + dragon, shaded, matching the path-traced
  framing when toggling Render/Edit.
- Gizmo translates/rotates/scales the selected object; the change persists into
  the path-traced Render result.
- Builds and runs; (later) loads under WASM/WebGL2.
