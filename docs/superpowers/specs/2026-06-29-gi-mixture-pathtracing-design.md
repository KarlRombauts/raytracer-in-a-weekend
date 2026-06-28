# GI Blending via Mixture-PDF Path Tracing — Design

**Date:** 2026-06-29
**Status:** Approved (pending spec review)

## Context

We have two separate integrators on `Camera`:
- `ray_color` — the original pure-BRDF path tracer (indirect/GI, emission at every
  hit, no light sampling).
- `ray_color_direct` — shadow-ray NEE direct lighting only (no indirect bounce),
  using the `pdf_value`/`random_dir` light-sampling interface.

This step blends them into one **mixture-PDF path tracer** (the "Ray Tracing: The
Rest of Your Life" approach): full global illumination *and* light importance
sampling, in a single integrator, reusing the `pdf_value`/`random_dir` machinery.

## Approach: one ray per bounce, sampled from a mixture

The defining property: we trace **exactly one ray per bounce**. At a diffuse hit
its direction is drawn from a *mixture* of "toward the lights" and
"cosine/BRDF". Because only one ray leaves each bounce, emission is simply added
wherever a ray lands — there is **no double counting and no suppression
bookkeeping**. The "shadow-ray optimization" becomes "importance-sample the
bounce toward the lights": the same variance reduction, folded into the path
rather than a separate shadow ray.

This handles non-registered emitters correctly: a mesh/sphere emitter has
`light_pdf = 0`, so it is found via the cosine term and weighted so the estimator
stays unbiased (just noisier than a registered light, which additionally gets the
light-importance term). Meshes illuminate via GI; registered lights also get the
optimization.

## The estimator

At a diffuse (Lambertian) hit, sample a direction `dir`, then update throughput:

```
throughput *= albedo * scattering_pdf(dir) / p_mixture(dir)
p_mixture(dir) = 0.5 * cosine_pdf(dir) + 0.5 * light_pdf(dir)
scattering_pdf(dir) = cosine_pdf(dir) = max(0, cos_theta) / PI    // cos_theta = n·dir_unit
```

With **no lights**, `p_mixture = cosine_pdf` and the weight collapses to `albedo`
— i.e. it reduces exactly to the current pure-BRDF path tracer. Emission is added
(throughput-weighted) at every hit, including the camera ray's first hit.

## Components

### 1. Material interface additions (`src/material/material.rs` + impls)

Two methods with defaults:

```rust
/// True for delta/near-delta BRDFs (mirror, glass, coat) that should be traced
/// with their own scattered ray rather than mixture-sampled. Default false.
fn is_specular(&self) -> bool { false }

/// PDF (solid angle) that this BRDF scatters `dir` at `hit`. Default 0.
/// Lambertian: max(0, n·dir_unit) / PI.
fn scattering_pdf(&self, hit: &HitRecord, dir: &Vec3) -> f32 { 0.0 }
```

Overrides:
- `Lambertian`: `scattering_pdf` = `max(0.0, hit.normal.dot(&dir.unit())) / PI`.
- `Metal`, `Dielectric`, `Glossy`: `is_specular` → `true`.

Only Lambertian (`is_specular() == false`) uses the mixture; specular materials
keep current behavior (their `scatter()` ray + attenuation, no mixture), which is
correct GI — mirrors/glass do not benefit from light sampling. `scatter()` and
`emitted()` are unchanged.

### 2. Light-mixture helpers on `IntersectGroup` (`src/group.rs`)

```rust
/// Average solid-angle PDF of sampling `dir` toward any registered light.
/// 0 if there are no lights.
fn light_pdf(&self, origin: Point3, dir: Vec3) -> f32;
// = if lights empty { 0.0 } else { sum(light.geom.pdf_value(origin, dir)) / lights.len() }

/// A random direction from `origin` toward a uniformly chosen registered light.
/// None if there are no lights.
fn sample_light_dir(&self, origin: Point3, rng: &mut SmallRng) -> Option<Vec3>;
// = pick random light index, return light.geom.random_dir(origin, rng)
```

### 3. Cosine direction sampler

A small helper, `cosine_direction(normal, rng) -> Vec3` = `(normal + Vec3::random_unit(rng)).unit()`
(the existing Lambertian trick; PDF `cos/PI`), with the existing near-zero guard
(fall back to `normal` if the sum is near zero). Lives next to the integrator.

### 4. The integrator (single `ray_color`, replacing both old ones)

Iterative, evolving from the current loop:

```
let mut color = ZERO;
let mut throughput = ONES;
let mut current = ray.clone();
for _ in 0..self.max_depth {
    let Some(hit) = world.intersect(&current, &(0.001, INF)) else {
        color += throughput * self.background; break;
    };
    color += throughput * hit.material.emitted(hit.u, hit.v, hit.p);

    let Some((scattered, atten)) = hit.material.scatter(&current, &hit, rng) else {
        break; // pure light / absorber
    };

    if hit.material.is_specular() {
        throughput = throughput * atten;
        current = scattered;
        continue;
    }

    // Lambertian: mixture sampling.
    let albedo = atten;
    let dir = match world.sample_light_dir(hit.p, rng) {
        Some(ld) if rng.random::<f32>() < 0.5 => ld,
        _ => cosine_direction(hit.normal, rng),
    };
    let s = hit.material.scattering_pdf(&hit, &dir);
    if s <= 0.0 { break; } // direction below the surface contributes nothing
    let p = if world.lights.is_empty() {
        s
    } else {
        0.5 * s + 0.5 * world.light_pdf(hit.p, dir)
    };
    if p <= 0.0 { break; }
    throughput = throughput * albedo * (s / p);
    current = Ray::new_t(hit.p, dir, current.time);
}
color
```

Notes:
- The mixture coin is only flipped when a light direction is available; with no
  lights it always cosine-samples and the weight is `albedo` (pure path tracer).
- Occlusion is handled by tracing the single ray — no shadow ray. A light-sampled
  direction that hits an occluder simply continues the path from the occluder.
- `dir` is **unit** when cosine-sampled but **unnormalized** when light-sampled
  (`random_dir` returns `sample_point - origin`). This is intentional and safe:
  `scattering_pdf` and `pdf_value` are solid-angle PDFs that normalize internally
  (`dir.unit()` / `dir.length()`), and ray–object intersection is invariant to
  direction magnitude. Do not "normalize `dir` to fix it" — the PDFs are
  magnitude-invariant by construction.

### 5. Wiring / removals

The mixture integrator becomes the single `ray_color(&self, ray, world, rng) ->
Color` (using `self.max_depth` internally, like `ray_color_direct` did). Both
`sample_pixel` (viewer) and the offline `render()` loop call it. The old
pure-BRDF `ray_color` (which took an explicit `depth` param) and
`ray_color_direct` are removed — the mixture integrator subsumes both. Update
`render()`'s call site accordingly (it currently passes `self.max_depth`).

## Scope / out of scope (YAGNI)

**In scope:** Lambertian mixture light sampling + full GI; specular materials via
BRDF-only bounce; non-registered emitters (mesh/sphere) illuminating via GI
(unbiased); collapsing to one integrator.

**Out of scope:**
- Russian roulette — fixed `max_depth` stays.
- Per-light MIS power/balance-heuristic weights — the 0.5/0.5 mixture is the
  one-sample MIS form, sufficient here.
- Explicit shadow rays — replaced by mixture importance sampling.
- Sphere/composite *light-importance* sampling — still deferred (their
  `area() = 0` ⇒ `light_pdf = 0` ⇒ they fall back to cosine/GI, which is correct).
- A dedicated `Material` reflectance method — the Lambertian albedo continues to
  come from `scatter()`'s attenuation (correct for diffuse).

## Testing

- `Lambertian::scattering_pdf` returns `cos/PI` for a known normal/dir and `0` for
  a direction below the surface.
- `is_specular` is `false` for `Lambertian`, `true` for `Metal`/`Dielectric`/`Glossy`.
- `IntersectGroup::light_pdf` returns the average of the per-light `pdf_value`
  (verify against a single-quad-light setup with a known analytic `pdf_value`),
  and `0.0` when there are no lights. `sample_light_dir` returns `None` with no
  lights and a direction toward the light otherwise.
- Integrator, no lights: with a world containing no registered lights, the
  mixture weight reduces to `albedo` — assert the integrator's result on a simple
  scene matches the pure-BRDF result (e.g. an emissive sphere lighting a diffuse
  surface, averaged over many samples, is finite and non-zero).
- Integrator, GI: in a Cornell-like diffuse scene, a point on the floor in shadow
  of the light still receives non-zero color via bounced (indirect) light —
  something the direct-only integrator returned black for.
