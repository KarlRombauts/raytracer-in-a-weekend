# Direct Light Sampling (Shadow Rays, No PDF) — Design

**Date:** 2026-06-29
**Status:** Approved (pending spec review)

## Context

This is the first step toward Next Event Estimation (NEE) / importance sampling
("Ray Tracing: The Rest of Your Life"). The goal here is deliberately narrow:
get the **shadow-ray machinery** working — find the lights, sample a random point
on a light's surface, shoot a ray at it, and test occlusion — without yet adding
the PDF / geometry weighting that makes the result physically correct.

Two later steps (out of scope here) complete the picture:
- **Step 2:** reintroduce the indirect bounce (global illumination) on top of the
  direct term, with double-count avoidance.
- **Step 3:** add the light PDF + geometry term (and area-weighted light
  selection, visible-hemisphere sphere sampling), making the estimate unbiased,
  and MIS to combine BRDF and light sampling.

## Scope for this step ("Option B")

**Direct lighting only.** A diffuse hit shoots shadow rays at the lights, shades
if visible, and stops. No indirect bounce, no GI, no PDF. Soft shadows emerge
naturally because each sample aims at a different random point on the light.

The result is **intentionally biased**: brightness is not physically correct, and
bigger/closer/more-tilted lights are not properly accounted for (no geometry/PDF
term). That is the lesson this step teaches; step 3 fixes it.

## Approach

Make geometry sampleable by extending the existing `Intersect` trait, rather than
introducing a parallel light-geometry hierarchy. This composes with the existing
type-erased `Arc<dyn Intersect>` world and the `Translate`/`Scale`/`Rotate`
wrappers for free, and it is the same seam book 3 later extends with `pdf_value`.

### Components

**1. `Intersect::sample_point(&self, rng: &mut SmallRng) -> Point3`**

New trait method. Default implementation returns `self.center()` (acceptable for
any geometry we do not specifically sample). Overrides:

- `Quad`: `q + a*u + b*v`, with `a, b` uniform in [0, 1).
- `Triangle`: uniform barycentric — with `r1, r2` uniform in [0, 1),
  `let su = r1.sqrt();` point = `p1*(1 - su) + p2*(su*(1 - r2)) + p3*(su*r2)`.
  (Triangle stores `q`, `u`, `v`; the vertices are `q`, `q+u`, `q+v`.)
- `Sphere`: `center + radius * Vec3::random_unit(rng)` (uniform over the whole
  surface).
- `IntersectGroup` and `BVH` (the "collection of planes/triangles" case): pick a
  uniformly random child and recurse into its `sample_point`. **Uniform among
  children, not area-weighted** — the resulting bias is acceptable because this
  step has no PDF anyway; area-weighting arrives with the PDF in step 3.
- Transform wrappers (`Translate`/`Scale`/`Rotate`): sample the inner object's
  point and map it through the same transform applied to the geometry.

**2. `Light { geom: Arc<dyn Intersect>, emit: Color }`**

A light pairs a sampleable geometry handle with a constant emission color.
Constant emission is sufficient for this step (textured-light emission is a later
refinement).

**3. Lights live on the world.**

Add `pub lights: Vec<Light>` to `IntersectGroup`, populated only by `build_world`
(by scanning `ObjectSpec`s whose `MaterialSpec` is `DiffuseLight`, building the
geometry, and pairing it with its `emit` color). Box sub-groups and other interior
`IntersectGroup`s simply leave the field empty. This keeps `ray_color`'s signature
unchanged — it already holds the world — avoiding new parameters through
`add_pass` / `sample_pixel` / `ray_color`.

**4. New `ray_color_direct` on `Camera`.**

The existing path-tracing `ray_color` is left intact so step 2 can merge the two.
`sample_pixel` switches to call `ray_color_direct`:

```
hit = world.intersect(ray, (0.001, INF))     // miss -> return background
emitted = hit.material.emitted(u, v, p)
match hit.material.scatter(ray, hit, rng):
    None             => return emitted        // the light itself (or absorber)
    Some((_, albedo)) =>
        direct = Color::ZERO
        for light in &world.lights:
            lp   = light.geom.sample_point(rng)        // random point on the light
            to   = lp - hit.p
            dist = to.length(); dir = to / dist
            cos  = hit.normal.dot(dir)
            if cos <= 0.0 { continue }                 // light behind the surface
            shadow = Ray from hit.p along dir
            if world.intersect(shadow, (0.001, dist - 0.001)).is_none() {
                direct += albedo * light.emit * cos    // no PDF / geometry term
            }
        return emitted + direct
```

The surface reflectance (`albedo`) is taken from `scatter`'s returned attenuation
to avoid adding a new `Material` method this step; this is adequate because the
scene in scope is diffuse. The discarded scattered ray is a minor, accepted
inefficiency.

### Front-facing light points

We do **not** restrict sampling to the side of the light facing the shaded point.
For flat lights (quad, triangle) there is no front/back point distinction. For
solid lights (sphere), a back-side sample is automatically rejected because the
shadow ray toward it is occluded by the light body itself (`world.intersect` finds
the near surface at `t < dist`) — so the image stays correct. The only cost is
wasted samples (~half, for a sphere) → extra noise. Visible-hemisphere sampling is
a variance optimization that requires a matching PDF, so it is deferred to step 3.

## Testing

- `sample_point` lands on the surface for each shape: quad point is coplanar and
  within bounds; sphere point is at `radius` from center; triangle point is inside
  the triangle (valid barycentric coordinates).
- `build_world` on `cornell_box` collects exactly one light with emit
  `(15.0, 15.0, 15.0)`.
- Occlusion: with a small hand-built world, a blocker between a point and the
  light yields an occluded shadow ray; a clear path yields an unoccluded one.

## Out of scope (YAGNI)

- The indirect bounce / global illumination (step 2).
- The light PDF, geometry term, area-weighted child selection, and
  visible-hemisphere sphere sampling (step 3).
- MIS combining BRDF and light sampling (step 3).
- Textured / non-constant light emission.
