# Power-Heuristic MIS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the one-sample mixture integrator with a power-heuristic multiple-importance-sampling path tracer (explicit NEE shadow ray + BRDF bounce), reducing firefly variance without bias.

**Architecture:** Add a `power_heuristic(a,b)=a²/(a²+b²)` helper, then rewrite `Camera::ray_color` so each Lambertian hit evaluates two MIS estimators — a shadow-ray light sample weighted by `power(p_light,p_brdf)` and a cosine BRDF bounce whose downstream emission is weighted by `power(p_brdf,p_light)`. Specular hits stay BRDF-only with full-weight emission via a `specular_bounce` flag.

**Tech Stack:** Rust, existing `Camera`/`IntersectGroup`/materials, `rand::rngs::SmallRng`.

## Global Constraints

- `power_heuristic(a, b) = a*a / (a*a + b*b)`, returning `0.0` when both are 0 (no NaN).
- Lambertian BRDF·cos = `albedo * scattering_pdf` (since `scattering_pdf = cos/π`).
- NEE contribution: `throughput * power_heuristic(p_light, p_brdf) * albedo * (p_brdf / p_light) * Le`, where `Le` is the `emitted()` of the nearest hit along the shadow ray (0 ⇒ shadowed / non-emitter). Only when `p_light > 0 && p_brdf > 0`.
- BRDF bounce: cosine direction; `throughput *= albedo`; record `prev_brdf_pdf` and set `specular_bounce = false`.
- Emission accounting at each hit: if `specular_bounce` (camera ray or after a specular bounce) add `throughput * emitted` in full; else add `throughput * emitted * power_heuristic(prev_brdf_pdf, light_pdf(current.origin, current.direction))`.
- Specular materials: `throughput *= atten`, continue, `specular_bounce = true`; no NEE.
- `ld` from `sample_light_dir` is unnormalized — do NOT normalize it (consumers are magnitude-invariant).
- No registered lights ⇒ `light_pdf = 0` ⇒ weights collapse to the plain path tracer.
- `start specular_bounce = true`, `prev_brdf_pdf = 0`.
- Only `src/camera/camera.rs` changes. Keep the existing `firefly_clamp` wiring, `cosine_direction`, and the `mixture_tests` module (they remain valid regression tests). Build pristine — no new warnings.

---

### Task 1: `power_heuristic` helper

**Files:**
- Modify: `src/camera/camera.rs` (free function + test)

**Interfaces:**
- Produces: `fn power_heuristic(a: f32, b: f32) -> f32` (private, module scope).

- [ ] **Step 1: Write the failing test**

Append to `src/camera/camera.rs`:

```rust
#[cfg(test)]
mod power_heuristic_tests {
    use super::power_heuristic;

    #[test]
    fn beta2_weights() {
        // 3^2 / (3^2 + 4^2) = 9/25 = 0.36
        assert!((power_heuristic(3.0, 4.0) - 0.36).abs() < 1e-6);
    }

    #[test]
    fn dominant_pdf_gets_full_weight() {
        assert!((power_heuristic(5.0, 0.0) - 1.0).abs() < 1e-6);
        assert!(power_heuristic(0.0, 5.0).abs() < 1e-6);
    }

    #[test]
    fn both_zero_is_zero_not_nan() {
        let w = power_heuristic(0.0, 0.0);
        assert_eq!(w, 0.0);
        assert!(!w.is_nan());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test power_heuristic_tests 2>&1 | tail -20`
Expected: compile error — `power_heuristic` is not defined.

- [ ] **Step 3: Implement the helper**

In `src/camera/camera.rs`, add a private free function at module scope (next to `cosine_direction`):

```rust
/// Power heuristic (β = 2) MIS weight for a technique with PDF `a` competing
/// against a technique with PDF `b`: `a² / (a² + b²)`. Returns 0 (not NaN) when
/// both PDFs are zero.
fn power_heuristic(a: f32, b: f32) -> f32 {
    let a2 = a * a;
    let b2 = b * b;
    let denom = a2 + b2;
    if denom > 0.0 {
        a2 / denom
    } else {
        0.0
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test power_heuristic_tests 2>&1 | tail -20`
Expected: `beta2_weights`, `dominant_pdf_gets_full_weight`, `both_zero_is_zero_not_nan` PASS.

- [ ] **Step 5: Commit**

```bash
git add src/camera/camera.rs
git commit -m "feat: add power_heuristic MIS weight helper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: rewrite `ray_color` as a power-heuristic MIS path tracer

**Files:**
- Modify: `src/camera/camera.rs` (`ray_color` body + new variance test)

**Interfaces:**
- Consumes: `power_heuristic` (Task 1); `Material::scattering_pdf`/`is_specular`/`emitted`/`scatter`; `IntersectGroup::light_pdf`/`sample_light_dir`/`intersect`; `cosine_direction`.
- Produces: an unbiased, lower-variance `ray_color` (same signature).

- [ ] **Step 1: Write the failing test**

Append to `src/camera/camera.rs` (a new module; it builds a *small, bright* overhead light so NEE's variance win over pure GI is large and unambiguous):

```rust
#[cfg(test)]
mod mis_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::group::{IntersectGroup, Light};
    use crate::material::{DiffuseLight, Lambertian};
    use crate::ray::{Intersect, Ray};
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn cam() -> Camera {
        Camera::from(
            CameraConfig::builder()
                .image_width(1)
                .aspect_ratio(1.0)
                .background(Color::ZERO)
                .build(),
        )
    }

    fn floor() -> Arc<dyn Intersect> {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.7, 0.7, 0.7)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 0.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    // Small, bright overhead light: pure GI rarely hits it (high variance);
    // NEE samples it every bounce (low variance).
    fn small_light() -> Arc<dyn Intersect> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(40.0, 40.0, 40.0)));
        Arc::new(Quad::new(
            Point3::new(-0.5, 4.0, -0.5),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            mat,
        ))
    }

    // Returns (mean, variance) of the .x channel over `n` samples.
    fn stats(register_light: bool, n: usize) -> (f32, f32) {
        let c = cam();
        let mut world = IntersectGroup::new();
        world.add(floor());
        let l = small_light();
        world.add(l.clone());
        if register_light {
            world.lights.push(Light { geom: l, emit: Color::new(40.0, 40.0, 40.0) });
        }
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        for _ in 0..n {
            let x = c.ray_color(&ray, &world, &mut rng).x;
            sum += x;
            sum2 += x * x;
        }
        let mean = sum / n as f32;
        let var = (sum2 / n as f32) - mean * mean;
        (mean, var)
    }

    #[test]
    fn mis_cuts_variance_versus_pure_gi() {
        let n = 4000;
        let (mis_mean, mis_var) = stats(true, n);
        let (gi_mean, gi_var) = stats(false, n);
        // Both lit and (being unbiased) roughly equal in mean.
        assert!(mis_mean > 0.0 && gi_mean > 0.0, "both lit: mis={mis_mean} gi={gi_mean}");
        // The whole point: NEE+MIS has markedly lower per-sample variance.
        assert!(
            mis_var < 0.5 * gi_var,
            "expected MIS variance well below pure-GI: mis_var={mis_var} gi_var={gi_var}"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test mis_tests 2>&1 | tail -20`
Expected: FAIL — the current one-sample mixture already reduces variance somewhat, but it is not the power-heuristic two-estimator MIS; with this small/bright light its per-sample variance does not satisfy `mis_var < 0.5 * gi_var` (the mixture still spends half its samples on cosine directions that miss the tiny light). The assertion fails until the explicit NEE+MIS rewrite lands.

(If by chance it passes on the current mixture, that is acceptable evidence the mixture is already strong; proceed with the rewrite regardless and confirm it still passes — the rewrite is the spec'd deliverable.)

- [ ] **Step 3: Rewrite `ray_color`**

In `src/camera/camera.rs`, replace the entire body of `fn ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color { ... }` with:

```rust
fn ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color {
    let interval = Interval::new(0.001, f32::INFINITY);
    let mut color = Color::ZERO;
    let mut throughput = Color::ones();
    let mut current = ray.clone();
    // Whether emission at the next hit gets full weight: true for the camera ray
    // and after specular bounces (NEE can't sample those), false after a diffuse
    // bounce (NEE already accounted for direct light, so MIS-weight the emission).
    let mut specular_bounce = true;
    let mut prev_brdf_pdf = 0.0_f32;

    for _ in 0..self.max_depth {
        let Some(hit) = world.intersect(&current, &interval) else {
            color += throughput * self.background;
            break;
        };

        let emitted = hit.material.emitted(hit.u, hit.v, hit.p);
        if emitted != Color::ZERO {
            if specular_bounce {
                color += throughput * emitted;
            } else {
                let p_light = world.light_pdf(current.origin, current.direction);
                let w = power_heuristic(prev_brdf_pdf, p_light);
                color += throughput * emitted * w;
            }
        }

        let Some((scattered, atten)) = hit.material.scatter(&current, &hit, rng) else {
            break; // pure light / absorber
        };

        if hit.material.is_specular() {
            throughput = throughput * atten;
            current = scattered;
            specular_bounce = true;
            continue;
        }

        // Lambertian.
        let albedo = atten;

        // (1) Next-event estimation: sample a light, weight against the BRDF pdf.
        if let Some(ld) = world.sample_light_dir(hit.p, rng) {
            let p_light = world.light_pdf(hit.p, ld);
            let p_brdf = hit.material.scattering_pdf(&hit, &ld);
            if p_light > 0.0 && p_brdf > 0.0 {
                let shadow = Ray::new_t(hit.p, ld, current.time);
                if let Some(lh) = world.intersect(&shadow, &interval) {
                    let le = lh.material.emitted(lh.u, lh.v, lh.p);
                    if le != Color::ZERO {
                        let w = power_heuristic(p_light, p_brdf);
                        color += throughput * w * albedo * (p_brdf / p_light) * le;
                    }
                }
            }
        }

        // (2) BRDF bounce (cosine), weighted against the light pdf at the next hit.
        let dir = cosine_direction(&hit.normal, rng);
        let p_brdf = hit.material.scattering_pdf(&hit, &dir);
        if p_brdf <= 0.0 {
            break;
        }
        throughput = throughput * albedo;
        prev_brdf_pdf = p_brdf;
        specular_bounce = false;
        current = Ray::new_t(hit.p, dir, current.time);
    }

    color
}
```

- [ ] **Step 4: Run the new and existing integrator tests**

Run: `cargo test mis_tests mixture_tests 2>&1 | tail -25`
Expected: `mis_cuts_variance_versus_pure_gi` PASSES, and the existing `mixture_tests` (`camera_sees_emitter_emission`, `mixture_matches_pure_gi_mean`) STILL PASS — confirming the rewrite preserves correctness (unbiased, direct emission intact) while adding the variance win.

- [ ] **Step 5: Run the full suite and confirm a clean build**

Run: `cargo test 2>&1 | grep -E "test result:|error" ; cargo build 2>&1 | tail -3`
Expected: all tests pass; no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/camera/camera.rs
git commit -m "feat: power-heuristic MIS path tracer (NEE + BRDF)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** `power_heuristic` helper + tests (Task 1); the MIS `ray_color` rewrite with NEE light sample (`power(p_light,p_brdf)·albedo·p_brdf/p_light·Le`), BRDF bounce (`throughput*=albedo`, record `prev_brdf_pdf`), emission accounting (`specular_bounce` full weight vs `power(prev_brdf_pdf, light_pdf)`), specular branch, no-lights collapse, unnormalized `ld`, firefly_clamp untouched (Task 2); unbiasedness preserved (existing `mixture_tests` kept) and variance-reduction verified (new test). All spec points covered.
- **Placeholder scan:** none — full code and commands throughout.
- **Type consistency:** `power_heuristic(a: f32, b: f32) -> f32` defined in Task 1 and called in Task 2 for both the NEE weight `(p_light, p_brdf)` and the emission weight `(prev_brdf_pdf, p_light)`. `ray_color` signature unchanged, so `sample_pixel`/`render`/tests keep compiling. Reuses `cosine_direction`, `scattering_pdf`, `light_pdf`, `sample_light_dir`, `emitted` with their established signatures.
