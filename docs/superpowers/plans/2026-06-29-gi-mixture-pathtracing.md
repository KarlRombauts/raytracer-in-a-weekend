# GI Blending via Mixture-PDF Path Tracing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the two separate integrators (`ray_color` pure-BRDF, `ray_color_direct` shadow-ray NEE) with one mixture-PDF path tracer that does full GI *and* light importance sampling in a single ray-per-bounce loop.

**Architecture:** Add `is_specular`/`scattering_pdf` to materials; add `light_pdf`/`sample_light_dir` to `IntersectGroup`; rewrite `Camera::ray_color` as a mixture path tracer (Lambertian hits sample one outgoing ray from a 50/50 mixture of cosine and light directions, weighted by `albedo·scattering_pdf/p_mixture`; specular hits use their `scatter()` ray directly). One ray per bounce ⇒ no double-count bookkeeping.

**Tech Stack:** Rust, existing `Vec3`/`Intersect`/`IntersectGroup`/`Camera`/materials, `rand::rngs::SmallRng`.

## Global Constraints

- One ray per bounce. Emission is added (throughput-weighted) at every hit; there is NO emission suppression and NO double-count logic.
- Mixture weight at a Lambertian hit: `throughput *= albedo * scattering_pdf(dir) / p_mixture(dir)`, where `p_mixture = 0.5*scattering_pdf(dir) + 0.5*light_pdf(dir)`, and with no lights `p_mixture = scattering_pdf` (weight collapses to `albedo`).
- `scattering_pdf` for Lambertian = `max(0, n·dir_unit) / PI` (use `std::f32::consts::PI`).
- Only non-specular (Lambertian) materials use the mixture. `is_specular()` materials (`Metal`/`Dielectric`/`Glossy`) use their `scatter()` ray + attenuation directly (`throughput *= atten`).
- `dir` may be unit (cosine-sampled) or unnormalized (light-sampled); all consumers (`scattering_pdf`, `pdf_value`, intersection) are magnitude-invariant. Do NOT normalize `dir` to "fix" anything.
- `light_pdf` = average of `light.geom.pdf_value(origin, dir)` over registered lights, 0 if none. `sample_light_dir` picks a uniformly random registered light and returns its `random_dir`, `None` if none.
- The mixture integrator becomes the single `Camera::ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color` (uses `self.max_depth` internally). The old pure-BRDF `ray_color` (with a `depth` param) and `ray_color_direct` are removed; both `sample_pixel` and `render()` call the new one.
- Build output must stay pristine — no new warnings. Obsolete tests of the removed `ray_color_direct` are removed.

---

### Task 1: Material `is_specular` + `scattering_pdf`

**Files:**
- Modify: `src/material/material.rs` (trait defaults)
- Modify: `src/material/lambertian.rs` (`scattering_pdf` override + tests)
- Modify: `src/material/metal.rs` (`is_specular` → true)
- Modify: `src/material/dielectric.rs` (`is_specular` → true)
- Modify: `src/material/glossy.rs` (`is_specular` → true)

**Interfaces:**
- Consumes: `HitRecord`, `Vec3`.
- Produces on the `Material` trait:
  - `fn is_specular(&self) -> bool` (default `false`; `true` for Metal/Dielectric/Glossy)
  - `fn scattering_pdf(&self, hit: &HitRecord, dir: &Vec3) -> f32` (default `0.0`; Lambertian = `max(0, n·dir_unit)/PI`)

- [ ] **Step 1: Write the failing tests**

Append to `src/material/lambertian.rs`:

```rust
#[cfg(test)]
mod pdf_tests {
    use super::*;
    use crate::material::{Dielectric, Glossy, Material, Metal};
    use crate::vec3::{Point3, Vec3};

    #[test]
    fn lambertian_scattering_pdf_is_cosine_over_pi() {
        let lam = Lambertian::from_color(Color::new(0.0, 0.0, 0.0));
        let hit = crate::ray::HitRecord::new(
            1.0,
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            &lam,
        );
        // dir straight up the normal: cos = 1 => 1/PI
        let up = lam.scattering_pdf(&hit, &Vec3::new(0.0, 1.0, 0.0));
        assert!((up - 1.0 / std::f32::consts::PI).abs() < 1e-6, "up={up}");
        // dir parallel to the surface: cos = 0 => 0
        let side = lam.scattering_pdf(&hit, &Vec3::new(1.0, 0.0, 0.0));
        assert!(side.abs() < 1e-6, "side={side}");
        // dir below the surface: clamped to 0
        let down = lam.scattering_pdf(&hit, &Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(down, 0.0);
    }

    #[test]
    fn specular_flags_are_correct() {
        assert!(!Lambertian::from_color(Color::new(0.0, 0.0, 0.0)).is_specular());
        assert!(Metal::new(Color::new(0.5, 0.5, 0.5), 0.0).is_specular());
        assert!(Dielectric::new(1.5).is_specular());
        assert!(Glossy::new(Color::new(0.5, 0.5, 0.5), 0.0).is_specular());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test pdf_tests 2>&1 | tail -20`
Expected: compile error — `scattering_pdf` and `is_specular` are not members of `Material`.

- [ ] **Step 3: Add the trait defaults**

In `src/material/material.rs`, change the imports line to also bring in `Vec3`:

```rust
use crate::{
    color::Color,
    ray::{HitRecord, Ray},
    vec3::{Point3, Vec3},
};
```

Add the two methods inside `pub trait Material` (after `emitted`):

```rust
    /// True for delta / near-delta BRDFs (mirror, glass, glossy coat) that are
    /// traced with their own scattered ray rather than mixture light-sampled.
    fn is_specular(&self) -> bool {
        false
    }

    /// Solid-angle PDF that this BRDF scatters into `dir` at `hit`. Default 0;
    /// diffuse materials override. `dir` need not be normalized.
    fn scattering_pdf(&self, _hit: &HitRecord, _dir: &Vec3) -> f32 {
        0.0
    }
```

- [ ] **Step 4: Override `scattering_pdf` for Lambertian**

In `src/material/lambertian.rs`, inside `impl Material for Lambertian`, add:

```rust
fn scattering_pdf(&self, hit_record: &HitRecord, dir: &crate::vec3::Vec3) -> f32 {
    let cos = hit_record.normal.dot(&dir.unit());
    (cos.max(0.0)) / std::f32::consts::PI
}
```

- [ ] **Step 5: Override `is_specular` for the specular materials**

In `src/material/metal.rs`, inside `impl Material for Metal`, add:

```rust
fn is_specular(&self) -> bool {
    true
}
```

In `src/material/dielectric.rs`, inside `impl Material for Dielectric`, add the same method.

In `src/material/glossy.rs`, inside `impl Material for Glossy`, add the same method.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test pdf_tests 2>&1 | tail -20`
Expected: `lambertian_scattering_pdf_is_cosine_over_pi` and `specular_flags_are_correct` PASS.

- [ ] **Step 7: Confirm no new warnings, then commit**

Run: `cargo build 2>&1 | tail -3`

```bash
git add src/material/material.rs src/material/lambertian.rs src/material/metal.rs src/material/dielectric.rs src/material/glossy.rs
git commit -m "feat: add is_specular + scattering_pdf to materials

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `light_pdf` + `sample_light_dir` on `IntersectGroup`

**Files:**
- Modify: `src/group.rs` (two methods + tests)

**Interfaces:**
- Consumes: `Intersect::pdf_value`, `Intersect::random_dir` (existing); `world.lights`.
- Produces:
  - `fn light_pdf(&self, origin: Point3, dir: Vec3) -> f32`
  - `fn sample_light_dir(&self, origin: Point3, rng: &mut SmallRng) -> Option<Vec3>`

- [ ] **Step 1: Write the failing tests**

Append to `src/group.rs`:

```rust
#[cfg(test)]
mod light_mixture_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    // Overhead quad light on the y=2 plane, area 4 (same as the analytic
    // pdf_value setup: pdf_value((0,0,0),(0,2,0)) == 1.0).
    fn overhead_light() -> Arc<dyn Intersect> {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        ))
    }

    #[test]
    fn light_pdf_is_zero_without_lights() {
        let w = IntersectGroup::new();
        assert_eq!(w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn light_pdf_averages_single_light() {
        let mut w = IntersectGroup::new();
        w.lights.push(Light { geom: overhead_light(), emit: Color::new(1.0, 1.0, 1.0) });
        // One light => average == that light's pdf_value; analytic value is 1.0.
        let p = w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        assert!((p - 1.0).abs() < 1e-5, "p={p}");
    }

    #[test]
    fn sample_light_dir_is_none_without_lights() {
        let w = IntersectGroup::new();
        let mut rng = SmallRng::seed_from_u64(1);
        assert!(w.sample_light_dir(Point3::new(0.0, 0.0, 0.0), &mut rng).is_none());
    }

    #[test]
    fn sample_light_dir_points_toward_the_light() {
        let mut w = IntersectGroup::new();
        w.lights.push(Light { geom: overhead_light(), emit: Color::new(1.0, 1.0, 1.0) });
        let mut rng = SmallRng::seed_from_u64(2);
        for _ in 0..100 {
            let d = w.sample_light_dir(Point3::new(0.0, 0.0, 0.0), &mut rng).unwrap();
            assert!(d.y > 0.0, "expected upward dir toward overhead light, got {:?}", d);
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test light_mixture_tests 2>&1 | tail -20`
Expected: compile error — `light_pdf` and `sample_light_dir` are not members of `IntersectGroup`.

- [ ] **Step 3: Implement the two methods**

In `src/group.rs`, inside `impl IntersectGroup` (the inherent impl block with `new`/`add`/`clear`, NOT the `impl Intersect`), add:

```rust
/// Average solid-angle PDF of sampling `dir` (from `origin`) toward any
/// registered light. 0 when there are no lights.
pub fn light_pdf(&self, origin: Point3, dir: Vec3) -> f32 {
    if self.lights.is_empty() {
        return 0.0;
    }
    let sum: f32 = self
        .lights
        .iter()
        .map(|l| l.geom.pdf_value(origin, dir))
        .sum();
    sum / self.lights.len() as f32
}

/// A random (unnormalized) direction from `origin` toward a uniformly chosen
/// registered light. `None` when there are no lights.
pub fn sample_light_dir(&self, origin: Point3, rng: &mut SmallRng) -> Option<Vec3> {
    if self.lights.is_empty() {
        return None;
    }
    let i = rng.random_range(0..self.lights.len());
    Some(self.lights[i].geom.random_dir(origin, rng))
}
```

(`SmallRng`, `Rng`, and `Point3` are already imported in `src/group.rs`; add any that are missing so `rng.random_range` and `Point3`/`Vec3` resolve.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test light_mixture_tests 2>&1 | tail -20`
Expected: all four `light_mixture_tests` PASS.

- [ ] **Step 5: Commit**

```bash
git add src/group.rs
git commit -m "feat: light_pdf and sample_light_dir on IntersectGroup

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: mixture-PDF integrator (single `ray_color`), wiring, and removals

**Files:**
- Modify: `src/camera/camera.rs` (new integrator, `cosine_direction` helper, rewire `sample_pixel`/`render`, remove old `ray_color`/`ray_color_direct` and their obsolete tests, add new tests)

**Interfaces:**
- Consumes: `Material::is_specular`/`scattering_pdf` (Task 1); `IntersectGroup::light_pdf`/`sample_light_dir`/`lights` (Task 2); existing `Intersect::intersect`, `Vec3::random_unit`/`near_zero`/`unit`.
- Produces: `fn ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color`.

This task swaps the integrator atomically (signature change + call-site updates + removal of the old methods and their tests must land together to compile).

- [ ] **Step 1: Write the failing tests**

Append to `src/camera/camera.rs`:

```rust
#[cfg(test)]
mod mixture_tests {
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
        let mat = Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 0.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    // Large overhead emitter (covers a big solid angle so pure-GI sampling
    // converges with feasible sample counts).
    fn ceiling_light() -> Arc<dyn Intersect> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 2.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    #[test]
    fn camera_sees_emitter_emission() {
        // Ray straight up into the emitter; no floor. The first hit is the
        // emitter: emission is added, scatter() is None, path ends.
        let c = cam();
        let mut world = IntersectGroup::new();
        world.add(ceiling_light());
        let ray = Ray::new(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(1);
        let col = c.ray_color(&ray, &world, &mut rng);
        assert!((col.x - 5.0).abs() < 1e-4, "expected emitter color, got {:?}", col);
    }

    fn avg_floor_color(register_light: bool) -> f32 {
        let c = cam();
        let mut world = IntersectGroup::new();
        world.add(floor());
        let light = ceiling_light();
        world.add(light.clone());
        if register_light {
            world.lights.push(Light { geom: light, emit: Color::new(5.0, 5.0, 5.0) });
        }
        // Look straight down at the floor centre.
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let n = 8000;
        let mut sum = 0.0;
        for _ in 0..n {
            sum += c.ray_color(&ray, &world, &mut rng).x;
        }
        sum / n as f32
    }

    #[test]
    fn mixture_matches_pure_gi_mean() {
        // With the light registered, the diffuse bounce is mixture-sampled
        // (light + cosine); unregistered, it is pure cosine GI. Both estimators
        // are unbiased, so their means must agree (mixture only cuts variance).
        let with_nee = avg_floor_color(true);
        let pure_gi = avg_floor_color(false);
        assert!(with_nee > 0.0 && pure_gi > 0.0, "both lit: nee={with_nee} gi={pure_gi}");
        let rel = (with_nee - pure_gi).abs() / pure_gi;
        assert!(rel < 0.15, "means should agree (unbiased): nee={with_nee} gi={pure_gi} rel={rel}");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test mixture_tests 2>&1 | tail -20`
Expected: compile error — `ray_color` currently takes a `depth` argument, so `c.ray_color(&ray, &world, &mut rng)` does not type-check.

- [ ] **Step 3: Add the `cosine_direction` helper**

In `src/camera/camera.rs`, add this private free function at module scope (e.g. just above `impl Camera`):

```rust
/// Cosine-weighted hemisphere direction about `normal` (PDF = cos/PI), using the
/// `normal + random_unit` trick. Returns a unit vector.
fn cosine_direction(normal: &Vec3, rng: &mut SmallRng) -> Vec3 {
    let mut d = *normal + Vec3::random_unit(rng);
    if d.near_zero() {
        d = *normal;
    }
    d.unit()
}
```

- [ ] **Step 4: Replace `ray_color_direct` with the mixture `ray_color`**

In `src/camera/camera.rs`, delete the entire `fn ray_color_direct(...) { ... }` method and replace it with the new integrator:

```rust
/// Mixture-PDF path tracer. One ray per bounce. At a diffuse (non-specular)
/// hit the outgoing ray is drawn from a 50/50 mixture of a cosine-weighted
/// direction and a direction toward a registered light, weighted by
/// `albedo * scattering_pdf / p_mixture`. Specular materials use their own
/// scattered ray. Emission is accumulated at every hit (one ray per bounce, so
/// no double counting).
fn ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color {
    let interval = Interval::new(0.001, f32::INFINITY);
    let mut color = Color::ZERO;
    let mut throughput = Color::ones();
    let mut current = ray.clone();

    for _ in 0..self.max_depth {
        let Some(hit) = world.intersect(&current, &interval) else {
            color += throughput * self.background;
            break;
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

        // Lambertian: sample one outgoing ray from the cosine/light mixture.
        let albedo = atten;
        let dir = if !world.lights.is_empty() && rng.random::<f32>() < 0.5 {
            match world.sample_light_dir(hit.p, rng) {
                Some(d) => d,
                None => cosine_direction(&hit.normal, rng),
            }
        } else {
            cosine_direction(&hit.normal, rng)
        };

        let s = hit.material.scattering_pdf(&hit, &dir);
        if s <= 0.0 {
            break; // direction below the surface contributes nothing
        }
        let p = if world.lights.is_empty() {
            s
        } else {
            0.5 * s + 0.5 * world.light_pdf(hit.p, dir)
        };
        if p <= 0.0 {
            break;
        }
        throughput = throughput * albedo * (s / p);
        current = Ray::new_t(hit.p, dir, current.time);
    }

    color
}
```

- [ ] **Step 5: Delete the old pure-BRDF `ray_color`**

In `src/camera/camera.rs`, delete the now-redundant old integrator entirely — the method

```rust
#[allow(dead_code)]
fn ray_color(&self, ray: &Ray, depth: u32, world: &IntersectGroup, rng: &mut SmallRng) -> Color {
    ... iterative pure-BRDF loop ...
}
```

(the one that takes a `depth` parameter and carries the `#[allow(dead_code)]` attribute). The new `ray_color` from Step 4 replaces it.

- [ ] **Step 6: Update the two call sites**

In `sample_pixel`, change the final line from `self.ray_color_direct(&ray, world, rng)` to:

```rust
self.ray_color(&ray, world, rng)
```

In `render`, change the sample loop body from `pixel_color += self.ray_color(&ray, self.max_depth, world, &mut rng);` to:

```rust
pixel_color += self.ray_color(&ray, world, &mut rng);
```

- [ ] **Step 7: Remove the obsolete `ray_color_direct` tests**

In `src/camera/camera.rs`, delete the two test modules that exercised the removed `ray_color_direct`: `mod direct_tests { ... }` and `mod pdf_direct_tests { ... }`. Leave `mod roll_tests` and the new `mod mixture_tests` in place.

- [ ] **Step 8: Run the new tests and the full suite**

Run: `cargo test mixture_tests 2>&1 | tail -20`
Expected: `camera_sees_emitter_emission` and `mixture_matches_pure_gi_mean` PASS.

Run: `cargo test 2>&1 | grep -E "test result:|error" ; cargo build 2>&1 | tail -3`
Expected: all tests pass; no new warnings; no references remain to `ray_color_direct` or the old `depth`-param `ray_color`.

- [ ] **Step 9: Commit**

```bash
git add src/camera/camera.rs
git commit -m "feat: mixture-PDF path tracer unifying GI and light sampling

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** `is_specular`/`scattering_pdf` defaults + Lambertian/specular overrides (Task 1); `light_pdf`/`sample_light_dir` (Task 2); the mixture integrator with the `albedo·s/p` weight, no-lights collapse, specular fallback, one-ray-no-suppression, `cosine_direction`, single `ray_color` replacing both old integrators, both call sites rewired, obsolete tests removed (Task 3); the unnormalized-`dir` invariant is respected (consumers normalize internally); deferred items (RR, power-heuristic MIS, sphere/composite light IS) untouched. All spec points covered.
- **Placeholder scan:** none — every code/command step is concrete.
- **Type consistency:** `is_specular(&self) -> bool` and `scattering_pdf(&self, hit: &HitRecord, dir: &Vec3) -> f32` match between trait (Task 1) and use (Task 3). `light_pdf(&self, origin: Point3, dir: Vec3) -> f32` and `sample_light_dir(&self, origin: Point3, rng: &mut SmallRng) -> Option<Vec3>` match between definition (Task 2) and use (Task 3). The new `ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color` matches the `sample_pixel`/`render`/test call sites.
