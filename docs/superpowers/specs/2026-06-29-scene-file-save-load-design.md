# `.scene` save / load (Phase 2A) — Design

Date: 2026-06-29
Status: Approved (pending spec review)

## Goal

Add a portable `.scene` file format and the Save/Load wiring so a scene (camera +
objects, including imported OBJ meshes and embedded image textures) can be
written to disk / downloaded and read back faithfully, on both native and the
web build. This is the persistence foundation ("Phase A") of the larger scene
library; the curated **bundled scene picker with preview thumbnails ("Phase B")**
builds on this format but is out of scope here.

## Context

The editable data model already exists (`src/scene.rs`): `Scene { camera, objects }`,
`ObjectSpec { name, shape, material, transform }`, plain-data `TextureSpec` /
`CellTexture` / `MaterialSpec`, and `Asset { bytes: Arc<[u8]>, label }` which
already holds image bytes inline. Almost everything is plain data and
serde-derivable.

The one runtime-only spot is `Shape::Mesh { object: Arc<dyn Intersect>, render:
Arc<RenderMesh> }` — a built BVH plus preview mesh. The OBJ loader
(`src/geometry/obj_loader.rs`) shows the source data is minimal: `ObjData { verts:
Vec<Vec3>, faces: Vec<[usize; 3]> }`. Triangle normals are geometric (computed in
`Triangle::from_points`), preview normals come from
`RenderMesh::from_triangles_smooth(verts, faces)`, and the BVH is built from the
triangles. So a mesh is fully described by **positions + triangle indices**, and
everything else is rebuilt from those via existing functions.

### Decisions carried in / corrected

- **Format codec corrected:** the earlier editable-textures spec recorded `.scene`
  = *zstd*-compressed postcard. zstd (`zstd-sys`, a C binding) does not compile for
  `wasm32-unknown-unknown`, and this feature must encode/decode in the browser.
  **Corrected to postcard + `lz4_flex`** (pure-Rust, wasm-safe, compresses both
  directions). The editable-textures design doc will be updated to note this.
- **Serialization strategy:** derive `serde` in place on the existing types; only
  `Shape` gets hand-written (de)serialization.
- **Mesh support is in scope** for this iteration.
- **Full round-trip on both platforms**, including async file *upload* on web.

## File format

`.scene` bytes = `MAGIC (4) ++ lz4(postcard(SceneFile))`:

```rust
struct SceneFile {
    version: u32,            // start at 1; load rejects unknown versions
    name: Option<String>,    // for the library (Phase B); carried now
    scene: Scene,            // camera + objects; image assets inline
    preview: Vec<u8>,        // PNG thumbnail bytes (may be empty)
}
```

- Magic: `b"RTSC"`. Sanity-checks that a file is one of ours before decompressing.
- **Encode:** `postcard::to_allocvec(&file)` → `lz4_flex::compress_prepend_size`
  → prepend magic.
- **Decode:** verify magic → `lz4_flex::decompress_size_prepended` →
  `postcard::from_bytes::<SceneFile>` → reject unknown `version`.
- Rationale: mesh float arrays and structural data compress well; embedded images
  are already compressed and pass through. Pure-Rust toolchain builds on wasm.

## Serializing the data model (derive-in-place)

- Add `#[derive(Serialize, Deserialize)]` to: `Vec3`, `Color`, `Mapping`,
  `Projection`, `TextureSpec`, `CellTexture`, `MaterialSpec`, `Transform`,
  `ObjectSpec`, `Scene`, `CameraConfig`. Add `PartialEq` where round-trip tests
  need it.
- `Asset.bytes: Arc<[u8]>` (de)serialized via a small `#[serde(with = "...")]`
  byte helper (serialize as a byte slice, deserialize into a fresh `Arc<[u8]>`) —
  avoids enabling serde's global `rc` feature.
- **`Shape` — the only hand-written case.** Add:
  ```rust
  struct MeshData { verts: Vec<Vec3>, faces: Vec<[u32; 3]> }
  ```
  Store it on `Shape::Mesh` next to the runtime handles. A serde proxy enum
  `ShapeData { Sphere{..}, Quad{..}, Box{..}, Mesh{ data: MeshData } }` is the
  on-disk form. Custom `Serialize for Shape` maps to `ShapeData` (Mesh writes only
  `MeshData`); custom `Deserialize for Shape` reads `ShapeData` and, for `Mesh`,
  **rebuilds** `object` (BVH from `into_triangles`) and `render`
  (`from_triangles_smooth`) from the arrays. The mesh keeps the baked grey
  Lambertian material, matching today's OBJ import (per-mesh material editing
  remains out of scope, unchanged from current behavior).

## Encode / decode module

New `src/scene_file.rs`:
- `pub fn encode(scene: &Scene, name: Option<&str>, preview: &[u8]) -> Vec<u8>`
- `pub fn decode(bytes: &[u8]) -> Result<LoadedScene, SceneFileError>` where
  `LoadedScene { scene: Scene, name: Option<String>, preview: Vec<u8> }`.
- `SceneFileError` covers bad magic, unknown version, decompress failure, and
  postcard failure. **Load never panics — it returns `Err`.**

## Save / load platform layer (extends `src/platform.rs`, cfg-split)

- `save_scene(suggested_name: &str, bytes: &[u8])`:
  - native: `rfd` save dialog (filter `*.scene`) → `std::fs::write`.
  - web: `web-sys` Blob + `<a download>` (mirrors the existing `save_png`).
- Loading is async on web, so a polled handle:
  - `pub fn pick_scene() -> ScenePicker` opens the file dialog.
  - `ScenePicker::take(&self) -> Option<Result<Vec<u8>, ScenePickError>>` — the app
    polls each frame; `Some` once the bytes are ready (or the pick failed/was
    cancelled → `None` stays, or an `Err` for a read failure).
  - native: fills synchronously (`rfd` open + `fs::read`).
  - web: `wasm_bindgen_futures::spawn_local` drives an `<input type=file>` +
    `FileReader`, delivering bytes into an `Arc<Mutex<Option<...>>>` slot the handle
    exposes.

## Preview thumbnail

At save time: read the displayed frame (`SharedFrame` rgba + dims), downscale to a
max edge of 256px (`image::imageops::thumbnail`), PNG-encode, embed as
`SceneFile.preview`. Empty if nothing has rendered yet. Not displayed in this
iteration — generated and embedded only so Phase B's picker can show it without a
format change.

## UI wiring (`src/viewer/mod.rs`)

A small **Scene** row in the side panel with **Save scene…** and **Load scene…**:
- Save → `scene_file::encode(current scene, name, thumbnail)` → `platform::save_scene`.
- Load → `platform::pick_scene()`, stash the `ScenePicker` on `ViewerApp`, poll it
  each frame; on bytes → `scene_file::decode` → replace `self.scene` contents,
  clear `selected`, `render.invalidate()`. Decode error → a brief inline status
  message; never a crash.

## Testing

- **Round-trip** (native unit test in `scene_file`): a scene containing each
  primitive (`Sphere`/`Quad`/`Box`), each texture variant (`Solid`/`Checker`/
  `Noise`/`Image` with real bytes), and a small mesh → `encode` → `decode` →
  assert camera fields, per-object shape/transform/material, asset bytes, and mesh
  `verts`/`faces` match.
- **Mesh rebuild:** a decoded mesh scene → `build_world` succeeds and the object's
  bounding box is finite/non-degenerate.
- **Robustness:** corrupt bytes, wrong magic, and an unknown `version` each return
  `Err` (no panic).
- **Preview:** a `SceneFile` with `preview` bytes round-trips and the preview
  decodes to a small image of the expected dimensions.
- Native: `cargo test`. Wasm: `just web-check` (lz4_flex, postcard, rfd async, and
  the `web-sys` File/FileReader APIs all build for `wasm32-unknown-unknown`).

## Out of scope (deferred)

- **Phase B:** bundled curated scenes + the scene-library picker UI that displays
  preview thumbnails.
- Per-mesh material editing (pre-existing limitation; unchanged).
- Scene metadata beyond `name` + `preview`.
