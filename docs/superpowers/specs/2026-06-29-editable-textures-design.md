# Editable Textures in the Material UI — Design

**Date:** 2026-06-29
**Status:** Approved (pending spec review)

## Context

This is **Phase 1** of a larger goal: a portable, serialized **scene library**
(view / edit / render) with a default blank scene for authoring new ones. The
library stores scenes as serialized files (option B — see roadmap below), which
makes serialization the backbone of Phase 2.

Several of the "show off the renderer" scenes we want in that library lean on
textures — a checkered floor under glass, a marble (Perlin) sphere, an
image-mapped earth. The renderer **core already has a full texture system**
(`SolidColor`, `CheckerTexture`, `NoiseTexture`/`Perlin`, `ImageTexture`), and
the texturable materials (`Lambertian`, `Glossy`, `DiffuseLight`) already accept
`Arc<dyn Texture>`. The only gap is the **editable plain-data layer**:
`MaterialSpec` carries a flat `Color albedo`, so the editor and (future) file
format can't express anything but a solid color.

Phase 1 closes that gap: add a plain-data `TextureSpec`, wire it through
`MaterialSpec::build()`, and add a texture picker to the material editor.
Doing it first **finalizes the data model** so Phase 2 doesn't churn the file
format.

### Roadmap (context, not this spec)

- **Phase 1 (this spec):** textures in the editable layer + UI. In-memory only.
- **Phase 2:** scene library — serde on the scene types, `.ron` load/save,
  scene picker, default/blank scene, curated sample scenes, OBJ/mesh embedding,
  WASM file-upload wiring.

## Scope for this phase

**In:** a plain-data texture model that the material editor can author, built
into the existing `dyn Texture` types at world-assembly time, with image assets
held as **embedded bytes** (portable by construction). Native file-picking for
images via the existing `rfd` dialog.

**Out (deferred to Phase 2 or later):**
- Serialization / base64 of scenes and assets.
- WASM file-*upload* wiring (the data model is ready — see Asset — but only the
  native path is implemented now).
- Mesh / OBJ embedding (Phase 2; meshes are still baked-at-import today).
- Texturing `Metal` and `Dielectric` — the core doesn't texture them, and
  extending it is out of scope (YAGNI).

## Design

### 1. Data model (`scene.rs`)

Image and (later) mesh assets are stored as **embedded bytes** — the single
source of truth — so a scene is portable with no external file dependencies. A
filesystem path is only a transient input at import time. This also makes WASM
upload a fill-in-the-branch change later (the browser hands us bytes, not paths).

```rust
/// An embedded binary asset (image now; meshes in Phase 2). Bytes are the
/// source of truth so scenes are self-contained and portable. `label` is for
/// display only (e.g. "earth.png").
#[derive(Clone)]
pub struct Asset {
    pub bytes: Arc<[u8]>,
    pub label: Option<String>,
}

/// Plain-data description of a texture, mirroring the core `Texture` types.
#[derive(Clone)]
pub enum TextureSpec {
    Solid   { color: Color },
    Checker { scale: f32, even: CellTexture, odd: CellTexture },
    Noise   { scale: f32 },
    Image   { asset: Asset },
}

/// A checker cell. Deliberately omits `Checker`, so checker-in-checker
/// recursion is unrepresentable by construction (one level of nesting only).
#[derive(Clone)]
pub enum CellTexture {
    Solid { color: Color },
    Noise { scale: f32 },
    Image { asset: Asset },
}
```

Texturable `MaterialSpec` variants change from a flat `Color` to a `TextureSpec`.
A bare color is just `TextureSpec::Solid`:

```rust
pub enum MaterialSpec {
    Lambertian   { albedo: TextureSpec },
    Glossy       { albedo: TextureSpec, roughness: f32 },
    Metal        { albedo: Color, fuzz: f32 },          // unchanged — flat color
    Dielectric   { ior: f32, tint: Color, roughness: f32 }, // unchanged — flat color
    DiffuseLight { emit: TextureSpec },
}
```

### 2. Core additions (`texture/`)

- `load_image_linear_buffer_from_bytes(&[u8]) -> Result<ImageBuffer<…>, _>` —
  mirrors the existing path loader but decodes with `image::load_from_memory`
  (format inferred from content/magic bytes, not extension). Keeps the same
  sRGB→linear conversion.
- `ImageTexture::from_bytes(&[u8]) -> Result<Self, _>` — **non-panicking**
  (today's `ImageTexture::new` does `.unwrap()`; the new path returns `Result`
  so the editor can fall back gracefully). `new(path)` may be kept for the
  existing book-style scenes or reimplemented on top of `from_bytes`.
- `TextureSpec::build(&self) -> Arc<dyn Texture>`, and `CellTexture::build`,
  wired into `MaterialSpec::build()`. `Checker` builds via
  `CheckerTexture::from_textures` with each cell built recursively (one level).

### 3. Preview color

The rasterized GL preview can't render checker/noise/image, and the material
editor's type-switch carry-over (`shared_color`) needs a representative flat
color. One helper serves both:

```rust
impl TextureSpec {
    pub fn preview_color(&self) -> Color { … }
}
```

- `Solid`   → its color
- `Checker` → average of the two cells' preview colors
- `Noise`   → mid-gray
- `Image`   → average pixel color if the asset decodes, else gray

`shared_color` (controls.rs) derives its carry-over color from
`preview_color()` so switching between a textured material and a flat-color one
(Metal/Dielectric) keeps a sensible color.

### 4. UI (`controls.rs`)

Where the albedo/emit color picker is today, render a **texture sub-editor**:

- A type dropdown: **Solid / Checker / Noise / Image** (Blender-ish names ok).
- **Solid** → color picker (the current behavior).
- **Checker** → `scale` drag + two `CellTexture` sub-pickers (Solid / Noise /
  Image each).
- **Noise** → `scale` slider.
- **Image** → a "Choose image…" button reusing the existing
  `rfd::FileDialog` pattern from the OBJ importer. On pick: read the file bytes
  immediately into an `Asset` (with `label` = file name); show the label and a
  re-pick button. Switching type carries the color over via `preview_color`.

Returns `changed` like the existing `material_controls`, so edits re-trigger the
render.

### 5. Error handling

Textures are built lazily at world assembly (`MaterialSpec::build`), so a bad or
undecodable image **never panics**: `ImageTexture::from_bytes` returns `Err`, and
the builder substitutes a **visible magenta** solid texture so the mistake is
obvious in the render rather than silent or fatal.

## Testing

- `TextureSpec::build` / `CellTexture::build` produce the expected `dyn Texture`
  (smoke: build each variant without panic; checker nests one level).
- `preview_color` returns the documented values (solid passes through; checker
  averages; noise/image gray-ish).
- `ImageTexture::from_bytes` round-trips a tiny in-memory PNG (encode → decode →
  sample a known pixel) and returns `Err` on garbage bytes.
- The magenta fallback is exercised: a `TextureSpec::Image` with garbage bytes
  builds to a non-panicking texture whose sampled color is the magenta sentinel.

## Open questions

None blocking. Phase 2 will decide path-vs-embedded *serialization* (we've chosen
embedded for portability) and whether to gzip embedded blobs.
