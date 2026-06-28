# Rasterized 3D Edit View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Edit mode's path-traced preview with a real-time rasterized 3D view of the scene (primitives + meshes), drawn in the egui viewport via the glow backend.

**Architecture:** Each scene object gets a `RenderMesh` (flat triangle soup) retained alongside its tracing BVH; a glow shader draws them with a per-object model matrix and Lambert+ambient shading. View/projection come from the shared `CameraConfig` (glam), so Edit (raster) and Render (path-traced) line up. The transform gizmo is a separate follow-on plan.

**Tech Stack:** Rust, eframe/egui **0.34** (glow backend), `glow` GL calls, `egui_glow::CallbackFn`, `glam` matrices.

## Global Constraints

- App runs on **eframe/egui 0.34** (downgraded from 0.35) so the later gizmo crate is usable. Pin `eframe = "0.34"`.
- 3D is drawn with **raw `egui_glow::CallbackFn` paint callbacks** — no `three-d` or other renderer crates.
- Shaders must be **WebGL2-safe**: dual `#version` header (`330` desktop / `300 es` web), GLES-3.0 feature set only.
- Shading: `albedo * (AMBIENT + (1.0 - AMBIENT) * max(dot(N, V), 0.0))`, `AMBIENT = 0.25`; emissive (`DiffuseLight`) objects drawn at full emit color.
- `RenderMesh` is in each shape's **definition space**; the per-object **model matrix** is the `ObjectSpec.transform` composed as `T(translate) · T(c) · R · S · T(-c)` where `c` is the shape's bbox centre — matching `ObjectSpec::build`.
- Projection is `glam::Mat4::perspective_rh_gl` (GL NDC z ∈ [−1,1]); view is `look_at_rh` with `v_up` rolled about the forward axis by `CameraConfig.roll` (matches `Camera::from`).
- Crate is a binary; unit tests live in `#[cfg(test)] mod` inside each module and run via `cargo test`.
- GL rendering and the egui integration cannot be unit-tested in this environment; those tasks are verified by clean build + a described manual check.

---

### Task 1: Downgrade to eframe/egui 0.34

**Files:**
- Modify: `Cargo.toml` (eframe version)
- Modify: `src/viewer/mod.rs` (App trait method + panel construction)
- Modify: `/Users/karl/.claude/projects/-Users-karl-Code-raytracer-in-a-weekend/memory/eframe-035-api-notes.md` and `MEMORY.md`

**Interfaces:**
- Produces: the viewer runs on eframe 0.34 with `impl eframe::App` using `fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame)`, panels built with `egui::SidePanel`/`egui::CentralPanel` `.show(ctx, …)`.

- [ ] **Step 1: Pin eframe 0.34**

In `Cargo.toml`, change the eframe line to:

```toml
eframe = "0.34"
```

- [ ] **Step 2: Port the App impl from 0.35 `ui()` to 0.34 `update()`**

In `src/viewer/mod.rs`, the impl currently is `fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame)` and builds panels with `egui::Panel::left("controls").show(ui, …)` / `egui::CentralPanel::default().show(ui, …)`.

Change the signature to:

```rust
impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
```

Replace the body's first line `let ctx = ui.ctx().clone();` with using the passed `ctx` directly (clone where an owned `egui::Context` is needed). Change the side panel from `egui::Panel::left("controls")…show(ui, |ui| …)` to `egui::SidePanel::left("controls")…show(ctx, |ui| …)`, and the central panel from `egui::CentralPanel::default().show(ui, |ui| …)` to `egui::CentralPanel::default().show(ctx, |ui| …)`. All inner `|ui|` closure bodies stay the same. Anywhere the old code called `ui.ctx()` for repaint requests, use `ctx` directly.

- [ ] **Step 3: Build and run**

Run: `cargo build 2>&1 | grep -E "^error" || echo OK`
Expected: `OK` (no errors). If `egui_glow`/`glow` resolve to new 0.34-matched versions, that's expected.

Run (manual, requires a display): `cargo run --release`
Expected: window opens; Render/Edit toggle, orbit, roll slider, Reset camera, and the reduced-res preview all still work as before.

- [ ] **Step 4: Update the memory note**

Edit `/Users/karl/.claude/projects/-Users-karl-Code-raytracer-in-a-weekend/memory/eframe-035-api-notes.md` to state the app is on **0.34** using `App::update(ctx, frame)` (not 0.35's `App::ui`), and why (gizmo-crate compatibility). Update the one-line pointer in `MEMORY.md` to match.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/viewer/mod.rs
git commit -m "chore: downgrade to eframe/egui 0.34 for gizmo compatibility"
```

---

### Task 2: `RenderMesh` + primitive tessellation

**Files:**
- Create: `src/geometry/render_mesh.rs`
- Modify: `src/geometry/mod.rs` (add `pub mod render_mesh; pub use render_mesh::*;`)
- Test: in `src/geometry/render_mesh.rs`

**Interfaces:**
- Produces:
  - `pub struct RenderMesh { pub positions: Vec<[f32; 3]>, pub normals: Vec<[f32; 3]> }`
  - `RenderMesh::from_triangles(verts: &[Vec3], faces: &[[usize; 3]]) -> RenderMesh`
  - `RenderMesh::sphere(center: Point3, radius: f32, rings: u32, segments: u32) -> RenderMesh`
  - `RenderMesh::unit_box(a: Point3, b: Point3) -> RenderMesh`
  - `RenderMesh::quad(q: Point3, u: Vec3, v: Vec3) -> RenderMesh`
  - Every triangle contributes 3 entries to `positions`/`normals`; each vertex carries the triangle's face normal (flat shading); all normals are unit length.

- [ ] **Step 1: Write the failing tests**

Create `src/geometry/render_mesh.rs`:

```rust
//! Flat-shaded triangle geometry for the rasterized preview, derived from the
//! same shapes the path tracer uses. Three vertices per triangle, each carrying
//! the triangle's face normal. Object-local / definition space — the per-object
//! model matrix applies the transform.

use crate::vec3::{Point3, Vec3};

pub struct RenderMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}

impl RenderMesh {
    fn push_tri(&mut self, a: Point3, b: Point3, c: Point3) {
        let n = (b - a).cross(&(c - a)).unit();
        for p in [a, b, c] {
            self.positions.push([p.x, p.y, p.z]);
            self.normals.push([n.x, n.y, n.z]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_len(n: &[f32; 3]) -> bool {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        (l - 1.0).abs() < 1e-4
    }

    #[test]
    fn box_has_12_triangles_and_unit_normals() {
        let m = RenderMesh::unit_box(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
        assert_eq!(m.positions.len(), 36); // 12 tris * 3
        assert_eq!(m.normals.len(), 36);
        assert!(m.normals.iter().all(unit_len));
    }

    #[test]
    fn quad_has_2_triangles() {
        let m = RenderMesh::quad(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert_eq!(m.positions.len(), 6);
        assert!(m.normals.iter().all(unit_len));
    }

    #[test]
    fn sphere_vertex_count_matches_tessellation() {
        let (rings, segs) = (8, 12);
        let m = RenderMesh::sphere(Point3::new(0.0, 0.0, 0.0), 1.0, rings, segs);
        // Each ring band is `segs` quads = 2 tris; `rings` bands.
        assert_eq!(m.positions.len() as u32, rings * segs * 2 * 3);
        assert!(m.normals.iter().all(unit_len));
        // All sphere vertices lie on the radius.
        assert!(m
            .positions
            .iter()
            .all(|p| ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 1.0).abs() < 1e-3));
    }

    #[test]
    fn from_triangles_expands_faces() {
        let verts = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let faces = vec![[0usize, 1, 2]];
        let m = RenderMesh::from_triangles(&verts, &faces);
        assert_eq!(m.positions.len(), 3);
        assert!(unit_len(&m.normals[0]));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test geometry::render_mesh 2>&1 | tail -20`
Expected: FAIL — `sphere`, `unit_box`, `quad`, `from_triangles` not found.

- [ ] **Step 3: Implement the constructors**

Add to the `impl RenderMesh` block in `src/geometry/render_mesh.rs`:

```rust
    pub fn from_triangles(verts: &[Vec3], faces: &[[usize; 3]]) -> RenderMesh {
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        for &[i, j, k] in faces {
            m.push_tri(verts[i], verts[j], verts[k]);
        }
        m
    }

    pub fn quad(q: Point3, u: Vec3, v: Vec3) -> RenderMesh {
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        m.push_tri(q, q + u, q + u + v);
        m.push_tri(q, q + u + v, q + v);
        m
    }

    pub fn unit_box(a: Point3, b: Point3) -> RenderMesh {
        let (min, max) = (Vec3::min(&a, &b), Vec3::max(&a, &b));
        // 8 corners
        let c = |x: f32, y: f32, z: f32| Point3::new(x, y, z);
        let corners = [
            c(min.x, min.y, min.z), c(max.x, min.y, min.z),
            c(max.x, max.y, min.z), c(min.x, max.y, min.z),
            c(min.x, min.y, max.z), c(max.x, min.y, max.z),
            c(max.x, max.y, max.z), c(min.x, max.y, max.z),
        ];
        // 6 faces, CCW outward, as two tris each.
        let faces = [
            [0, 3, 2, 1], // -z
            [4, 5, 6, 7], // +z
            [0, 1, 5, 4], // -y
            [3, 7, 6, 2], // +y
            [0, 4, 7, 3], // -x
            [1, 2, 6, 5], // +x
        ];
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        for [i, j, k, l] in faces {
            m.push_tri(corners[i], corners[j], corners[k]);
            m.push_tri(corners[i], corners[k], corners[l]);
        }
        m
    }

    pub fn sphere(center: Point3, radius: f32, rings: u32, segments: u32) -> RenderMesh {
        use std::f32::consts::PI;
        let p = |ring: u32, seg: u32| {
            let theta = PI * ring as f32 / rings as f32; // 0..PI (lat)
            let phi = 2.0 * PI * seg as f32 / segments as f32; // 0..2PI (lon)
            center
                + radius
                    * Vec3::new(
                        theta.sin() * phi.cos(),
                        theta.cos(),
                        theta.sin() * phi.sin(),
                    )
        };
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        for ring in 0..rings {
            for seg in 0..segments {
                let (a, b) = (p(ring, seg), p(ring, seg + 1));
                let (cc, d) = (p(ring + 1, seg), p(ring + 1, seg + 1));
                m.push_tri(a, b, d);
                m.push_tri(a, d, cc);
            }
        }
        m
    }
```

Register the module: in `src/geometry/mod.rs` add `pub mod render_mesh;` and `pub use render_mesh::*;` alongside the others.

> NOTE: degenerate triangles at the sphere poles produce a near-zero cross product; `unit()` of a near-zero vector may not be exactly length 1. If the `sphere_vertex_count_matches_tessellation` normal check fails at the poles, guard `push_tri` to fall back to the direction from `center` to the vertex when the cross product is near zero. Implement that guard only if the test fails.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test geometry::render_mesh 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/geometry/render_mesh.rs src/geometry/mod.rs
git commit -m "feat: add RenderMesh with primitive tessellation"
```

---

### Task 3: Retain `RenderMesh` per object (Scene + OBJ loader)

**Files:**
- Modify: `src/geometry/obj_loader.rs` (add raw accessor)
- Modify: `src/scene.rs` (`Shape::Mesh` retains a `RenderMesh`; add `Shape::render_mesh`)
- Modify: `src/scenes/obj.rs` (and any other site building `Shape::Mesh`)
- Test: in `src/scene.rs`

**Interfaces:**
- Consumes: `RenderMesh` and its constructors (Task 2).
- Produces:
  - `Shape::Mesh { object: Arc<dyn Intersect>, render: Arc<RenderMesh> }`
  - `Shape::render_mesh(&self) -> RenderMesh` returning the shape in definition space (clones the retained mesh for `Mesh`; tessellates primitives with fixed detail `rings=16, segments=24`).
  - `ObjData::render_mesh(&self) -> RenderMesh` building positions/face-normals from `verts`/`faces`.

- [ ] **Step 1: Write the failing test**

Add to the end of `src/scene.rs`:

```rust
#[cfg(test)]
mod render_mesh_tests {
    use super::*;
    use crate::vec3::{Point3, Vec3};

    #[test]
    fn primitive_shapes_produce_nonempty_meshes() {
        let sphere = Shape::Sphere { center: Point3::new(0.0, 0.0, 0.0), radius: 1.0 };
        let quad = Shape::Quad {
            q: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
        };
        let bx = Shape::Box { a: Point3::new(0.0, 0.0, 0.0), b: Point3::new(1.0, 1.0, 1.0) };
        assert!(!sphere.render_mesh().positions.is_empty());
        assert_eq!(quad.render_mesh().positions.len(), 6);
        assert_eq!(bx.render_mesh().positions.len(), 36);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test render_mesh_tests 2>&1 | tail -20`
Expected: FAIL — no method `render_mesh` on `Shape`.

- [ ] **Step 3: Add the OBJ accessor**

In `src/geometry/obj_loader.rs`, add to `impl ObjData`:

```rust
    pub fn render_mesh(&self) -> crate::geometry::RenderMesh {
        crate::geometry::RenderMesh::from_triangles(&self.verts, &self.faces)
    }
```

- [ ] **Step 4: Change `Shape::Mesh` to retain a RenderMesh**

In `src/scene.rs`, update the `Shape` enum variant and its `build`/`is_editable`, and add `render_mesh`. Change:

```rust
    Mesh(Arc<dyn Intersect>),
```

to:

```rust
    Mesh {
        object: Arc<dyn Intersect>,
        render: Arc<crate::geometry::RenderMesh>,
    },
```

In `Shape::build`, change the `Mesh` arm from `Shape::Mesh(object) => object.clone()` to:

```rust
            Shape::Mesh { object, .. } => object.clone(),
```

In `Shape::is_editable`, change `!matches!(self, Shape::Mesh(_))` to `!matches!(self, Shape::Mesh { .. })`.

Add a new method to `impl Shape`:

```rust
    /// Triangle geometry for the rasterized preview, in the shape's own
    /// definition space (the object's transform is applied separately as a
    /// model matrix).
    pub fn render_mesh(&self) -> crate::geometry::RenderMesh {
        use crate::geometry::RenderMesh;
        match self {
            Shape::Sphere { center, radius } => RenderMesh::sphere(*center, *radius, 16, 24),
            Shape::Quad { q, u, v } => RenderMesh::quad(*q, *u, *v),
            Shape::Box { a, b } => RenderMesh::unit_box(*a, *b),
            Shape::Mesh { render, .. } => RenderMesh {
                positions: render.positions.clone(),
                normals: render.normals.clone(),
            },
        }
    }
```

- [ ] **Step 5: Fix the `Shape::Mesh` construction site(s)**

Find them: `grep -rn "Shape::Mesh" src/`. In `src/scenes/obj.rs` (the OBJ scene builder), where it currently wraps the loaded BVH as `Shape::Mesh(bvh_handle)`, keep the `ObjData` around long enough to build both. The pattern: load `ObjData`, build the render mesh, then build the BVH/mesh handle:

```rust
    let obj = ObjData::load("objs/dragon.obj");
    let render = Arc::new(obj.render_mesh());
    let object: Arc<dyn Intersect> = Arc::new(/* existing BVH build from obj */);
    // ... Shape::Mesh { object, render }
```

Adjust to however `obj.rs` currently builds the handle (it may call `into_mesh`/`into_triangles` then BVH-build — call `render_mesh()` BEFORE the consuming `into_*` call, since those take `self`). Update the `Shape::Mesh(...)` literal to `Shape::Mesh { object, render }`.

- [ ] **Step 6: Run tests + build**

Run: `cargo test render_mesh_tests 2>&1 | tail -20`
Expected: PASS (1 test).
Run: `cargo build 2>&1 | grep -E "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 7: Commit**

```bash
git add src/geometry/obj_loader.rs src/scene.rs src/scenes/obj.rs
git commit -m "feat: retain RenderMesh per object for the rasterizer"
```

---

### Task 4: Camera & model matrices (glam)

**Files:**
- Modify: `Cargo.toml` (add `glam`)
- Create: `src/viewer/raster/mod.rs` (`pub mod camera_gl;`)
- Create: `src/viewer/raster/camera_gl.rs`
- Modify: `src/viewer/mod.rs` (add `mod raster;`)
- Test: in `src/viewer/raster/camera_gl.rs`

**Interfaces:**
- Consumes: `CameraConfig` fields (`look_from`, `look_at`, `v_up`, `roll`, `fov`, `image_width`, `aspect_ratio`); `crate::scene::Transform`.
- Produces:
  - `view_matrix(cam: &CameraConfig) -> glam::Mat4`
  - `projection_matrix(cam: &CameraConfig, near: f32, far: f32) -> glam::Mat4`
  - `model_matrix(t: &Transform, center: glam::Vec3) -> glam::Mat4` — composes `T(translate)·T(center)·R(euler°)·S(scale)·T(-center)`.

- [ ] **Step 1: Add glam**

In `Cargo.toml` dependencies add:

```toml
glam = { version = "0.29", features = ["mint"] }
```

(The `mint` feature is for the later gizmo plan; harmless now.)

- [ ] **Step 2: Write the failing tests**

Create `src/viewer/raster/camera_gl.rs`:

```rust
//! View / projection / model matrices for the rasterized preview, built to
//! match the path tracer's framing so Edit and Render line up.

use glam::{Mat4, Vec3};

use crate::camera::CameraConfig;
use crate::scene::Transform;

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CameraConfig {
        CameraConfig::builder()
            .look_from(crate::vec3::Vec3::new(0.0, 0.0, 5.0))
            .look_at(crate::vec3::Vec3::new(0.0, 0.0, 0.0))
            .v_up(crate::vec3::Vec3::new(0.0, 1.0, 0.0))
            .build()
    }

    #[test]
    fn view_places_look_from_at_origin_looking_down_neg_z() {
        let v = view_matrix(&cfg());
        // The camera position maps to the origin in view space.
        let eye = v.transform_point3(Vec3::new(0.0, 0.0, 5.0));
        assert!(eye.length() < 1e-4, "eye={eye:?}");
        // A point in front of the camera (toward look_at) has negative view z.
        let front = v.transform_point3(Vec3::new(0.0, 0.0, 0.0));
        assert!(front.z < 0.0, "front={front:?}");
    }

    #[test]
    fn projection_keeps_a_centered_point_centered() {
        let p = projection_matrix(&cfg(), 0.1, 100.0);
        let clip = p.project_point3(Vec3::new(0.0, 0.0, -5.0));
        assert!(clip.x.abs() < 1e-4 && clip.y.abs() < 1e-4, "clip={clip:?}");
    }

    #[test]
    fn model_identity_transform_is_identity() {
        let m = model_matrix(&Transform::identity(), Vec3::ZERO);
        assert!((m - Mat4::IDENTITY).abs_diff_eq(Mat4::ZERO, 1e-5));
    }

    #[test]
    fn model_translate_moves_point() {
        let mut t = Transform::identity();
        t.translate = crate::vec3::Vec3::new(2.0, 0.0, 0.0);
        let m = model_matrix(&t, Vec3::ZERO);
        let p = m.transform_point3(Vec3::new(0.0, 0.0, 0.0));
        assert!((p - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5, "p={p:?}");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test viewer::raster::camera_gl 2>&1 | tail -20`
Expected: FAIL — `view_matrix`/`projection_matrix`/`model_matrix` not found.

- [ ] **Step 4: Implement the matrices**

Register the module: create `src/viewer/raster/mod.rs` with `pub mod camera_gl;`, and add `mod raster;` to `src/viewer/mod.rs`.

Add to `src/viewer/raster/camera_gl.rs` (above the test module):

```rust
fn g(v: crate::vec3::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Right-handed view matrix; `v_up` rolled about the forward axis by `roll`
/// (degrees), matching `Camera::from`.
pub fn view_matrix(cam: &CameraConfig) -> Mat4 {
    let eye = g(cam.look_from);
    let target = g(cam.look_at);
    let forward = (target - eye).normalize();
    let up = g(cam.v_up);
    let rolled_up = glam::Quat::from_axis_angle(forward, cam.roll.to_radians()) * up;
    Mat4::look_at_rh(eye, target, rolled_up)
}

/// GL-NDC perspective (z ∈ [−1, 1]) using the config's vertical fov.
pub fn projection_matrix(cam: &CameraConfig, near: f32, far: f32) -> Mat4 {
    let aspect = cam.image_width as f32
        / ((cam.image_width as f64 / cam.aspect_ratio) as f32).max(1.0);
    Mat4::perspective_rh_gl(cam.fov.to_radians(), aspect, near, far)
}

/// Object model matrix: scale + Euler rotation about `center`, then translate —
/// the same composition as `ObjectSpec::build`.
pub fn model_matrix(t: &Transform, center: Vec3) -> Mat4 {
    let translate = Mat4::from_translation(g(t.translate));
    let to_center = Mat4::from_translation(center);
    let from_center = Mat4::from_translation(-center);
    let rot = Mat4::from_euler(
        glam::EulerRot::XYZ,
        t.rotate.x.to_radians(),
        t.rotate.y.to_radians(),
        t.rotate.z.to_radians(),
    );
    let scale = Mat4::from_scale(g(t.scale));
    translate * to_center * rot * scale * from_center
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test viewer::raster::camera_gl 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/viewer/raster/mod.rs src/viewer/raster/camera_gl.rs src/viewer/mod.rs
git commit -m "feat: glam view/projection/model matrices for the rasterizer"
```

---

### Task 5: GL renderer (shader + glow buffers + paint)

**Files:**
- Create: `src/viewer/raster/renderer.rs`
- Modify: `src/viewer/raster/mod.rs` (`pub mod renderer;`)

**Interfaces:**
- Consumes: `camera_gl::{view_matrix, projection_matrix, model_matrix}`; `Scene`, `ObjectSpec`, `Shape::render_mesh`, `MaterialSpec`; the `glow::Context` from eframe (`cc.gl`).
- Produces:
  - `pub struct SceneRenderer` with:
    - `pub fn new(gl: &glow::Context) -> Self`
    - `pub fn paint(&mut self, gl: &glow::Context, scene: &Scene, viewport_px: (f32, f32))`
  - It uploads each object's `RenderMesh` to a VBO once, keyed by a geometry generation it tracks internally (rebuild when the object count or any mesh changes — for this task, rebuild when the object **count** changes; finer invalidation is fine to add later).

> This task renders GL to the screen and cannot be unit-tested in this harness. It is verified by a clean build here and a visual check in Task 6.

- [ ] **Step 1: Write the renderer**

Create `src/viewer/raster/renderer.rs`. Use the `glow` 0.x API (matches the version eframe 0.34 pulls). This is the complete module:

```rust
//! A minimal flat-shaded OpenGL renderer for the scene preview, driven through
//! eframe's glow context inside an egui paint callback.

use glow::HasContext;

use crate::scene::{MaterialSpec, Scene};
use super::camera_gl;

const AMBIENT: f32 = 0.25;

struct ObjectBuffers {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    vertex_count: i32,
}

pub struct SceneRenderer {
    program: glow::Program,
    objects: Vec<ObjectBuffers>,
    built_count: usize, // object count the buffers were built for
}

impl SceneRenderer {
    pub fn new(gl: &glow::Context) -> Self {
        let shader_version = if cfg!(target_arch = "wasm32") {
            "#version 300 es\nprecision highp float;\n"
        } else {
            "#version 330\n"
        };
        let vert = format!("{shader_version}{VERT}");
        let frag = format!("{shader_version}{FRAG}");
        let program = unsafe { link_program(gl, &vert, &frag) };
        SceneRenderer { program, objects: Vec::new(), built_count: usize::MAX }
    }

    fn rebuild(&mut self, gl: &glow::Context, scene: &Scene) {
        unsafe {
            for o in self.objects.drain(..) {
                gl.delete_vertex_array(o.vao);
                gl.delete_buffer(o.vbo);
            }
            for obj in &scene.objects {
                let mesh = obj.shape.render_mesh();
                // Interleave position+normal as 6 f32 per vertex.
                let mut data: Vec<f32> = Vec::with_capacity(mesh.positions.len() * 6);
                for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
                    data.extend_from_slice(p);
                    data.extend_from_slice(n);
                }
                let vao = gl.create_vertex_array().unwrap();
                let vbo = gl.create_buffer().unwrap();
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&data),
                    glow::STATIC_DRAW,
                );
                let stride = 6 * std::mem::size_of::<f32>() as i32;
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
                gl.enable_vertex_attrib_array(1);
                gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 3 * 4);
                self.objects.push(ObjectBuffers { vao, vbo, vertex_count: mesh.positions.len() as i32 });
            }
            self.built_count = scene.objects.len();
        }
    }

    pub fn paint(&mut self, gl: &glow::Context, scene: &Scene, viewport_px: (f32, f32)) {
        if self.built_count != scene.objects.len() {
            self.rebuild(gl, scene);
        }
        let view = camera_gl::view_matrix(&scene.camera);
        let proj = camera_gl::projection_matrix(&scene.camera, 0.05, 1000.0);
        unsafe {
            gl.enable(glow::DEPTH_TEST);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.use_program(Some(self.program));
            uniform_mat4(gl, self.program, "u_view", &view);
            uniform_mat4(gl, self.program, "u_proj", &proj);
            gl.uniform_1_f32(
                gl.get_uniform_location(self.program, "u_ambient").as_ref(),
                AMBIENT,
            );
            for (obj, buf) in scene.objects.iter().zip(&self.objects) {
                let center = bbox_center(obj);
                let model = camera_gl::model_matrix(&obj.transform, center);
                uniform_mat4(gl, self.program, "u_model", &model);
                let (color, emissive) = preview_color(&obj.material);
                gl.uniform_3_f32(
                    gl.get_uniform_location(self.program, "u_color").as_ref(),
                    color[0], color[1], color[2],
                );
                gl.uniform_1_i32(
                    gl.get_uniform_location(self.program, "u_emissive").as_ref(),
                    emissive as i32,
                );
                gl.bind_vertex_array(Some(buf.vao));
                gl.draw_arrays(glow::TRIANGLES, 0, buf.vertex_count);
            }
            gl.disable(glow::DEPTH_TEST);
        }
        let _ = viewport_px; // glow uses the callback's scissor/viewport
    }
}

fn bbox_center(obj: &crate::scene::ObjectSpec) -> glam::Vec3 {
    let bb = obj.shape.build(crate::material::blank_material()).bounding_box().center();
    glam::Vec3::new(bb.x, bb.y, bb.z)
}

/// Preview color and whether the material is emissive (drawn unlit at full color).
fn preview_color(m: &MaterialSpec) -> ([f32; 3], bool) {
    match m {
        MaterialSpec::Lambertian { albedo } => ([albedo.x, albedo.y, albedo.z], false),
        MaterialSpec::Metal { albedo, .. } => ([albedo.x, albedo.y, albedo.z], false),
        MaterialSpec::Dielectric { .. } => ([0.85, 0.9, 1.0], false),
        MaterialSpec::DiffuseLight { emit } => ([emit.x, emit.y, emit.z], true),
    }
}

unsafe fn uniform_mat4(gl: &glow::Context, prog: glow::Program, name: &str, m: &glam::Mat4) {
    let loc = gl.get_uniform_location(prog, name);
    gl.uniform_matrix_4_f32_slice(loc.as_ref(), false, &m.to_cols_array());
}

unsafe fn link_program(gl: &glow::Context, vert: &str, frag: &str) -> glow::Program {
    let program = gl.create_program().unwrap();
    let shaders = [(glow::VERTEX_SHADER, vert), (glow::FRAGMENT_SHADER, frag)];
    let mut compiled = Vec::new();
    for (ty, src) in shaders {
        let s = gl.create_shader(ty).unwrap();
        gl.shader_source(s, src);
        gl.compile_shader(s);
        assert!(gl.get_shader_compile_status(s), "{}", gl.get_shader_info_log(s));
        gl.attach_shader(program, s);
        compiled.push(s);
    }
    gl.link_program(program);
    assert!(gl.get_program_link_status(program), "{}", gl.get_program_info_log(program));
    for s in compiled {
        gl.detach_shader(program, s);
        gl.delete_shader(s);
    }
    program
}

const VERT: &str = r#"
layout (location = 0) in vec3 a_pos;
layout (location = 1) in vec3 a_normal;
uniform mat4 u_model;
uniform mat4 u_view;
uniform mat4 u_proj;
out vec3 v_normal_view;
void main() {
    mat4 mv = u_view * u_model;
    v_normal_view = mat3(mv) * a_normal;
    gl_Position = u_proj * mv * vec4(a_pos, 1.0);
}
"#;

const FRAG: &str = r#"
in vec3 v_normal_view;
uniform vec3 u_color;
uniform float u_ambient;
uniform bool u_emissive;
out vec4 frag;
void main() {
    if (u_emissive) {
        frag = vec4(u_color, 1.0);
        return;
    }
    // Headlight: light direction = view direction = -z in view space, so the
    // shade is just the normal's view-space z (facing the camera).
    vec3 n = normalize(v_normal_view);
    float ndotv = max(n.z, 0.0);
    float shade = u_ambient + (1.0 - u_ambient) * ndotv;
    frag = vec4(u_color * shade, 1.0);
}
"#;
```

> Two project hooks this needs: (a) a way to get a throwaway material for `shape.build` in `bbox_center` — add `pub fn blank_material() -> std::sync::Arc<dyn Material>` to `src/material/mod.rs` returning an `Arc::new(Blank)` (the existing `blank` material), or reuse an existing constructor if one exists. (b) `bytemuck` for `cast_slice` — add `bytemuck = "1"` to `Cargo.toml`. Both are tiny; include them in this task's commit.

- [ ] **Step 2: Add deps and module registration**

Add to `Cargo.toml`: `bytemuck = "1"`. In `src/viewer/raster/mod.rs` add `pub mod renderer;`. Add the `blank_material()` helper to `src/material/mod.rs` if not already present.

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | grep -E "^error" || echo OK`
Expected: `OK`. Resolve any glow API signature differences against the installed `glow` version (the call names above are stable across glow 0.13–0.17; adjust only if the compiler flags a specific signature).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/viewer/raster/mod.rs src/viewer/raster/renderer.rs src/material/mod.rs
git commit -m "feat: flat-shaded glow scene renderer"
```

---

### Task 6: Wire the renderer into Edit mode

**Files:**
- Modify: `src/viewer/mod.rs` (hold a `SceneRenderer`, paint it in Edit mode)

**Interfaces:**
- Consumes: `SceneRenderer::{new, paint}` (Task 5); eframe's `cc.gl` (an `Option<Arc<glow::Context>>`).
- Produces: Edit mode draws the GL preview via an `egui_glow::CallbackFn` instead of the path-traced texture; Edit-mode camera gestures no longer restart the path trace (they just request a repaint).

> GL-on-screen behavior is verified by build + manual visual check, not a unit test.

- [ ] **Step 1: Store the renderer on the app**

In `src/viewer/mod.rs`, add to `ViewerApp`:

```rust
    gl_renderer: std::sync::Arc<std::sync::Mutex<super::viewer::raster::renderer::SceneRenderer>>,
```

(Use the crate-correct path: `raster::renderer::SceneRenderer`.) In `ViewerApp::new`, build it from the glow context:

```rust
        let gl = cc.gl.as_ref().expect("eframe glow context");
        let gl_renderer = std::sync::Arc::new(std::sync::Mutex::new(
            raster::renderer::SceneRenderer::new(gl),
        ));
```

and set `gl_renderer` in the returned struct.

- [ ] **Step 2: Paint the GL view in Edit mode**

In the central panel, in the `Mode::Edit` branch, instead of painting the path-traced texture, add a glow paint callback over the viewport rect. After the gesture handling, replace the texture paint (for Edit mode only) with:

```rust
            let scene = self.scene.clone();
            let renderer = self.gl_renderer.clone();
            let rect = vp;
            let cb = egui_glow::CallbackFn::new(move |_info, painter| {
                let scene = scene.lock().unwrap();
                renderer
                    .lock()
                    .unwrap()
                    .paint(painter.gl(), &scene, (rect.width(), rect.height()));
            });
            ui.painter().add(egui::PaintCallback { rect, callback: std::sync::Arc::new(cb) });
```

Render mode keeps painting the path-traced texture exactly as before.

- [ ] **Step 3: Stop restarting the path trace while orbiting in Edit**

In the `Mode::Edit` gesture handling, the camera still updates on drag/scroll, but since the GL view is instant, the path-trace `invalidate()` and the preview-scale machinery are no longer needed for Edit. Change the Edit arm so that after a `moved` gesture it calls `ui.ctx().request_repaint()` (to redraw the GL view) instead of `self.render.invalidate()`. Leave the preview-scale code paths alone (they become inert in Edit since we no longer invalidate); do not delete them in this task.

- [ ] **Step 4: Build and manually verify**

Run: `cargo build 2>&1 | grep -E "^error" || echo OK`
Expected: `OK`.

Run (manual): `cargo run --release`
Verify:
- Edit mode shows a shaded 3D view of the scene (primitives + the dragon), not the path-traced image.
- Orbit/pan/dolly move the 3D view smoothly and instantly.
- The framing matches: toggling Edit↔Render keeps the same camera framing.
- Render mode still path-traces as before.

- [ ] **Step 5: Commit**

```bash
git add src/viewer/mod.rs
git commit -m "feat: draw rasterized GL preview in Edit mode"
```

---

## Self-Review

- **Spec coverage:** Phase 0 downgrade → Task 1 ✓; RenderMesh + scope-B mesh retention → Tasks 2–3 ✓; camera matrices matched to tracer → Task 4 ✓; GL renderer (shader, Lambert+ambient, emissive, WebGL2 `#version`, retained buffers) → Task 5 ✓; Edit-mode integration replacing path-traced texture, no path-trace restart in Edit → Task 6 ✓; lit shading with ambient floor → Task 5 fragment shader ✓; primitives + meshes → Tasks 2/3/5 ✓. **Gizmo is intentionally deferred to a separate plan** (noted up front). Camera `roll` handled in Task 4 view matrix ✓.
- **Placeholder scan:** The two `> NOTE` blocks (sphere-pole normal guard; the `blank_material`/`bytemuck` hooks) are conditional implementation details with concrete instructions, not deferrals. The Task 3 Step 5 "adjust to however obj.rs builds the handle" is real (the WIP scene code is in flux) and bounded by the stated rule (call `render_mesh()` before the consuming `into_*`).
- **Type consistency:** `RenderMesh { positions, normals }` used identically across Tasks 2–5; `Shape::Mesh { object, render }` consistent in Tasks 3 and 5; `camera_gl::{view_matrix, projection_matrix, model_matrix}` signatures match between Tasks 4 and 5; `SceneRenderer::{new, paint}` consistent between Tasks 5 and 6.
- **Note for execution:** the working tree has uncommitted glossy-material WIP; commit it as a baseline before starting Task 1 so feature commits stay clean.
