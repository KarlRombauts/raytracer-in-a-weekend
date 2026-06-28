# Light PDF Weighting (Unbiased Direct Lighting) — Design

**Date:** 2026-06-29
**Status:** Approved (pending spec review)

## Context

The previous step ([direct light sampling](2026-06-29-direct-light-sampling-design.md))
added shadow-ray direct lighting but deliberately skipped the PDF / geometry
term, so the image is biased: `direct = albedo * emit * cos_surface`. This step
makes the direct lighting **unbiased** for planar lights by dividing each light's
contribution by the solid-angle PDF of the light sample, adding the Lambertian
`1/π` BRDF normalization, and folding the geometry term (`cos_light / dist²`,
light area) into the PDF.

We adopt the *Ray Tracing: The Rest of Your Life* `pdf_value` / `random`
interface (chosen over extending `sample_point` with a normal + `area()`),
because it keeps the integrator trivial and is the seam the later MIS / mixture-
PDF step plugs into.

## The math

Sampling a point uniformly on a light's area `A` has area-measure PDF `1/A`. The
lighting integral is over solid angle, so converting introduces the geometry
term. The correct per-light, per-sample contribution for a Lambertian surface is:

```
direct = (albedo / π) · emit · cos_surface · cos_light · A / dist²
```

Expressed via a solid-angle PDF `pdf = dist² / (cos_light · A)`, this is simply:

```
direct = (albedo / π) · emit · cos_surface / pdf
```

so the geometry term (`cos_light`, `A`, `dist²`) lives entirely inside `pdf` and
the integrator only divides by it. Because we **sum over all lights** (splitting
the integral), there is no light-selection PDF — each light divides by its own
area PDF.

## Interface

Three methods added to the `Intersect` trait (`src/ray/intersect.rs`):

```rust
/// Surface area, for area-light PDFs. Default 0 (not a sampleable light).
fn area(&self) -> f32 { 0.0 }

/// Solid-angle PDF of sampling `dir` (from `origin`) toward this object.
/// Generic default: intersect self along `dir`, convert the `1/area` area PDF
/// to solid angle via the hit distance and the light's facing cosine. Correct
/// for any convex single surface that reports a real `area()`.
fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32 {
    let ray = Ray::new(origin, dir);
    match self.intersect(&ray, &Interval::new(0.001, f32::INFINITY)) {
        None => 0.0,
        Some(hit) => {
            let dist2 = hit.t * hit.t * dir.length_squared();
            let cos = (dir.dot(&hit.normal) / dir.length()).abs(); // two-sided light
            let a = self.area();
            if cos < 1e-8 || a <= 0.0 { 0.0 } else { dist2 / (cos * a) }
        }
    }
}

/// A random (unnormalized) direction from `origin` toward a point on this
/// object. Default reuses the existing `sample_point`, so it composes through
/// groups/BVH/transforms exactly as `sample_point` does.
fn random_dir(&self, origin: Point3, rng: &mut SmallRng) -> Vec3 {
    self.sample_point(rng) - origin
}
```

Per-shape overrides this step: **`area()` for `Quad` and `Triangle` only.**
- `Quad::area` = `self.u.cross(&self.v).length()`.
- `Triangle::area` = `0.5 * self.u.cross(&self.v).length()`.

The `area()` default of 0 means every other shape returns `pdf_value == 0`, so
the integrator skips it (see scope).

## Integrator change

In `Camera::ray_color_direct` (`src/camera/camera.rs`), replace the per-light body
with the PDF-weighted form. `dir` is unnormalized (points exactly at the sampled
light point at parameter `t = 1`):

```rust
for light in &world.lights {
    let dir = light.geom.random_dir(hit.p, rng);
    let pdf = light.geom.pdf_value(hit.p, dir);
    if pdf <= 0.0 {
        continue;
    }
    let unit = dir.unit();
    let cos_surface = hit.normal.dot(&unit);
    if cos_surface <= 0.0 {
        continue; // light behind the surface
    }
    let shadow = Ray::new_t(hit.p, dir, ray.time);
    // The sampled light point is at t = 1 along `dir`; check for blockers strictly
    // before it.
    if world.intersect(&shadow, &Interval::new(0.001, 1.0 - 1e-4)).is_none() {
        direct += (albedo / std::f32::consts::PI) * light.emit * cos_surface / pdf;
    }
}
```

`albedo` is still `scatter`'s returned attenuation; dividing by π gives the
correct Lambertian BRDF. The `emitted + direct` structure and the
miss→background / scatter-None→emitted handling are unchanged. Still direct-only
(no indirect bounce), so no double counting.

## Light registration (`build_world`)

`build_world` (`src/scene.rs`) currently registers **every** `DiffuseLight`
object in `world.lights`. Change it to register a light **only when its built
geometry reports `area() > 0`** — i.e. only the shapes we can correctly
importance-sample (this step: `Quad`/`Triangle`). The object is still added to
the world geometry unconditionally, so a non-registered emitter (mesh, sphere)
**still glows when a ray hits it directly** (its `emitted()` is returned); it
simply isn't NEE-sampled. This keeps `world.lights` honest ("things we actually
sample"), avoids a wasted `pdf_value` intersect per unsamplable emitter per
sample, and degrades gracefully: once the indirect bounce returns, those emitters
illuminate the scene via GI (just noisier, un-importance-sampled).

```rust
let geom = obj.build();
world.add(geom.clone());
if let MaterialSpec::DiffuseLight { emit } = &obj.material {
    if geom.area() > 0.0 {
        world.lights.push(Light { geom, emit: *emit });
    }
}
```

## Scope

**Correct this step:** planar single-primitive lights — `Quad` and `Triangle`.
This covers the Cornell box, whose light is a single untransformed quad. After
this change the Cornell box's direct lighting is physically correct (no longer
over/under-exposed).

**Deferred** (each keeps the `area() = 0` default, so it is **not registered** as
an NEE light by `build_world` — it still emits/glows, it just isn't shadow-ray
sampled; not silently wrong):
- **Sphere lights:** uniform surface sampling is inconsistent with the
  nearest-hit `pdf_value` (a back-side sample's PDF would not match its hit), so
  correct spheres need cone sampling toward the visible cap. Separate step.
- **Composite lights** (box-of-quads via `IntersectGroup`, triangle-mesh via
  `BVH`): need area-weighted child selection (e.g. a per-triangle area CDF /
  alias table) so the total-area PDF is consistent with the generic `pdf_value`.
  Separate step.
- **Transformed lights** (`Translate`/`Scale`/`Rotate` wrapping a light): need an
  `area()` that accounts for the transform (`Scale` changes area). Separate step.

## Testing

- `Quad::area` and `Triangle::area` return the analytic area for known inputs.
- `Quad::pdf_value` returns the analytic value `dist² / (cos · area)` for a
  hand-computed origin/direction (deterministic — no rng). E.g. a point directly
  below an axis-aligned quad light, `dir` straight up: `pdf == dist² / area`.
- `pdf_value` returns 0 for a direction that misses the light.
- Integrator inverse-square behavior: for a fixed simple configuration, averaging
  many samples, moving the light from distance `d` to `2d` reduces the direct
  term by ≈ 4× (within a tolerance), confirming the geometry term is applied.
- `build_world` registration guard: a scene with a quad emitter and a sphere
  emitter registers only the quad in `world.lights` (`len == 1`), while both
  objects remain in the world geometry (the sphere still glows when hit).

## Out of scope (YAGNI)

- The indirect bounce / GI and reunifying with the path tracer (a later step;
  needs a proper `Material` reflectance method instead of the `scatter`-derived
  albedo, and double-count handling / MIS).
- MIS combining BRDF and light sampling, and the mixture PDF.
- Sphere cone sampling, composite area-weighting, transformed-light area.
- Light-selection PDF (only needed if we switch from summing all lights to
  sampling one).
