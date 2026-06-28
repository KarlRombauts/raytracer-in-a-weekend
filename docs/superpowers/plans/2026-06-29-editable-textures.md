# Editable Textures in the Material UI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the material editor author textures (solid / checker / noise / image) on Diffuse, Glossy, and Emission materials, with image assets held as embedded bytes so scenes stay portable.

**Architecture:** Add a plain-data `TextureSpec` (mirroring the core `Texture` types) plus a non-recursive `CellTexture` for checker cells, and an `Asset { bytes, label }` that embeds image bytes. The texturable `MaterialSpec` variants change their flat `Color` field to a `TextureSpec`; `MaterialSpec::build()` builds the concrete `Arc<dyn Texture>` at world-assembly time, falling back to a visible magenta texture if an image fails to decode. The GL preview and the editor's type-switch carry-over both read a representative flat color via `TextureSpec::preview_color()`.

**Tech Stack:** Rust, `image` 0.25 (`load_from_memory`), `eframe`/`egui` 0.34, `rfd` 0.17 (native file dialog).

## Global Constraints

- Textures apply only to `Lambertian` (albedo), `Glossy` (albedo), and `DiffuseLight` (emit). `Metal` and `Dielectric` keep a flat `Color` — the core does not texture them. Do not change them.
- Image assets are stored as embedded bytes (`Asset { bytes: Arc<[u8]>, label: Option<String> }`) — the single source of truth. No filesystem paths are persisted; a path is only a transient input at import time.
- No serialization (serde), no base64, no WASM upload wiring, no OBJ/mesh embedding — those are Phase 2. Do not add them.
- A bad/undecodable image must never panic: it falls back to a visible **magenta** (`Color::new(1.0, 0.0, 1.0)`) solid texture at build time.
- The editor's emission UI edits only the `Solid` color (hue + HDR strength), preserving today's behavior. The data model allows other emit textures for the future, but Phase 1 does not expose them for emission.
- Every task must leave the project compiling (`cargo build`) and the suite green (`cargo test`).

---

### Task 1: Core — decode an image from in-memory bytes

**Files:**
- Modify: `src/texture/image_loader.rs`
- Modify: `src/texture/image_texture.rs`
- Test: `src/texture/image_texture.rs` (inline `#[cfg(test)]` module)

**Interfaces:**
- Produces: `texture::load_image_linear_buffer_from_bytes(&[u8]) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, Box<dyn std::error::Error>>`
- Produces: `ImageTexture::from_bytes(&[u8]) -> Result<ImageTexture, Box<dyn std::error::Error>>` (non-panicking; existing `ImageTexture::new(path)` is left unchanged for the book scenes)

- [ ] **Step 1: Write the failing test**

In `src/texture/image_texture.rs`, append:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::Point3;
    use image::{DynamicImage, ImageFormat, RgbImage};
    use std::io::Cursor;

    fn red_png_bytes() -> Vec<u8> {
        let mut img = RgbImage::new(2, 2);
        for p in img.pixels_mut() {
            *p = image::Rgb([255, 0, 0]);
        }
        let mut bytes: Vec<u8> = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn from_bytes_decodes_a_png() {
        let tex = ImageTexture::from_bytes(&red_png_bytes()).expect("valid png decodes");
        // Uniform red image: any uv samples the same linear-red pixel.
        let c = tex.value(0.5, 0.5, &Point3::new(0.0, 0.0, 0.0));
        assert!(c.x > 0.99 && c.y < 0.01 && c.z < 0.01, "got {c:?}");
    }

    #[test]
    fn from_bytes_rejects_garbage() {
        assert!(ImageTexture::from_bytes(&[1, 2, 3, 4]).is_err());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib texture::image_texture`
Expected: FAIL to compile — `from_bytes` not found.

- [ ] **Step 3: Add the byte-based loader (DRY with the path loader)**

In `src/texture/image_loader.rs`, replace the file with:

```rust
use image::{ImageBuffer, Rgb, RgbImage};
use palette::{IntoColor, LinSrgb, Srgb};

/// Convert an 8-bit sRGB image into a linear-light f32 buffer.
fn rgb8_to_linear(img: RgbImage) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
    let (width, height) = img.dimensions();
    let linear_data: Vec<f32> = img
        .pixels()
        .flat_map(|pixel| {
            let srgb = Srgb::new(
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
            );
            let linear: LinSrgb<f32> = srgb.into_color();
            [linear.red, linear.green, linear.blue]
        })
        .collect();
    ImageBuffer::from_raw(width, height, linear_data).unwrap()
}

pub fn load_image_linear_buffer(
    path: &str,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, Box<dyn std::error::Error>> {
    Ok(rgb8_to_linear(image::open(path)?.to_rgb8()))
}

/// Decode an image from in-memory bytes (format inferred from content), into a
/// linear-light f32 buffer. Used for embedded, portable image assets.
pub fn load_image_linear_buffer_from_bytes(
    bytes: &[u8],
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, Box<dyn std::error::Error>> {
    Ok(rgb8_to_linear(image::load_from_memory(bytes)?.to_rgb8()))
}
```

- [ ] **Step 4: Add `ImageTexture::from_bytes`**

In `src/texture/image_texture.rs`, update the imports and `impl` block. Change the import line to include the new loader:

```rust
use crate::{
    color::Color,
    texture::{load_image_linear_buffer, load_image_linear_buffer_from_bytes, Texture},
    vec3::Point3,
};
```

and add the method inside `impl ImageTexture` (keep `new` as-is):

```rust
    /// Decode an image from embedded bytes. Non-panicking: returns `Err` on an
    /// undecodable buffer so callers can fall back gracefully.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(ImageTexture {
            image: load_image_linear_buffer_from_bytes(bytes)?,
        })
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib texture::image_texture`
Expected: PASS (both tests).

- [ ] **Step 6: Commit**

```bash
git add src/texture/image_loader.rs src/texture/image_texture.rs
git commit -m "feat: decode image textures from in-memory bytes"
```

---

### Task 2: Data model — `TextureSpec`, `CellTexture`, `Asset` + build + preview_color

**Files:**
- Modify: `src/scene.rs` (add types near `MaterialSpec`; extend the existing `#[cfg(test)]` area)
- Test: `src/scene.rs` (new inline `#[cfg(test)]` module `texture_spec_tests`)

**Interfaces:**
- Consumes: `ImageTexture::from_bytes` (Task 1)
- Produces:
  - `Asset { bytes: Arc<[u8]>, label: Option<String> }`, with `Asset::empty() -> Asset`
  - `enum TextureSpec { Solid { color: Color }, Checker { scale: f32, even: CellTexture, odd: CellTexture }, Noise { scale: f32 }, Image { asset: Asset } }`
  - `enum CellTexture { Solid { color: Color }, Noise { scale: f32 }, Image { asset: Asset } }`
  - `TextureSpec::solid(color: Color) -> TextureSpec`
  - `TextureSpec::build(&self) -> Arc<dyn Texture>` and `CellTexture::build(&self) -> Arc<dyn Texture>`
  - `TextureSpec::preview_color(&self) -> Color`

- [ ] **Step 1: Write the failing test**

In `src/scene.rs`, append a new test module:

```rust
#[cfg(test)]
mod texture_spec_tests {
    use super::*;
    use crate::color::Color;
    use crate::vec3::Point3;

    #[test]
    fn solid_builds_and_previews_its_color() {
        let t = TextureSpec::solid(Color::new(0.2, 0.4, 0.6));
        let built = t.build();
        let c = built.value(0.0, 0.0, &Point3::new(0.0, 0.0, 0.0));
        assert!((c.x - 0.2).abs() < 1e-6 && (c.y - 0.4).abs() < 1e-6 && (c.z - 0.6).abs() < 1e-6);
        assert_eq!(t.preview_color(), Color::new(0.2, 0.4, 0.6));
    }

    #[test]
    fn checker_previews_the_average_of_its_cells() {
        let t = TextureSpec::Checker {
            scale: 1.0,
            even: CellTexture::Solid { color: Color::new(0.0, 0.0, 0.0) },
            odd: CellTexture::Solid { color: Color::new(1.0, 1.0, 1.0) },
        };
        let _ = t.build(); // builds without panic
        let p = t.preview_color();
        assert!((p.x - 0.5).abs() < 1e-6 && (p.y - 0.5).abs() < 1e-6 && (p.z - 0.5).abs() < 1e-6);
    }

    #[test]
    fn noise_previews_mid_gray() {
        let t = TextureSpec::Noise { scale: 4.0 };
        let _ = t.build();
        assert_eq!(t.preview_color(), Color::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn bad_image_builds_to_magenta_not_a_panic() {
        let t = TextureSpec::Image { asset: Asset { bytes: vec![1, 2, 3].into(), label: None } };
        let built = t.build(); // must not panic
        let c = built.value(0.5, 0.5, &Point3::new(0.0, 0.0, 0.0));
        assert_eq!(c, Color::new(1.0, 0.0, 1.0));
        // Image preview is a constant neutral gray (no per-frame decode).
        assert_eq!(t.preview_color(), Color::new(0.5, 0.5, 0.5));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib texture_spec_tests`
Expected: FAIL to compile — `TextureSpec` / `CellTexture` / `Asset` not found.

- [ ] **Step 3: Add the types and their builders**

In `src/scene.rs`, update the top-of-file imports to bring in the texture types:

```rust
use crate::material::{Dielectric, DiffuseLight, Glossy, Lambertian, Material, Metal};
use crate::texture::{
    CheckerTexture, ImageTexture, NoiseTexture, SolidColor, Texture,
};
```

(Keep the other existing imports.) Then add, just above `pub enum MaterialSpec`:

```rust
/// An embedded binary asset (image bytes now; meshes in Phase 2). Bytes are the
/// single source of truth, so a scene is self-contained and portable. `label`
/// is for display only (e.g. "earth.png").
#[derive(Clone)]
pub struct Asset {
    pub bytes: Arc<[u8]>,
    pub label: Option<String>,
}

impl Asset {
    /// An asset with no bytes yet — builds to the magenta placeholder until a
    /// file is chosen in the editor.
    pub fn empty() -> Self {
        Asset { bytes: Arc::from([] as [u8; 0]), label: None }
    }
}

/// The magenta sentinel used when an image asset fails to decode.
fn magenta() -> Arc<dyn Texture> {
    Arc::new(SolidColor::from_color(Color::new(1.0, 0.0, 1.0)))
}

/// Plain-data description of a texture, mirroring the core `Texture` types.
#[derive(Clone)]
pub enum TextureSpec {
    Solid { color: Color },
    Checker { scale: f32, even: CellTexture, odd: CellTexture },
    Noise { scale: f32 },
    Image { asset: Asset },
}

/// A checker cell. Deliberately omits `Checker`, so checker-in-checker
/// recursion is unrepresentable (one level of nesting only).
#[derive(Clone)]
pub enum CellTexture {
    Solid { color: Color },
    Noise { scale: f32 },
    Image { asset: Asset },
}

fn build_image(asset: &Asset) -> Arc<dyn Texture> {
    match ImageTexture::from_bytes(&asset.bytes) {
        Ok(t) => Arc::new(t),
        Err(_) => magenta(),
    }
}

impl CellTexture {
    fn build(&self) -> Arc<dyn Texture> {
        match self {
            CellTexture::Solid { color } => Arc::new(SolidColor::from_color(*color)),
            CellTexture::Noise { scale } => Arc::new(NoiseTexture::new(*scale)),
            CellTexture::Image { asset } => build_image(asset),
        }
    }

    fn preview_color(&self) -> Color {
        match self {
            CellTexture::Solid { color } => *color,
            CellTexture::Noise { .. } => Color::new(0.5, 0.5, 0.5),
            CellTexture::Image { .. } => Color::new(0.5, 0.5, 0.5),
        }
    }
}

impl TextureSpec {
    /// A bare flat color is just a solid texture.
    pub fn solid(color: Color) -> Self {
        TextureSpec::Solid { color }
    }

    pub fn build(&self) -> Arc<dyn Texture> {
        match self {
            TextureSpec::Solid { color } => Arc::new(SolidColor::from_color(*color)),
            TextureSpec::Checker { scale, even, odd } => {
                Arc::new(CheckerTexture::from_textures(*scale, even.build(), odd.build()))
            }
            TextureSpec::Noise { scale } => Arc::new(NoiseTexture::new(*scale)),
            TextureSpec::Image { asset } => build_image(asset),
        }
    }

    /// A representative flat color for the rasterized preview and the editor's
    /// type-switch carry-over. Cheap and deterministic — never decodes an image
    /// (the preview runs every frame), so images report a neutral gray.
    pub fn preview_color(&self) -> Color {
        match self {
            TextureSpec::Solid { color } => *color,
            TextureSpec::Checker { even, odd, .. } => {
                (even.preview_color() + odd.preview_color()) * 0.5
            }
            TextureSpec::Noise { .. } => Color::new(0.5, 0.5, 0.5),
            TextureSpec::Image { .. } => Color::new(0.5, 0.5, 0.5),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib texture_spec_tests`
Expected: PASS (all four tests).

- [ ] **Step 5: Commit**

```bash
git add src/scene.rs
git commit -m "feat: TextureSpec/CellTexture plain-data model with build + preview"
```

---

### Task 3: Migrate `MaterialSpec` to `TextureSpec` and fix every call site

This task changes three `MaterialSpec` variants from a flat `Color` to a `TextureSpec`, then repairs every place that constructs or reads them so the project compiles again. It lands atomically.

**Files:**
- Modify: `src/material/glossy.rs` (add `from_texture`)
- Modify: `src/scene.rs` (`MaterialSpec` variants + `MaterialSpec::build`)
- Modify: `src/scenes/cornell_box.rs` (Lambertian + DiffuseLight constructions)
- Modify: `src/scenes/new_bvh.rs` (Lambertian construction)
- Modify: `src/viewer/raster/renderer.rs` (`preview_color`)
- Modify: `src/viewer/raster/pick.rs` (test fixture)
- Modify: `src/viewer/controls.rs` (`shared_color` + the `pick` builder closures only — the texture editor UI is Task 4)

**Interfaces:**
- Consumes: `TextureSpec::{solid, build, preview_color}` (Task 2)
- Produces:
  - `Glossy::from_texture(texture: Arc<dyn Texture>, roughness: f32) -> Glossy`
  - `MaterialSpec::Lambertian { albedo: TextureSpec }`, `MaterialSpec::Glossy { albedo: TextureSpec, roughness: f32 }`, `MaterialSpec::DiffuseLight { emit: TextureSpec }` (Metal/Dielectric unchanged)

- [ ] **Step 1: Add `Glossy::from_texture`**

In `src/material/glossy.rs`, inside `impl Glossy` (after `new`):

```rust
    pub fn from_texture(texture: Arc<dyn Texture>, roughness: f32) -> Self {
        Glossy { texture, roughness }
    }
```

- [ ] **Step 2: Change the `MaterialSpec` variants and `build()`**

In `src/scene.rs`, change the three variants:

```rust
pub enum MaterialSpec {
    Lambertian { albedo: TextureSpec },
    Glossy { albedo: TextureSpec, roughness: f32 },
    Metal { albedo: Color, fuzz: f32 },
    Dielectric { ior: f32, tint: Color, roughness: f32 },
    DiffuseLight { emit: TextureSpec },
}
```

and update the matching arms in `MaterialSpec::build`:

```rust
            MaterialSpec::Lambertian { albedo } => {
                Arc::new(Lambertian::from_texture(albedo.build()))
            }
            MaterialSpec::Glossy { albedo, roughness } => {
                Arc::new(Glossy::from_texture(albedo.build(), *roughness))
            }
            MaterialSpec::Metal { albedo, fuzz } => Arc::new(Metal::new(*albedo, *fuzz)),
            MaterialSpec::Dielectric { ior, tint, roughness } => {
                Arc::new(Dielectric::new_glass(*ior, *tint, *roughness))
            }
            MaterialSpec::DiffuseLight { emit } => {
                Arc::new(DiffuseLight::from_texture(emit.build()))
            }
```

- [ ] **Step 3: Fix the book scenes**

In `src/scenes/cornell_box.rs`, wrap each Lambertian albedo and the light's emit in `TextureSpec::solid(...)`. Add `TextureSpec` to the `use crate::{... scene::{...}}` import. Each material becomes, e.g.:

```rust
    let red = MaterialSpec::Lambertian { albedo: TextureSpec::solid(Color::new(0.65, 0.05, 0.05)) };
    let white = MaterialSpec::Lambertian { albedo: TextureSpec::solid(Color::new(0.73, 0.73, 0.73)) };
    let green = MaterialSpec::Lambertian { albedo: TextureSpec::solid(Color::new(0.12, 0.45, 0.15)) };
    let light = MaterialSpec::DiffuseLight { emit: TextureSpec::solid(Color::new(15.0, 15.0, 15.0)) };
```

In `src/scenes/new_bvh.rs`, update its import and the Lambertian construction (the `Metal` one is unchanged):

```rust
        material: MaterialSpec::Lambertian { albedo: TextureSpec::solid(Color::new(0.8, 0.8, 0.8)) },
```

- [ ] **Step 4: Fix the rasterizer preview**

In `src/viewer/raster/renderer.rs`, update `preview_color` (around line 569) to read texture colors via `TextureSpec::preview_color`:

```rust
fn preview_color(m: &MaterialSpec) -> ([f32; 3], bool) {
    match m {
        MaterialSpec::Lambertian { albedo } => {
            let c = albedo.preview_color();
            ([c.x, c.y, c.z], false)
        }
        MaterialSpec::Glossy { albedo, .. } => {
            let c = albedo.preview_color();
            ([c.x, c.y, c.z], false)
        }
        MaterialSpec::Metal { albedo, .. } => ([albedo.x, albedo.y, albedo.z], false),
        MaterialSpec::Dielectric { .. } => ([0.85, 0.9, 1.0], false),
        MaterialSpec::DiffuseLight { emit } => {
            let c = emit.preview_color();
            ([c.x, c.y, c.z], true)
        }
    }
}
```

- [ ] **Step 5: Fix the `pick.rs` test fixture**

In `src/viewer/raster/pick.rs` (around line 79), wrap the test material's albedo. Add `TextureSpec` to that test module's `use crate::scene::{...}` import:

```rust
            material: MaterialSpec::Lambertian { albedo: TextureSpec::solid(Color::new(0.5, 0.5, 0.5)) },
```

- [ ] **Step 6: Fix `shared_color` and the `pick` builders in controls.rs**

In `src/viewer/controls.rs`, add `TextureSpec` to the `use crate::scene::{...}` import. Update `shared_color` (line ~260) so the texturable variants read their representative color:

```rust
fn shared_color(m: &MaterialSpec) -> Color {
    match m {
        MaterialSpec::Lambertian { albedo } => albedo.preview_color(),
        MaterialSpec::Glossy { albedo, .. } => albedo.preview_color(),
        MaterialSpec::Metal { albedo, .. } => *albedo,
        MaterialSpec::Dielectric { tint, .. } => *tint,
        MaterialSpec::DiffuseLight { emit } => {
            let e = emit.preview_color();
            e / e.x.max(e.y).max(e.z).max(1e-4)
        }
    }
}
```

Update the three `pick(...)` builder closures (lines ~303-329) that construct texturable variants to wrap the carried color in `TextureSpec::solid`:

```rust
                c |= pick(ui, m, matches!(m, MaterialSpec::Lambertian { .. }), "Diffuse", |col, _| {
                    MaterialSpec::Lambertian { albedo: TextureSpec::solid(col) }
                });
                c |= pick(ui, m, matches!(m, MaterialSpec::Glossy { .. }), "Glossy", |col, r| {
                    MaterialSpec::Glossy { albedo: TextureSpec::solid(col), roughness: r }
                });
```

(Leave the Metal and Dielectric `pick` closures unchanged.) For the Emission `pick` closure:

```rust
                c |= pick(ui, m, matches!(m, MaterialSpec::DiffuseLight { .. }), "Emission", |col, _| {
                    MaterialSpec::DiffuseLight { emit: TextureSpec::solid(col * 5.0) }
                });
```

- [ ] **Step 7: Temporarily fix the per-material param UI so it compiles**

The `match m { ... }` block at lines ~334-366 reads `albedo`/`emit` as `Color` for the property rows; that becomes the real texture editor in Task 4. For now, make it compile by reading/writing through a temporary solid color so behavior is preserved. Replace the Lambertian, Glossy, and DiffuseLight arms with:

```rust
        MaterialSpec::Lambertian { albedo } => {
            let mut c = albedo.preview_color();
            if color_prop(ui, "Color", &mut c) {
                *albedo = TextureSpec::solid(c);
                changed = true;
            }
        }
        MaterialSpec::Glossy { albedo, roughness } => {
            let mut c = albedo.preview_color();
            if color_prop(ui, "Color", &mut c) {
                *albedo = TextureSpec::solid(c);
                changed = true;
            }
            changed |= axis_row(ui, "Roughness", roughness, 0.01, "", Some(3), Some(0.0..=1.0));
        }
```

(Metal and Dielectric arms unchanged.) For the DiffuseLight arm, keep the existing hue+strength logic but source/store the color through the `emit` texture's solid color:

```rust
        MaterialSpec::DiffuseLight { emit } => {
            let e = emit.preview_color();
            let intensity = e.x.max(e.y).max(e.z).max(1e-4);
            let mut rgb = [e.x / intensity, e.y / intensity, e.z / intensity];
            let mut strength = intensity;
            let col = prop_row(ui, "Color", |ui| ui.color_edit_button_rgb(&mut rgb).changed());
            let str_changed =
                axis_row(ui, "Strength", &mut strength, 0.1, "", Some(2), Some(0.0..=10_000.0));
            if col || str_changed {
                *emit = TextureSpec::solid(Color::new(
                    rgb[0] * strength,
                    rgb[1] * strength,
                    rgb[2] * strength,
                ));
                changed = true;
            }
        }
```

- [ ] **Step 8: Build and run the whole suite**

Run: `cargo build && cargo test`
Expected: compiles clean; all tests pass (existing tests + Tasks 1-2 tests).

- [ ] **Step 9: Commit**

```bash
git add src/material/glossy.rs src/scene.rs src/scenes/cornell_box.rs src/scenes/new_bvh.rs src/viewer/raster/renderer.rs src/viewer/raster/pick.rs src/viewer/controls.rs
git commit -m "feat: MaterialSpec carries TextureSpec for diffuse/glossy/emission"
```

---

### Task 4: Texture editor UI (type picker, checker cells, image file dialog)

Replaces the temporary solid-only color rows from Task 3 (for Diffuse and Glossy) with a full texture editor: a type dropdown plus per-type params, including a `CellTexture` editor for checker cells and an `rfd` image picker that reads bytes into an `Asset`. Emission keeps its Solid-only hue+strength editor (per Global Constraints).

**Files:**
- Modify: `src/viewer/controls.rs`

**Interfaces:**
- Consumes: `TextureSpec`, `CellTexture`, `Asset` (Task 2); `TextureSpec::preview_color` for type-switch carry-over.
- Produces (private helpers): `texture_controls(ui, t: &mut TextureSpec) -> bool`, `cell_texture_controls(ui, id: &str, t: &mut CellTexture) -> bool`, `image_picker_row(ui, asset: &mut Asset) -> bool`.

- [ ] **Step 1: Add `CellTexture`/`Asset` to imports**

In `src/viewer/controls.rs`, extend the scene import:

```rust
use crate::scene::{self, Asset, CellTexture, MaterialSpec, ObjectSpec, Shape, TextureSpec, Transform};
```

- [ ] **Step 2: Add the texture editor helpers**

Add these functions near `material_controls` (e.g. just after it). The combo switches variant while carrying the representative color over via `preview_color`; defaults: checker `scale = 1.0` with black/white cells, noise `scale = 4.0`, image an empty asset (renders magenta until a file is chosen).

```rust
/// Full texture editor: a type dropdown + per-type parameters. Used for the
/// albedo of Diffuse/Glossy materials.
fn texture_controls(ui: &mut egui::Ui, t: &mut TextureSpec) -> bool {
    let mut changed = false;
    let current = match t {
        TextureSpec::Solid { .. } => "Color",
        TextureSpec::Checker { .. } => "Checker",
        TextureSpec::Noise { .. } => "Noise",
        TextureSpec::Image { .. } => "Image",
    };

    changed |= prop_row(ui, "Texture", |ui| {
        let mut c = false;
        egui::ComboBox::from_id_salt("texture_type")
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                let prev = t.preview_color();
                if ui.selectable_label(matches!(t, TextureSpec::Solid { .. }), "Color").clicked() {
                    *t = TextureSpec::Solid { color: prev };
                    c = true;
                }
                if ui.selectable_label(matches!(t, TextureSpec::Checker { .. }), "Checker").clicked() {
                    *t = TextureSpec::Checker {
                        scale: 1.0,
                        even: CellTexture::Solid { color: prev },
                        odd: CellTexture::Solid { color: Color::new(1.0, 1.0, 1.0) },
                    };
                    c = true;
                }
                if ui.selectable_label(matches!(t, TextureSpec::Noise { .. }), "Noise").clicked() {
                    *t = TextureSpec::Noise { scale: 4.0 };
                    c = true;
                }
                if ui.selectable_label(matches!(t, TextureSpec::Image { .. }), "Image").clicked() {
                    *t = TextureSpec::Image { asset: Asset::empty() };
                    c = true;
                }
            });
        c
    });

    match t {
        TextureSpec::Solid { color } => changed |= color_prop(ui, "Color", color),
        TextureSpec::Checker { scale, even, odd } => {
            changed |= axis_row(ui, "Scale", scale, 0.01, "", Some(3), Some(0.01..=100.0));
            changed |= cell_texture_controls(ui, "checker_even", even);
            changed |= cell_texture_controls(ui, "checker_odd", odd);
        }
        TextureSpec::Noise { scale } => {
            changed |= axis_row(ui, "Scale", scale, 0.01, "", Some(3), Some(0.01..=100.0));
        }
        TextureSpec::Image { asset } => changed |= image_picker_row(ui, asset),
    }
    changed
}

/// Editor for one checker cell (Solid / Noise / Image — no nested checker).
fn cell_texture_controls(ui: &mut egui::Ui, id: &str, t: &mut CellTexture) -> bool {
    let mut changed = false;
    let current = match t {
        CellTexture::Solid { .. } => "Color",
        CellTexture::Noise { .. } => "Noise",
        CellTexture::Image { .. } => "Image",
    };

    changed |= prop_row(ui, "Cell", |ui| {
        let mut c = false;
        egui::ComboBox::from_id_salt(id)
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                if ui.selectable_label(matches!(t, CellTexture::Solid { .. }), "Color").clicked() {
                    *t = CellTexture::Solid { color: Color::new(0.0, 0.0, 0.0) };
                    c = true;
                }
                if ui.selectable_label(matches!(t, CellTexture::Noise { .. }), "Noise").clicked() {
                    *t = CellTexture::Noise { scale: 4.0 };
                    c = true;
                }
                if ui.selectable_label(matches!(t, CellTexture::Image { .. }), "Image").clicked() {
                    *t = CellTexture::Image { asset: Asset::empty() };
                    c = true;
                }
            });
        c
    });

    match t {
        CellTexture::Solid { color } => changed |= color_prop(ui, "Color", color),
        CellTexture::Noise { scale } => {
            changed |= axis_row(ui, "Scale", scale, 0.01, "", Some(3), Some(0.01..=100.0));
        }
        CellTexture::Image { asset } => changed |= image_picker_row(ui, asset),
    }
    changed
}

/// A row showing the current image label and a button that opens a native file
/// dialog, reading the chosen file's bytes straight into the embedded `Asset`.
fn image_picker_row(ui: &mut egui::Ui, asset: &mut Asset) -> bool {
    let mut changed = false;
    prop_row(ui, "Image", |ui| {
        let label = asset.label.clone().unwrap_or_else(|| "(none)".to_string());
        if ui.button("Choose\u{2026}").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg"])
                .pick_file()
            {
                if let Ok(bytes) = std::fs::read(&path) {
                    asset.bytes = bytes.into();
                    asset.label = path.file_name().map(|s| s.to_string_lossy().into_owned());
                    changed = true;
                }
            }
        }
        ui.label(label);
    });
    changed
}
```

- [ ] **Step 3: Use the texture editor for Diffuse and Glossy**

In `material_controls`, replace the temporary Lambertian and Glossy arms from Task 3 (Step 7) with the real editor:

```rust
        MaterialSpec::Lambertian { albedo } => changed |= texture_controls(ui, albedo),
        MaterialSpec::Glossy { albedo, roughness } => {
            changed |= texture_controls(ui, albedo);
            changed |= axis_row(ui, "Roughness", roughness, 0.01, "", Some(3), Some(0.0..=1.0));
        }
```

(Leave the Metal, Dielectric, and DiffuseLight arms exactly as they are after Task 3 — emission stays Solid-only.)

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`
Expected: compiles clean; all tests pass.

- [ ] **Step 5: Manual smoke test**

Run: `cargo run`
Verify, in the editor:
1. Select an object, set its material to **Diffuse**, change **Texture** to **Checker** — two `Cell` rows + a `Scale` appear; the preview shows a mid-gray-ish flat color and the render shows a checkerboard.
2. Set **Texture** to **Noise** — a marbled pattern renders.
3. Set **Texture** to **Image**, click **Choose…**, pick a PNG/JPG — the label updates and the render shows the image mapped onto the surface. (Before choosing, the surface renders magenta — expected.)
4. A checker **Cell** set to **Image** with no file chosen renders magenta in that cell — expected, no crash.
5. Switch the material to **Emission** — the Color + Strength rows still work (HDR lights unchanged).

- [ ] **Step 6: Commit**

```bash
git add src/viewer/controls.rs
git commit -m "feat: texture editor UI (solid/checker/noise/image) for materials"
```

---

## Notes / deviations from the spec

- **Image preview color** is a constant neutral gray `(0.5, 0.5, 0.5)` rather than the image's average pixel. The spec floated "average pixel if loaded"; decoding every frame in the rasterizer's `preview_color` would be too costly, so a constant gray is used. The path-traced render still uses the real image (decoded once at world assembly).
- **Emission** keeps a Solid-only editor (hue + HDR strength). The data model stores `emit: TextureSpec`, so emissive image/checker/noise textures are representable for the future, but the Phase 1 editor does not expose them — HDR strength matters more for lights and a scaled emissive texture is extra scope (YAGNI).
```
