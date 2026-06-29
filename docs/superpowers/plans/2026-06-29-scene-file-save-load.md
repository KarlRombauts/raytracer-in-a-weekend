# `.scene` Save / Load (Phase 2A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a portable `.scene` file format (lz4-compressed postcard) plus Save/Load wiring, so a scene — camera, primitives, imported OBJ meshes, and embedded image textures — round-trips faithfully on both native and the web build, with an embedded preview thumbnail for the future library.

**Architecture:** Derive `serde` in place on the plain-data scene types; the only hand-written case is `Shape`, whose `Mesh` variant serializes just `verts`+`faces` and rebuilds the BVH + preview mesh on load. A `scene_file` module wraps `postcard` + `lz4_flex` + a magic/version header. A cfg-split `platform` layer handles native (rfd + fs) and web (Blob download / async file pick) I/O, exposed to the UI through a polled `ScenePicker`.

**Tech Stack:** Rust (edition 2024), serde, postcard, lz4_flex, rfd (native sync + wasm async), eframe/egui, image, wasm-bindgen-futures.

## Global Constraints

- **Codec is postcard + `lz4_flex` (pure Rust, wasm-safe). NOT zstd** (its C binding doesn't build for `wasm32-unknown-unknown`). This corrects the earlier editable-textures decision.
- **Format:** `.scene` bytes = `b"RTSC"` (4-byte magic) ++ `lz4_flex::compress_prepend_size(postcard(SceneFile))`. `SceneFile { version: u32 (=1), name: Option<String>, scene: Scene, preview: Vec<u8> }`.
- **Load never panics** — `decode` returns `Result`, and the UI surfaces errors as a message.
- **Native stays on stable Rust; everything must also build for `wasm32-unknown-unknown`.** New deps (serde, postcard, lz4_flex) go in the shared `[dependencies]` table; all are pure Rust.
- **Mesh material:** rebuilt meshes keep the baked grey Lambertian (same as today's OBJ import). Per-mesh material editing stays out of scope.
- **`Point3` and `Color` are both aliases of `Vec3`** — deriving serde on `Vec3` covers them.
- Serde derive-in-place; only `Shape` is hand-written. `Asset.bytes: Arc<[u8]>` uses a `#[serde(with)]` helper (no serde `rc` feature).

---

## File Structure

| File | Responsibility | Change |
|------|----------------|--------|
| `Cargo.toml` | add `serde`, `postcard`, `lz4_flex` (shared); move `rfd` to shared; web-sys/futures for upload | Modify |
| `src/vec3.rs` | `Serialize`/`Deserialize` on `Vec3` | Modify |
| `src/texture/mapped_texture.rs` | serde on `Projection` | Modify |
| `src/camera/config.rs` | serde on `CameraConfig` | Modify |
| `src/scene.rs` | serde on the spec types; `arc_bytes` helper; `MeshData` + custom `Shape` serde; `ObjectSpec`/`Scene` serde | Modify |
| `src/geometry/obj_loader.rs` | `ObjData::mesh_data()` accessor | Modify |
| `src/scene_file.rs` | `SceneFile`, `encode`/`decode`, `LoadedScene`, `SceneFileError` | Create |
| `src/lib.rs` | `pub mod scene_file;` | Modify |
| `src/platform.rs` | `save_scene`, `pick_scene`, `ScenePicker`, `PickStatus` | Modify |
| `src/viewer/mod.rs` | thumbnail helper; Scene row (Save/Load buttons); poll+apply loaded scene | Modify |
| `docs/superpowers/specs/2026-06-29-editable-textures-design.md` | note zstd→lz4 correction | Modify |

---

## Task 1: Serde on the leaf data types

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/vec3.rs`, `src/texture/mapped_texture.rs`, `src/camera/config.rs`
- Modify: `src/scene.rs` (`Asset`, `Mapping`, `TextureSpec`, `CellTexture`, `MaterialSpec`, `Transform` + `arc_bytes` helper + test)

**Interfaces:**
- Produces: `Serialize`/`Deserialize`/`PartialEq` on `Vec3`, `Projection`, `CameraConfig`, `Asset`, `Mapping`, `TextureSpec`, `CellTexture`, `MaterialSpec`, `Transform`. `Shape`, `ObjectSpec`, `Scene` are NOT serde yet (Task 2 — they reference `Shape`).

- [ ] **Step 1: Add serde + postcard dependencies**

In `Cargo.toml`, add to the shared `[dependencies]` table:

```toml
serde = { version = "1", features = ["derive"] }
postcard = { version = "1", features = ["alloc"] }
```

- [ ] **Step 2: Derive serde on `Vec3`**

In `src/vec3.rs`, add the import near the top:

```rust
use serde::{Deserialize, Serialize};
```

Change the `Vec3` derive line (currently `#[derive(Debug, Copy, Clone, PartialEq)]`) to:

```rust
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
```

- [ ] **Step 3: Derive serde on `Projection`**

In `src/texture/mapped_texture.rs`, add `use serde::{Deserialize, Serialize};` near the top, and change the `Projection` derive (currently `#[derive(Clone, Copy, PartialEq, Debug)]`) to add `Serialize, Deserialize`:

```rust
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
```

- [ ] **Step 4: Derive serde on `CameraConfig`**

In `src/camera/config.rs`, add `use serde::{Deserialize, Serialize};` near the top, and change the derive (currently `#[derive(TypedBuilder, Clone)]`) to:

```rust
#[derive(TypedBuilder, Clone, PartialEq, Serialize, Deserialize)]
```

(TypedBuilder's `#[builder(...)]` field attributes are independent of serde; all field types — `f64`, `u32`, `f32`, `Vec3`, `Color` — are now serde-able.)

- [ ] **Step 5: Add the `arc_bytes` serde helper + derive serde on the scene leaf types**

In `src/scene.rs`, add the import near the top (with the other `use` lines):

```rust
use serde::{Deserialize, Serialize};
```

Add this module near the top of `src/scene.rs` (after the imports):

```rust
/// (De)serialize `Arc<[u8]>` as a byte sequence without enabling serde's global
/// `rc` feature. Round-trips through a `Vec<u8>` (a postcard length-prefixed seq).
mod arc_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S: Serializer>(bytes: &Arc<[u8]>, s: S) -> Result<S::Ok, S::Error> {
        bytes.as_ref().to_vec().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<[u8]>, D::Error> {
        Ok(Arc::from(Vec::<u8>::deserialize(d)?))
    }
}
```

Update the leaf type derives + the `Asset.bytes` field attribute:

`Asset` (currently `#[derive(Clone)]`):
```rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    #[serde(with = "arc_bytes")]
    pub bytes: Arc<[u8]>,
    pub label: Option<String>,
}
```

`Mapping` (currently `#[derive(Clone, Copy, PartialEq)]`):
```rust
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
```

`TextureSpec` (currently `#[derive(Clone)]`):
```rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
```

`CellTexture` (currently `#[derive(Clone)]`):
```rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
```

`MaterialSpec` (currently `#[derive(Clone)]`):
```rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
```

`Transform` (currently `#[derive(Clone)]`):
```rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
```

- [ ] **Step 6: Write the round-trip test**

Add to the bottom of `src/scene.rs`:

```rust
#[cfg(test)]
mod serde_tests {
    use super::*;

    #[test]
    fn material_spec_with_image_asset_round_trips_via_postcard() {
        let m = MaterialSpec::Glossy {
            albedo: TextureSpec::Image {
                asset: Asset {
                    bytes: Arc::from([1u8, 2, 3, 4, 5].as_slice()),
                    label: Some("tex.png".to_string()),
                },
                mapping: Mapping::default(),
            },
            roughness: 0.3,
        };
        let bytes = postcard::to_allocvec(&m).expect("encode");
        let back: MaterialSpec = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(m, back);
    }

    #[test]
    fn checker_texture_round_trips() {
        let t = TextureSpec::Checker {
            scale: 2.5,
            even: CellTexture::Solid { color: Color::new(0.1, 0.2, 0.3) },
            odd: CellTexture::Noise { scale: 4.0, depth: 7 },
        };
        let bytes = postcard::to_allocvec(&t).expect("encode");
        let back: TextureSpec = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(t, back);
    }
}
```

- [ ] **Step 7: Run the tests**

Run: `cargo test --lib serde_tests`
Expected: 2 passed. (If it fails to compile because `Shape`/`ObjectSpec` need serde, that's expected to NOT happen here — only leaf types changed. The whole crate must still build: `cargo build`.)

Run: `cargo build`
Expected: compiles (existing tests unaffected).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml src/vec3.rs src/texture/mapped_texture.rs src/camera/config.rs src/scene.rs
git commit -m "feat: serde on leaf scene data types (vec3, camera, textures, materials)"
```

---

## Task 2: `MeshData` + custom `Shape` serde + `ObjectSpec`/`Scene` serde

**Files:**
- Modify: `src/geometry/obj_loader.rs` (add `mesh_data()`)
- Modify: `src/scene.rs` (`MeshData`, `Shape::Mesh` carries `data`, custom `Serialize`/`Deserialize` for `Shape`, refactor `from_obj`, serde on `ObjectSpec`/`Scene`, test)

**Interfaces:**
- Consumes: leaf serde from Task 1; `crate::geometry::{Triangle, RenderMesh, ObjData}`, `crate::ray::BVH`.
- Produces: `Scene: Serialize + Deserialize`. `MeshData { verts: Vec<Vec3>, faces: Vec<[u32; 3]> }` with `MeshData::build(&self) -> (Arc<dyn Intersect>, Arc<RenderMesh>)`. `Shape::Mesh { data: Arc<MeshData>, object, render }`.

- [ ] **Step 1: Add `ObjData::mesh_data()`**

In `src/geometry/obj_loader.rs`, add this method inside `impl ObjData` (after `render_mesh`):

```rust
    /// The mesh's positions and triangle indices — the portable, serializable
    /// description of the geometry (everything else is rebuilt from these).
    pub fn mesh_data(&self) -> (Vec<crate::vec3::Vec3>, Vec<[u32; 3]>) {
        let faces = self
            .faces
            .iter()
            .map(|[i, j, k]| [*i as u32, *j as u32, *k as u32])
            .collect();
        (self.verts.clone(), faces)
    }
```

- [ ] **Step 2: Write the failing test (full-scene round-trip incl. a mesh)**

Add to the bottom of `src/scene.rs`, inside the existing `serde_tests` module (or a new `mesh_serde_tests` module):

```rust
#[cfg(test)]
mod mesh_serde_tests {
    use super::*;

    fn tiny_mesh_scene() -> Scene {
        // A single triangle mesh + a sphere, so we cover both Mesh and a primitive.
        let data = MeshData {
            verts: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 2]],
        };
        let (object, render) = data.build();
        let mesh = ObjectSpec {
            name: "tri".to_string(),
            shape: Shape::Mesh { data: Arc::new(data), object, render },
            material: MaterialSpec::Lambertian {
                albedo: TextureSpec::solid(Color::new(0.7, 0.7, 0.7)),
            },
            transform: Transform::identity(),
        };
        let sphere = ObjectSpec {
            name: "ball".to_string(),
            shape: Shape::Sphere { center: Vec3::new(2.0, 0.0, 0.0), radius: 1.5 },
            material: MaterialSpec::Metal { albedo: Color::new(0.8, 0.8, 0.8), fuzz: 0.1 },
            transform: Transform::identity(),
        };
        Scene {
            camera: CameraConfig::builder().image_width(64).build(),
            objects: vec![mesh, sphere],
        }
    }

    #[test]
    fn scene_with_mesh_round_trips_via_postcard() {
        let scene = tiny_mesh_scene();
        let bytes = postcard::to_allocvec(&scene).expect("encode");
        let back: Scene = postcard::from_bytes(&bytes).expect("decode");

        assert_eq!(back.objects.len(), 2);
        assert_eq!(back.camera, scene.camera);
        assert_eq!(back.objects[0].name, "tri");
        assert_eq!(back.objects[1].name, "ball");
        assert_eq!(back.objects[0].material, scene.objects[0].material);

        // Mesh geometry survived as verts + faces.
        match &back.objects[0].shape {
            Shape::Mesh { data, .. } => {
                assert_eq!(data.verts.len(), 3);
                assert_eq!(data.faces, vec![[0u32, 1, 2]]);
            }
            other => panic!("expected mesh, got {other:?}"),
        }

        // The decoded mesh rebuilt a usable BVH: the world assembles and the
        // mesh's bounding box is finite.
        let world = build_world(&back);
        assert!(world.bounding_box().center().x.is_finite());
    }
}
```

> The test prints `{other:?}` on the mesh branch, so `Shape` needs `#[derive(Debug)]`. Add `Debug` to `Shape`'s derive in Step 4 if it isn't already present. If `IntersectGroup`/`world` lacks a `bounding_box()` accessor, replace the last two lines with `let _ = build_world(&back);` and assert it did not panic.

- [ ] **Step 3: Run the test — verify it fails**

Run: `cargo test --lib mesh_serde_tests`
Expected: FAIL to compile — `MeshData` doesn't exist, `Shape::Mesh` has no `data` field, `Scene` isn't `Deserialize`.

- [ ] **Step 4: Implement `MeshData`, the `Shape` change, custom serde, and `from_obj` refactor**

In `src/scene.rs`, add these imports if not already present:

```rust
use crate::geometry::{RenderMesh, Triangle};
```

Add `MeshData` (near `Shape`):

```rust
/// The portable description of a triangle mesh: positions + triangle indices.
/// Everything else (per-triangle geometry, BVH, preview mesh) is rebuilt from
/// these on load.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub verts: Vec<Vec3>,
    pub faces: Vec<[u32; 3]>,
}

impl MeshData {
    /// Rebuild the runtime intersect handle (BVH) and the preview mesh from the
    /// stored arrays. Bakes the default grey Lambertian, matching OBJ import.
    pub fn build(&self) -> (Arc<dyn Intersect>, Arc<RenderMesh>) {
        let material = MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.73, 0.73, 0.73)),
        }
        .build();
        let faces_usize: Vec<[usize; 3]> = self
            .faces
            .iter()
            .map(|[i, j, k]| [*i as usize, *j as usize, *k as usize])
            .collect();
        let triangles: Vec<Triangle> = faces_usize
            .iter()
            .map(|[i, j, k]| {
                Triangle::from_points(&self.verts[*i], &self.verts[*j], &self.verts[*k], material.clone())
            })
            .collect();
        let bvh = BVH::build(triangles);
        let render = Arc::new(RenderMesh::from_triangles_smooth(&self.verts, &faces_usize));
        (Arc::new(bvh), render)
    }
}
```

Change `Shape` to carry `data` on `Mesh` and add `Debug` to the derive. Replace the `Shape` enum definition with:

```rust
#[derive(Clone, Debug)]
pub enum Shape {
    Sphere { center: Point3, radius: f32 },
    Quad { q: Point3, u: Vec3, v: Vec3 },
    Box { a: Point3, b: Point3 },
    Mesh {
        data: Arc<MeshData>,
        object: Arc<dyn Intersect>,
        render: Arc<crate::geometry::RenderMesh>,
    },
}
```

> `Arc<dyn Intersect>` and `Arc<RenderMesh>` aren't `Debug`; if adding `Debug` to `Shape` fails to compile, instead implement a manual `Debug` that prints only the variant name (e.g. `Shape::Mesh{..} => write!(f, "Mesh")`), or drop `Debug` from `Shape` and change the test's panic to `panic!("expected mesh")` without `{other:?}`. Prefer the latter (simplest).

Add the custom serde for `Shape` (after the `Shape` impl block). It uses a private proxy enum:

```rust
#[derive(Serialize, Deserialize)]
enum ShapeData {
    Sphere { center: Point3, radius: f32 },
    Quad { q: Point3, u: Vec3, v: Vec3 },
    Box { a: Point3, b: Point3 },
    Mesh { data: MeshData },
}

impl Serialize for Shape {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let repr = match self {
            Shape::Sphere { center, radius } => ShapeData::Sphere { center: *center, radius: *radius },
            Shape::Quad { q, u, v } => ShapeData::Quad { q: *q, u: *u, v: *v },
            Shape::Box { a, b } => ShapeData::Box { a: *a, b: *b },
            Shape::Mesh { data, .. } => ShapeData::Mesh { data: (**data).clone() },
        };
        repr.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Shape {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match ShapeData::deserialize(d)? {
            ShapeData::Sphere { center, radius } => Shape::Sphere { center, radius },
            ShapeData::Quad { q, u, v } => Shape::Quad { q, u, v },
            ShapeData::Box { a, b } => Shape::Box { a, b },
            ShapeData::Mesh { data } => {
                let data = Arc::new(data);
                let (object, render) = data.build();
                Shape::Mesh { data, object, render }
            }
        })
    }
}
```

Refactor `ObjectSpec::from_obj` so the `Shape::Mesh` carries `data`. Replace the body section that builds the mesh (the `let obj = ObjData::load(...)` through the `Shape::Mesh { ... }` construction) with:

```rust
        let obj = ObjData::load(path_str);
        let (verts, faces) = obj.mesh_data();
        let data = Arc::new(MeshData { verts, faces });
        let (object, render) = data.build();

        // Auto-fit: scale the mesh to the target size and recentre it.
        let bbox = object.bounding_box();
        let c = bbox.center();
        let e = bbox.extent();
        let e_max = e.x.max(e.y).max(e.z).max(1e-6);
        let s = (target_size / e_max).max(1e-4);

        let transform = Transform {
            rotate: Vec3::ZERO,
            scale: Vec3::new(s, s, s),
            translate: target_center - c,
        };

        Some(ObjectSpec {
            name,
            shape: Shape::Mesh { data, object, render },
            material,
            transform,
        })
```

> This drops the now-unused `obj.into_triangles(...)`/`obj.render_mesh()`/`BVH::build` lines from `from_obj` (they're folded into `MeshData::build`). Keep the earlier `let material = MaterialSpec::Lambertian {...}` line — it sets `ObjectSpec.material`. Remove the local `triangles`/`bvh` variables that are no longer used.

Finally, derive serde on `ObjectSpec` and `Scene` (currently `#[derive(Clone)]`):

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct ObjectSpec { /* unchanged fields */ }
```
```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct Scene { /* unchanged fields */ }
```

- [ ] **Step 5: Run the test — verify it passes**

Run: `cargo test --lib mesh_serde_tests serde_tests`
Expected: all pass.

Run: `cargo build`
Expected: compiles. Fix any unused-import/variable warnings introduced in `from_obj`.

- [ ] **Step 6: Commit**

```bash
git add src/scene.rs src/geometry/obj_loader.rs
git commit -m "feat: serde for Shape/Scene with mesh rebuild from verts+faces"
```

---

## Task 3: `scene_file` module — encode / decode

**Files:**
- Create: `src/scene_file.rs`
- Modify: `src/lib.rs` (add `pub mod scene_file;`)
- Modify: `Cargo.toml` (add `lz4_flex`)
- Modify: `docs/superpowers/specs/2026-06-29-editable-textures-design.md` (zstd→lz4 note)

**Interfaces:**
- Consumes: `Scene: Serialize + Deserialize` (Task 2).
- Produces: `scene_file::encode(scene: &Scene, name: Option<&str>, preview: &[u8]) -> Vec<u8>`, `scene_file::decode(bytes: &[u8]) -> Result<LoadedScene, SceneFileError>`, `LoadedScene { scene: Scene, name: Option<String>, preview: Vec<u8> }`.

- [ ] **Step 1: Add the `lz4_flex` dependency**

In `Cargo.toml`, shared `[dependencies]`:

```toml
lz4_flex = "0.11"
```

- [ ] **Step 2: Write the failing tests**

Create `src/scene_file.rs` with ONLY the test module first (so it fails to compile against missing items):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::scene::Scene;

    fn empty_scene() -> Scene {
        Scene { camera: CameraConfig::builder().image_width(32).build(), objects: vec![] }
    }

    #[test]
    fn round_trips_scene_name_and_preview() {
        let scene = empty_scene();
        let preview = vec![1u8, 2, 3, 4];
        let bytes = encode(&scene, Some("My Scene"), &preview);
        let loaded = decode(&bytes).expect("decode");
        assert_eq!(loaded.name.as_deref(), Some("My Scene"));
        assert_eq!(loaded.preview, preview);
        assert_eq!(loaded.scene.camera.image_width, 32);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = encode(&empty_scene(), None, &[]);
        bytes[0] = b'X';
        assert!(matches!(decode(&bytes), Err(SceneFileError::BadMagic)));
    }

    #[test]
    fn rejects_truncated_garbage() {
        assert!(decode(&[1, 2, 3]).is_err());
        assert!(decode(b"RTSCgarbage").is_err());
    }
}
```

- [ ] **Step 3: Run — verify it fails**

Run: `cargo test --lib scene_file` (after adding `pub mod scene_file;` to `src/lib.rs` — do that now)
Expected: FAIL to compile (`encode`/`decode`/`SceneFileError` undefined).

Add to `src/lib.rs` alongside the other `pub mod` lines:
```rust
pub mod scene_file;
```

- [ ] **Step 4: Implement the module**

Prepend (above the test module) in `src/scene_file.rs`:

```rust
//! The `.scene` container: an lz4-compressed postcard blob behind a 4-byte
//! magic header. Pure-Rust codec so it builds for both native and wasm.

use serde::{Deserialize, Serialize};

use crate::scene::Scene;

const MAGIC: &[u8; 4] = b"RTSC";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SceneFile {
    version: u32,
    name: Option<String>,
    scene: Scene,
    preview: Vec<u8>,
}

/// A decoded scene plus its metadata.
pub struct LoadedScene {
    pub scene: Scene,
    pub name: Option<String>,
    pub preview: Vec<u8>,
}

#[derive(Debug)]
pub enum SceneFileError {
    BadMagic,
    UnsupportedVersion(u32),
    Decompress,
    Decode,
}

impl std::fmt::Display for SceneFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneFileError::BadMagic => write!(f, "not a .scene file"),
            SceneFileError::UnsupportedVersion(v) => write!(f, "unsupported .scene version {v}"),
            SceneFileError::Decompress => write!(f, "could not decompress"),
            SceneFileError::Decode => write!(f, "could not decode scene"),
        }
    }
}

/// Encode a scene (with optional name + preview PNG bytes) into `.scene` bytes.
pub fn encode(scene: &Scene, name: Option<&str>, preview: &[u8]) -> Vec<u8> {
    let file = SceneFile {
        version: VERSION,
        name: name.map(str::to_string),
        scene: scene.clone(),
        preview: preview.to_vec(),
    };
    let raw = postcard::to_allocvec(&file).expect("postcard encode");
    let compressed = lz4_flex::compress_prepend_size(&raw);
    let mut out = Vec::with_capacity(MAGIC.len() + compressed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&compressed);
    out
}

/// Decode `.scene` bytes. Never panics — malformed input returns `Err`.
pub fn decode(bytes: &[u8]) -> Result<LoadedScene, SceneFileError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(SceneFileError::BadMagic);
    }
    let raw = lz4_flex::decompress_size_prepended(&bytes[MAGIC.len()..])
        .map_err(|_| SceneFileError::Decompress)?;
    let file: SceneFile = postcard::from_bytes(&raw).map_err(|_| SceneFileError::Decode)?;
    if file.version != VERSION {
        return Err(SceneFileError::UnsupportedVersion(file.version));
    }
    Ok(LoadedScene { scene: file.scene, name: file.name, preview: file.preview })
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib scene_file`
Expected: 3 passed.

- [ ] **Step 6: Correct the editable-textures doc note**

In `docs/superpowers/specs/2026-06-29-editable-textures-design.md`, find the `**Format decided:**` line that says zstd, and append a sentence:

```
> Correction (2026-06-29): the codec is postcard + `lz4_flex`, not zstd — zstd's
> C binding does not build for wasm. See the scene-file save/load design.
```

(Insert it right after the existing "Format decided:" paragraph; leave the rest unchanged.)

- [ ] **Step 7: Commit**

```bash
git add src/scene_file.rs src/lib.rs Cargo.toml docs/superpowers/specs/2026-06-29-editable-textures-design.md
git commit -m "feat: .scene container (postcard + lz4_flex + magic/version)"
```

---

## Task 4: Platform save / load (native + web)

**Files:**
- Modify: `Cargo.toml` (move `rfd` to shared; add web-sys File APIs)
- Modify: `src/platform.rs` (`save_scene`, `PickStatus`, `ScenePicker`, `pick_scene`)

**Interfaces:**
- Produces: `platform::save_scene(suggested_name: &str, bytes: &[u8])`; `platform::pick_scene() -> ScenePicker`; `ScenePicker::poll(&self) -> PickStatus`; `enum PickStatus { Pending, Done(Vec<u8>), Cancelled, Failed(String) }`.

- [ ] **Step 1: Cargo — make `rfd` shared, add web-sys upload features**

In `Cargo.toml`:
- Remove `rfd = "0.17.2"` from the `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` table.
- Add `rfd = "0.17.2"` to the shared `[dependencies]` table.
- The wasm table already has `wasm-bindgen-futures`. (No new web-sys features needed — `rfd`'s async backend brings its own.)

- [ ] **Step 2: Add the shared `PickStatus` + `ScenePicker` types and `save_scene`/`pick_scene`**

Append to `src/platform.rs`:

```rust
use std::sync::{Arc, Mutex};

/// Outcome of an async scene-file pick, polled by the UI each frame.
pub enum PickStatus {
    Pending,
    Done(Vec<u8>),
    Cancelled,
    Failed(String),
}

/// Handle to an in-flight (web) or already-resolved (native) file pick.
pub struct ScenePicker {
    slot: Arc<Mutex<Option<PickStatus>>>,
}

impl ScenePicker {
    /// Returns the outcome once, then `Pending` thereafter. Callers should drop
    /// the picker once they get a non-`Pending` status.
    pub fn poll(&self) -> PickStatus {
        self.slot.lock().unwrap().take().unwrap_or(PickStatus::Pending)
    }
}
```

Native `save_scene` + `pick_scene` (cfg-gated):

```rust
#[cfg(not(target_arch = "wasm32"))]
pub fn save_scene(suggested_name: &str, bytes: &[u8]) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Scene", &["scene"])
        .set_file_name(suggested_name)
        .save_file()
    {
        if let Err(e) = std::fs::write(&path, bytes) {
            eprintln!("failed to save {}: {e}", path.display());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn pick_scene() -> ScenePicker {
    let status = match rfd::FileDialog::new().add_filter("Scene", &["scene"]).pick_file() {
        Some(path) => match std::fs::read(&path) {
            Ok(b) => PickStatus::Done(b),
            Err(e) => PickStatus::Failed(e.to_string()),
        },
        None => PickStatus::Cancelled,
    };
    ScenePicker { slot: Arc::new(Mutex::new(Some(status))) }
}
```

Web `save_scene` (Blob download, mirrors `save_png`) + `pick_scene` (async via `rfd::AsyncFileDialog`):

```rust
#[cfg(target_arch = "wasm32")]
pub fn save_scene(suggested_name: &str, bytes: &[u8]) {
    use wasm_bindgen::JsCast;

    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)
        .expect("create blob");
    let url = web_sys::Url::create_object_url_with_blob(&blob).expect("object url");

    let document = web_sys::window().unwrap().document().unwrap();
    let anchor: web_sys::HtmlAnchorElement =
        document.create_element("a").unwrap().dyn_into().unwrap();
    anchor.set_href(&url);
    anchor.set_download(suggested_name);
    anchor.click();
    web_sys::Url::revoke_object_url(&url).ok();
}

#[cfg(target_arch = "wasm32")]
pub fn pick_scene() -> ScenePicker {
    let slot: Arc<Mutex<Option<PickStatus>>> = Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let status = match rfd::AsyncFileDialog::new()
            .add_filter("Scene", &["scene"])
            .pick_file()
            .await
        {
            Some(handle) => PickStatus::Done(handle.read().await),
            None => PickStatus::Cancelled,
        };
        *slot2.lock().unwrap() = Some(status);
    });
    ScenePicker { slot }
}
```

- [ ] **Step 3: Build both targets**

Run: `cargo build`
Expected: native compiles.

Run: `just web-check`
Expected: `wasm32-unknown-unknown` compiles (`rfd` async backend + `wasm-bindgen-futures` build for wasm).

> No unit test here — file dialogs are I/O-bound and not unit-testable; the encode/decode core is covered in Task 3, and end-to-end save/load is verified manually in Task 5. If `rfd::AsyncFileDialog` requires an extra `rfd` feature for the web backend, add it (`rfd = { version = "0.17.2", features = ["..."] }`) and note which.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/platform.rs
git commit -m "feat: native+web save_scene and async pick_scene"
```

---

## Task 5: Preview thumbnail + UI wiring + verification

**Files:**
- Modify: `src/viewer/mod.rs` (thumbnail helper + test; `ViewerApp` fields; Scene row; poll/apply loaded scene)

**Interfaces:**
- Consumes: `scene_file::{encode, decode}`, `platform::{save_scene, pick_scene, ScenePicker, PickStatus}`.
- Produces: Save scene / Load scene buttons; loaded scenes replace the live scene.

- [ ] **Step 1: Write the thumbnail helper test**

Add to the bottom of `src/viewer/mod.rs`:

```rust
#[cfg(test)]
mod thumb_tests {
    use super::*;

    #[test]
    fn thumbnail_shrinks_to_max_edge_and_keeps_aspect() {
        // 800x400 opaque buffer → thumbnail capped at 256 on the long edge.
        let (w, h) = (800u32, 400u32);
        let rgba = vec![128u8; (w * h * 4) as usize];
        let png = scene_thumbnail(&rgba, w, h);
        let img = image::load_from_memory(&png).expect("valid PNG");
        assert!(img.width() <= 256 && img.height() <= 256, "{}x{}", img.width(), img.height());
        assert_eq!(img.width(), 256); // long edge maps to 256
        assert_eq!(img.height(), 128); // aspect preserved
    }

    #[test]
    fn thumbnail_is_empty_for_empty_buffer() {
        assert!(scene_thumbnail(&[], 0, 0).is_empty());
    }
}
```

- [ ] **Step 2: Run — verify it fails**

Run: `cargo test --lib thumb_tests`
Expected: FAIL to compile (`scene_thumbnail` undefined).

- [ ] **Step 3: Implement `scene_thumbnail`**

In `src/viewer/mod.rs`, add near `encode_rgba_png`:

```rust
/// A small PNG preview (≤256px on the long edge, aspect preserved) of the given
/// RGBA frame, for embedding in a `.scene` file. Empty if the buffer is empty.
fn scene_thumbnail(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    if width == 0 || height == 0 || rgba.len() != (width * height * 4) as usize {
        return Vec::new();
    }
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .expect("rgba buffer matches dimensions");
    // `thumbnail` preserves aspect within the 256x256 box.
    let thumb = image::DynamicImage::ImageRgba8(img).thumbnail(256, 256);
    let mut bytes = Vec::new();
    thumb
        .to_rgb8()
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("PNG encode");
    bytes
}
```

- [ ] **Step 4: Run — verify it passes**

Run: `cargo test --lib thumb_tests`
Expected: 2 passed.

- [ ] **Step 5: Add `ViewerApp` fields**

In `src/viewer/mod.rs`, add two fields to `struct ViewerApp`:

```rust
    /// In-flight scene-file load (None when idle).
    scene_picker: Option<crate::platform::ScenePicker>,
    /// Transient status line for scene save/load (e.g. "Loaded scene").
    scene_status: Option<String>,
```

Initialize them in `ViewerApp::new` (in the struct literal):

```rust
            scene_picker: None,
            scene_status: None,
```

- [ ] **Step 6: Poll + apply a loaded scene (top of `ui`)**

In `src/viewer/mod.rs`, in `fn ui`, immediately after `self.render.pump();`, add:

```rust
        // Apply a completed scene load (the picker resolves asynchronously on web).
        if let Some(status) = self.scene_picker.as_ref().map(|p| p.poll()) {
            match status {
                crate::platform::PickStatus::Pending => {
                    ctx.request_repaint(); // keep polling until it resolves
                }
                crate::platform::PickStatus::Done(bytes) => {
                    self.scene_picker = None;
                    match crate::scene_file::decode(&bytes) {
                        Ok(loaded) => {
                            *self.scene.lock().unwrap() = loaded.scene;
                            self.selected = None;
                            self.render.invalidate();
                            self.scene_status = Some("Loaded scene".to_string());
                        }
                        Err(e) => self.scene_status = Some(format!("Load failed: {e}")),
                    }
                }
                crate::platform::PickStatus::Cancelled => self.scene_picker = None,
                crate::platform::PickStatus::Failed(e) => {
                    self.scene_picker = None;
                    self.scene_status = Some(format!("Load failed: {e}"));
                }
            }
        }
```

- [ ] **Step 7: Add the Scene row (Save / Load buttons)**

In `src/viewer/mod.rs`, inside the left panel, right after the Save-image `ui.horizontal(...)` block (the one containing `Save image`) and before its following `ui.separator();`, add:

```rust
                    ui.horizontal(|ui| {
                        if ui.button("Save scene\u{2026}").clicked() {
                            let (rgba, w, h) = {
                                let s = self.render.lock();
                                (s.rgba.clone(), s.width, s.height)
                            };
                            let preview = scene_thumbnail(&rgba, w, h);
                            let bytes = crate::scene_file::encode(&scene, None, &preview);
                            crate::platform::save_scene("scene.scene", &bytes);
                        }
                        if ui.button("Load scene\u{2026}").clicked() {
                            self.scene_picker = Some(crate::platform::pick_scene());
                            self.scene_status = None;
                        }
                        if let Some(msg) = &self.scene_status {
                            ui.weak(msg);
                        }
                    });
```

> `scene` here is the `MutexGuard<Scene>` already locked in the panel closure (`let mut scene = scene_arc.lock().unwrap();`). `encode(&scene, ...)` derefs it to `&Scene`. Assigning `self.scene_picker` / `self.scene_status` inside the closure is fine — they're different fields from the separately-locked `scene_arc`.

- [ ] **Step 8: Build, test, and verify**

Run: `cargo build && cargo test`
Expected: compiles; all tests pass (including the new serde/scene_file/thumb tests).

Run: `just web-check`
Expected: wasm compiles.

Manual (native): `cargo run` → edit the scene (move the short box) → **Save scene…** → choose a path → quit → `cargo run` → **Load scene…** → pick the file → the edited scene loads and renders (box in its moved position). Try **Load scene…** on a non-`.scene` file → a "Load failed: …" message appears, no crash.

Manual (web, optional): `just serve` → **Save scene…** downloads a `.scene`; **Load scene…** opens the browser file picker and loads it back.

- [ ] **Step 9: Commit**

```bash
git add src/viewer/mod.rs
git commit -m "feat: Save/Load scene UI with embedded preview thumbnail"
```

---

## Self-Review notes

- **Spec coverage:** format/codec (T3, Global Constraints), serde derive-in-place (T1) + custom Shape/mesh rebuild (T2), encode/decode + magic/version + no-panic (T3), native+web save/load + async picker (T4), preview thumbnail (T5), UI wiring (T5), zstd→lz4 doc correction (T3). All spec sections mapped.
- **Type consistency:** `MeshData{verts,faces}`, `Shape::Mesh{data,object,render}`, `ScenePicker::poll → PickStatus`, `scene_file::{encode(&Scene,Option<&str>,&[u8]), decode→LoadedScene}` used identically across tasks.
- **Known risks flagged inline:** `Debug` on `Shape` (Arc<dyn> isn't Debug — fallback given), `rfd` async web feature flag, and `world.bounding_box()` accessor existence.
