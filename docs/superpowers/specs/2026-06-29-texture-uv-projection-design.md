# Texture UV Mapping / Projection + Noise Settings — Design

**Date:** 2026-06-29
**Status:** Approved (autonomous — implementer decides specifics)
**Branch:** rasterized-edit-view (post-merge of editable-textures; `TextureSpec` present)

## Context

After merging the editable-textures work, `TextureSpec { Solid, Checker, Noise, Image }`
exists with `build() -> Arc<dyn Texture>`, `preview_color()`, and a material-editor
UI. Two gaps: (1) the noise texture's `scale` is ignored and its turbulence depth
is hardcoded; (2) image textures have no UV mapping/projection control. This adds
both.

## Part 1 — Noise settings (fix + expose)

`NoiseTexture::value` currently computes `self.scale * p` but then calls
`turb(p, 7)` on the **unscaled** `p` with a hardcoded depth. Fix and expose:

- `NoiseTexture` gains `depth: u32`; `new(scale: f32, depth: u32)`. `value` becomes
  `Color::ones() * self.noise.turb(&(self.scale * *p), self.depth)`.
- `TextureSpec::Noise { scale, depth: u32 }` and `CellTexture::Noise { scale, depth: u32 }`.
  `build()` passes both. Editor noise rows: scale **and** depth (depth as an integer
  slider, clamped e.g. 1..=10).
- Update the four `NoiseTexture::new` call sites: `scene.rs` (×2),
  `scenes/simple_light.rs`, `scenes/perlin_spheres.rs` (use depth `7`, today's value).

## Part 2 — UV mapping / projection (image textures)

A runtime wrapper remaps the `(u, v)` fed to a texture, plus plain-data on the spec.

### `texture/mapped_texture.rs` (new)

```rust
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Projection { MeshUv, Planar, Spherical, Cylindrical }

pub struct MappedTexture {
    inner: Arc<dyn Texture>,
    projection: Projection,
    scale: f32,
    offset: (f32, f32),
}
```

`value(u, v, p)`:
1. Base coords by projection:
   - `MeshUv`: `(u, v)` (the geometry's own uv).
   - `Planar`: `(p.x, p.z)` (XZ plane).
   - `Spherical`: from `d = p.unit()` — `(atan2(d.z, d.x)/(2π) + 0.5, acos(d.y.clamp(-1,1))/π)`.
   - `Cylindrical`: `(atan2(p.z, p.x)/(2π) + 0.5, p.y)`.
2. Apply scale/offset then **wrap** into [0,1) (so `scale > 1` tiles — `ImageTexture`
   clamps uv, so the wrap must happen here): `uu = (base_u * scale + offset.0).rem_euclid(1.0)`,
   likewise `vv`.
3. Return `inner.value(uu, vv, p)` (original `p` passed through unchanged).

### Spec side (`scene.rs`)

```rust
#[derive(Clone, Copy, PartialEq)]
pub struct Mapping { pub projection: Projection, pub scale: f32, pub offset: (f32, f32) }
impl Default for Mapping { /* MeshUv, 1.0, (0.0, 0.0) */ }
impl Mapping { fn is_identity(&self) -> bool { /* MeshUv && scale==1 && offset==(0,0) */ } }
```

`TextureSpec::Image { asset }` → `Image { asset, mapping: Mapping }`. `build()` for
`Image`: build the inner image texture; if `mapping.is_identity()` return it as-is,
else wrap in `MappedTexture`. `preview_color()` for `Image` is unchanged (neutral
gray). `Projection` is imported from the texture module (no circular dep:
`MappedTexture` takes primitives, `scene.rs`'s `build` passes
`mapping.projection/scale/offset`).

### Editor (`viewer/controls.rs`)

When the texture type is `Image`, show a **Projection** dropdown (MeshUv / Planar /
Spherical / Cylindrical) and **scale** + **offset (u, v)** rows, editing the
`Image` variant's `Mapping`. Other texture types are unchanged.

## Scope decisions (honest)

- **Mapping is on `Image` only.** `Solid` is constant; `Noise`/`Checker` sample by
  world `p`, so uv remapping is a no-op for them — their frequency is their own
  `scale` (Part 1 / existing). `CellTexture::Image` (checker cells) keeps default
  mesh-uv (no mapping UI) — out of scope.
- **No `Texture` trait change.** All projections derive from `(u, v, p)`; triplanar
  (needs the surface normal) stays deferred.
- **Planar uses the fixed XZ plane** (not dominant-axis) — simple and predictable;
  dominant-axis/triplanar is the deferred upgrade.

## Testing

- `NoiseTexture`: `value` differs for `scale=1` vs `scale=10` at the same `p`
  (scale now actually applied); differs for `depth=1` vs `depth=7`.
- `MappedTexture` (via a test-only `UvProbe` texture returning `Color::new(u, v, 0)`):
  - `MeshUv`, identity mapping → returns the incoming `(u, v)`.
  - `Planar` → returns `(p.x, p.z)` (wrapped) for `scale=1, offset=0`.
  - `scale`/`offset` tiling: e.g. `scale=2` on uv `0.6` → `0.2` (wrapped).
- `Mapping::is_identity` true for default, false when projection/scale/offset differ.
- `TextureSpec::Image { mapping }` `build()`: identity mapping returns the bare
  image texture; non-identity returns a `MappedTexture` (assert behavior via a
  sampled value rather than the concrete type).
- Editor: compiles; data-model round-trips (covered by build tests). GUI verified
  manually.

## Out of scope (YAGNI)

Triplanar / dominant-axis planar; mapping on Solid/Noise/Checker/cell textures;
per-axis (3D) texture transforms; rotation of uv (only scale + offset this pass).
