# Texture UV Projection + Noise Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix/expose the noise texture's scale + turbulence depth, and add UV mapping/projection (MeshUv/Planar/Spherical/Cylindrical + scale/offset) to image textures, both editable in the material UI.

**Architecture:** `NoiseTexture` gains a `depth` and actually applies `scale`. A new `MappedTexture` wrapper remaps `(u,v)` by a `Projection` + scale/offset; `TextureSpec::Image` carries a `Mapping` that `build()` wraps the image in. Editor exposes both.

**Tech Stack:** Rust, existing `texture`/`scene`/`eframe`-egui, `Perlin`.

## Global Constraints

- `NoiseTexture::new(scale: f32, depth: u32)`; `value` = `Color::ones() * self.noise.turb(&(self.scale * *p), self.depth)`.
- `TextureSpec::Noise { scale: f32, depth: u32 }` and `CellTexture::Noise { scale: f32, depth: u32 }`. Default depth when created in the editor = `7`.
- `Projection { MeshUv, Planar, Spherical, Cylindrical }` in `texture/mapped_texture.rs`; derived only from `(u,v,p)` — NO `Texture` trait change.
- `MappedTexture::value` wraps final uv with `rem_euclid(1.0)` (so `scale>1` tiles — `ImageTexture` clamps uv). Planar = XZ plane `(p.x, p.z)`.
- `Mapping { projection: Projection, scale: f32, offset: (f32, f32) }` in `scene.rs`, `Default` = `{ MeshUv, 1.0, (0.0, 0.0) }`. Only on `TextureSpec::Image`.
- `Image` `build()`: identity mapping → bare image texture; else wrap in `MappedTexture`.
- Build pristine — no new warnings. Update ALL `NoiseTexture::new` and `Noise {`/`Image {` construction sites so the project compiles.

---

### Task 1: Noise scale + depth

**Files:**
- Modify: `src/texture/noise_texture.rs` (depth field + apply scale + test)
- Modify: `src/scene.rs` (`TextureSpec::Noise`/`CellTexture::Noise` add `depth`; build sites; any Noise constructions in tests)
- Modify: `src/scenes/simple_light.rs`, `src/scenes/perlin_spheres.rs` (`NoiseTexture::new(4., 7)`)
- Modify: `src/viewer/controls.rs` (noise rows: scale + depth; Noise creation sites add `depth: 7`)

**Interfaces:**
- Produces: `NoiseTexture::new(scale: f32, depth: u32)`; `TextureSpec::Noise { scale: f32, depth: u32 }`; `CellTexture::Noise { scale: f32, depth: u32 }`.

- [ ] **Step 1: Write the failing test**

Append to `src/texture/noise_texture.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::Point3;

    #[test]
    fn scale_actually_changes_the_sample() {
        let p = Point3::new(1.3, 0.7, 2.1);
        let a = NoiseTexture::new(1.0, 7).value(0.0, 0.0, &p);
        let b = NoiseTexture::new(10.0, 7).value(0.0, 0.0, &p);
        assert!((a.x - b.x).abs() > 1e-4, "scale had no effect: a={a:?} b={b:?}");
    }

    #[test]
    fn depth_changes_the_sample() {
        let p = Point3::new(1.3, 0.7, 2.1);
        let a = NoiseTexture::new(2.0, 1).value(0.0, 0.0, &p);
        let b = NoiseTexture::new(2.0, 7).value(0.0, 0.0, &p);
        assert!((a.x - b.x).abs() > 1e-4, "depth had no effect: a={a:?} b={b:?}");
    }
}
```

- [ ] **Step 2: Run the tests — verify they fail**

Run: `cargo test noise_texture 2>&1 | tail -20`
Expected: compile error — `NoiseTexture::new` takes one argument.

- [ ] **Step 3: Update `NoiseTexture`**

Replace the struct + impl in `src/texture/noise_texture.rs`:

```rust
pub struct NoiseTexture {
    noise: Perlin,
    scale: f32,
    depth: u32,
}

impl NoiseTexture {
    pub fn new(scale: f32, depth: u32) -> Self {
        NoiseTexture {
            noise: Perlin::new(),
            scale,
            depth,
        }
    }
}

impl Texture for NoiseTexture {
    fn value(&self, _u: f32, _v: f32, p: &Point3) -> Color {
        Color::new(1., 1., 1.) * self.noise.turb(&(self.scale * *p), self.depth)
    }
}
```

- [ ] **Step 4: Update the scene build sites + scene callers**

In `src/scene.rs`, change the enum variants:
- `Noise { scale: f32 }` → `Noise { scale: f32, depth: u32 }` in BOTH `TextureSpec` and `CellTexture`.
- In `CellTexture::build`: `CellTexture::Noise { scale, depth } => Arc::new(NoiseTexture::new(*scale, *depth)),`
- In `TextureSpec::build`: `TextureSpec::Noise { scale, depth } => Arc::new(NoiseTexture::new(*scale, *depth)),`
- `preview_color` arms use `Noise { .. }` already — leave them.
- Update any `TextureSpec::Noise { scale: .. }` / `CellTexture::Noise { scale: .. }` in `#[cfg(test)]` modules to add `depth: 7`.

In `src/scenes/simple_light.rs` and `src/scenes/perlin_spheres.rs`: `NoiseTexture::new(4., 7)`.

- [ ] **Step 5: Update the editor noise rows + creation**

In `src/viewer/controls.rs`:
- Noise creation sites: `TextureSpec::Noise { scale: 4.0 }` → `{ scale: 4.0, depth: 7 }`; `CellTexture::Noise { scale: 4.0 }` → `{ scale: 4.0, depth: 7 }`.
- The `TextureSpec::Noise { scale }` editor arm becomes `TextureSpec::Noise { scale, depth }` with a depth row after scale (depth is `u32`, edited through a temp `f32`):

```rust
TextureSpec::Noise { scale, depth } => {
    changed |= axis_row(ui, "Scale", scale, 0.01, "", Some(3), Some(0.01..=100.0));
    let mut d = *depth as f32;
    if axis_row(ui, "Detail", &mut d, 1.0, "", Some(0), Some(1.0..=10.0)) {
        *depth = d.round().clamp(1.0, 10.0) as u32;
        changed = true;
    }
}
```

(If the `CellTexture::Noise` editor arm also renders a scale row, give it the same `depth` treatment; if it only matches `Noise { .. }`, just fix its construction site.)

- [ ] **Step 6: Run tests + build**

Run: `cargo test noise_texture 2>&1 | tail -10 ; cargo build 2>&1 | tail -3`
Expected: both noise tests PASS; clean build, no new warnings.

- [ ] **Step 7: Commit**

```bash
git add src/texture/noise_texture.rs src/scene.rs src/scenes/simple_light.rs src/scenes/perlin_spheres.rs src/viewer/controls.rs
git commit -m "feat: apply noise scale and expose turbulence depth

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `Projection` + `MappedTexture`

**Files:**
- Create: `src/texture/mapped_texture.rs`
- Modify: `src/texture/mod.rs` (add `pub mod mapped_texture; pub use mapped_texture::*;`)

**Interfaces:**
- Produces: `pub enum Projection { MeshUv, Planar, Spherical, Cylindrical }`; `pub struct MappedTexture` impl `Texture`; `MappedTexture::new(inner: Arc<dyn Texture>, projection: Projection, scale: f32, offset: (f32, f32)) -> Self`.

- [ ] **Step 1: Write the failing test**

Create `src/texture/mapped_texture.rs`:

```rust
use std::sync::Arc;

use crate::color::Color;
use crate::texture::Texture;
use crate::vec3::Point3;

/// How a texture's (u, v) lookup coordinates are derived for a hit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Projection {
    /// Use the geometry's own (u, v).
    MeshUv,
    /// Project the hit point onto the world XZ plane: (p.x, p.z).
    Planar,
    /// Spherical coordinates of the direction of p (about the origin).
    Spherical,
    /// Cylindrical: angle about the world Y axis, and height p.y.
    Cylindrical,
}

/// Wraps a texture, remapping the (u, v) it samples with by `projection`, then a
/// `scale` and `offset`, wrapping the result into [0, 1) so `scale > 1` tiles.
pub struct MappedTexture {
    inner: Arc<dyn Texture>,
    projection: Projection,
    scale: f32,
    offset: (f32, f32),
}

impl MappedTexture {
    pub fn new(
        inner: Arc<dyn Texture>,
        projection: Projection,
        scale: f32,
        offset: (f32, f32),
    ) -> Self {
        MappedTexture {
            inner,
            projection,
            scale,
            offset,
        }
    }
}

impl Texture for MappedTexture {
    fn value(&self, u: f32, v: f32, p: &Point3) -> Color {
        use std::f32::consts::PI;
        let (bu, bv) = match self.projection {
            Projection::MeshUv => (u, v),
            Projection::Planar => (p.x, p.z),
            Projection::Spherical => {
                let d = p.unit();
                (
                    f32::atan2(d.z, d.x) / (2.0 * PI) + 0.5,
                    (d.y.clamp(-1.0, 1.0)).acos() / PI,
                )
            }
            Projection::Cylindrical => (f32::atan2(p.z, p.x) / (2.0 * PI) + 0.5, p.y),
        };
        let uu = (bu * self.scale + self.offset.0).rem_euclid(1.0);
        let vv = (bv * self.scale + self.offset.1).rem_euclid(1.0);
        self.inner.value(uu, vv, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::Point3;

    /// Test texture that echoes its (u, v) back as a color, so we can assert
    /// exactly which coordinates `MappedTexture` produced.
    struct UvProbe;
    impl Texture for UvProbe {
        fn value(&self, u: f32, v: f32, _p: &Point3) -> Color {
            Color::new(u, v, 0.0)
        }
    }

    fn probe(projection: Projection, scale: f32, offset: (f32, f32)) -> MappedTexture {
        MappedTexture::new(Arc::new(UvProbe), projection, scale, offset)
    }

    #[test]
    fn mesh_uv_identity_passes_uv_through() {
        let c = probe(Projection::MeshUv, 1.0, (0.0, 0.0)).value(0.3, 0.7, &Point3::new(9.0, 9.0, 9.0));
        assert!((c.x - 0.3).abs() < 1e-5 && (c.y - 0.7).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn planar_uses_xz_of_point() {
        // p.x=0.2, p.z=0.4 → uv (0.2, 0.4); incoming uv ignored.
        let c = probe(Projection::Planar, 1.0, (0.0, 0.0)).value(0.9, 0.9, &Point3::new(0.2, 5.0, 0.4));
        assert!((c.x - 0.2).abs() < 1e-5 && (c.y - 0.4).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn scale_and_offset_tile_with_wrap() {
        // MeshUv, scale 2, offset 0: u 0.6 → 1.2 → wrap → 0.2.
        let c = probe(Projection::MeshUv, 2.0, (0.0, 0.0)).value(0.6, 0.1, &Point3::new(0.0, 0.0, 0.0));
        assert!((c.x - 0.2).abs() < 1e-5, "expected wrapped 0.2, got {c:?}");
    }
}
```

- [ ] **Step 2: Run the tests — verify they fail**

Run: `cargo test mapped_texture 2>&1 | tail -15`
Expected: compile error — module not declared.

- [ ] **Step 3: Register the module**

In `src/texture/mod.rs`, add (keeping the existing ordering pattern):

```rust
pub mod mapped_texture;
```
and in the `pub use` block:
```rust
pub use mapped_texture::*;
```

- [ ] **Step 4: Run the tests — verify they pass**

Run: `cargo test mapped_texture 2>&1 | tail -15`
Expected: `mesh_uv_identity_passes_uv_through`, `planar_uses_xz_of_point`, `scale_and_offset_tile_with_wrap` PASS.

- [ ] **Step 5: Commit**

```bash
git add src/texture/mapped_texture.rs src/texture/mod.rs
git commit -m "feat: MappedTexture with Projection (mesh-uv/planar/spherical/cylindrical)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `Mapping` on `TextureSpec::Image` + editor UI

**Files:**
- Modify: `src/scene.rs` (`Mapping` struct; `Image { asset, mapping }`; build wraps; construction/test sites)
- Modify: `src/viewer/controls.rs` (Image arm: projection dropdown + scale + offset; Image creation site)

**Interfaces:**
- Consumes: `Projection`, `MappedTexture` (Task 2).
- Produces: `pub struct Mapping { pub projection: Projection, pub scale: f32, pub offset: (f32, f32) }` with `Default`/`is_identity`; `TextureSpec::Image { asset: Asset, mapping: Mapping }`.

- [ ] **Step 1: Write the failing test**

Append to `src/scene.rs` (in or after the existing texture test module):

```rust
#[cfg(test)]
mod mapping_tests {
    use super::*;

    #[test]
    fn default_mapping_is_identity() {
        let m = Mapping::default();
        assert!(m.is_identity());
        assert_eq!(m.projection, crate::texture::Projection::MeshUv);
        assert_eq!(m.scale, 1.0);
        assert_eq!(m.offset, (0.0, 0.0));
    }

    #[test]
    fn non_identity_when_changed() {
        let m = Mapping { projection: crate::texture::Projection::Planar, scale: 1.0, offset: (0.0, 0.0) };
        assert!(!m.is_identity());
        let m2 = Mapping { scale: 2.0, ..Mapping::default() };
        assert!(!m2.is_identity());
    }
}
```

- [ ] **Step 2: Run the test — verify it fails**

Run: `cargo test mapping_tests 2>&1 | tail -15`
Expected: compile error — `Mapping` does not exist.

- [ ] **Step 3: Add `Mapping` and migrate the `Image` variant**

In `src/scene.rs`, import `Projection` and `MappedTexture` from the texture module (extend the existing `use crate::texture::{...}` line). Add:

```rust
#[derive(Clone, Copy, PartialEq)]
pub struct Mapping {
    pub projection: crate::texture::Projection,
    pub scale: f32,
    pub offset: (f32, f32),
}

impl Default for Mapping {
    fn default() -> Self {
        Mapping {
            projection: crate::texture::Projection::MeshUv,
            scale: 1.0,
            offset: (0.0, 0.0),
        }
    }
}

impl Mapping {
    fn is_identity(&self) -> bool {
        self.projection == crate::texture::Projection::MeshUv
            && self.scale == 1.0
            && self.offset == (0.0, 0.0)
    }
}
```

Change the variant: `TextureSpec::Image { asset: Asset }` → `Image { asset: Asset, mapping: Mapping }`.

In `TextureSpec::build`, replace the `Image` arm:

```rust
TextureSpec::Image { asset, mapping } => {
    let inner = build_image(asset);
    if mapping.is_identity() {
        inner
    } else {
        Arc::new(crate::texture::MappedTexture::new(
            inner,
            mapping.projection,
            mapping.scale,
            mapping.offset,
        ))
    }
}
```

`preview_color`'s `Image { .. }` arm already uses `..` — leave it. Update every other `TextureSpec::Image { asset: ... }` construction (e.g. in `#[cfg(test)]` modules and any scene) to `Image { asset: ..., mapping: Mapping::default() }`.

- [ ] **Step 4: Update the editor Image arm + creation**

In `src/viewer/controls.rs`:
- Image creation site: `TextureSpec::Image { asset: Asset::empty() }` → `Image { asset: Asset::empty(), mapping: Mapping::default() }` (import `Mapping`).
- Replace the `TextureSpec::Image { asset } => changed |= image_picker_row(ui, asset),` arm with one that also edits the mapping:

```rust
TextureSpec::Image { asset, mapping } => {
    changed |= image_picker_row(ui, asset);
    use crate::texture::Projection;
    let proj_label = match mapping.projection {
        Projection::MeshUv => "Mesh UV",
        Projection::Planar => "Planar",
        Projection::Spherical => "Spherical",
        Projection::Cylindrical => "Cylindrical",
    };
    labeled_row(ui, "Projection", |ui| {
        egui::ComboBox::from_id_salt("texture_projection")
            .selected_text(proj_label)
            .show_ui(ui, |ui| {
                for (p, label) in [
                    (Projection::MeshUv, "Mesh UV"),
                    (Projection::Planar, "Planar"),
                    (Projection::Spherical, "Spherical"),
                    (Projection::Cylindrical, "Cylindrical"),
                ] {
                    if ui.selectable_label(mapping.projection == p, label).clicked() {
                        mapping.projection = p;
                        changed = true;
                    }
                }
            });
    });
    changed |= axis_row(ui, "Tile", &mut mapping.scale, 0.01, "", Some(3), Some(0.01..=100.0));
    changed |= axis_row(ui, "Offset U", &mut mapping.offset.0, 0.01, "", Some(3), Some(-10.0..=10.0));
    changed |= axis_row(ui, "Offset V", &mut mapping.offset.1, 0.01, "", Some(3), Some(-10.0..=10.0));
}
```

(`labeled_row` is the existing helper used by the type combos — if its name/signature differs, match the existing dropdown pattern in `texture_controls`. If `axis_row` cannot bind `mapping.offset.0` directly, copy into a local `f32`, row it, write back on change.)

- [ ] **Step 5: Run tests + build**

Run: `cargo test mapping_tests 2>&1 | tail -10 ; cargo test 2>&1 | grep -E "test result:|error" ; cargo build 2>&1 | tail -3`
Expected: `mapping_tests` pass; full suite passes; clean build, no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/scene.rs src/viewer/controls.rs
git commit -m "feat: UV projection + tiling/offset mapping for image textures

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** noise scale applied + depth exposed (Task 1: NoiseTexture, specs, callers, editor); `Projection` + `MappedTexture` with XZ planar, spherical, cylindrical, mesh-uv and rem_euclid wrap (Task 2); `Mapping` on `Image`, build-time wrap, editor projection/scale/offset (Task 3). Deferred items (triplanar, mapping on non-image, rotation) untouched. All spec points covered.
- **Placeholder scan:** none — full code given; the two parenthetical "if the helper differs, match the existing pattern" notes are fallback guidance for the editor's pre-existing helpers, not unfinished steps.
- **Type consistency:** `NoiseTexture::new(f32, u32)` used at all four sites; `Noise { scale, depth }` consistent across `TextureSpec`/`CellTexture`/build/editor; `Projection`/`MappedTexture::new(inner, projection, scale, offset)` defined in Task 2 and called identically in Task 3's `build`; `Mapping { projection, scale, offset }` consistent between definition and `Image` use.
