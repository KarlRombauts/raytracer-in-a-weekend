# Light PDF Weighting (Unbiased Direct Lighting) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make shadow-ray direct lighting physically correct for planar lights by dividing each light's contribution by its solid-angle PDF (geometry term) and adding the Lambertian `1/π` BRDF normalization.

**Architecture:** Add `area()`, a generic `pdf_value(origin, dir)`, and `random_dir(origin, rng)` to the `Intersect` trait; override `area()` for `Quad`/`Triangle`; register only `area() > 0` emitters as NEE lights; and rewrite the per-light body of `Camera::ray_color_direct` to the PDF-weighted form.

**Tech Stack:** Rust, existing `Vec3`/`Intersect`/`IntersectGroup`/`Camera`, `rand::rngs::SmallRng`.

## Global Constraints

- `pdf_value` and `random_dir` must stay object-safe (take concrete `&mut SmallRng`, plain `Point3`/`Vec3`), so `dyn Intersect` keeps working.
- The geometry term lives inside `pdf_value`: `pdf = dist² / (cos_light · area)`, computed by intersecting self along `dir`. `dist² = hit.t² · dir.length_squared()`; `cos_light = |dir·hit.normal| / dir.length()` (absolute value — two-sided light).
- `random_dir` returns the UNNORMALIZED direction `sample_point(rng) - origin`; the sampled light point therefore sits at parameter `t = 1` along it.
- Integrator contribution per light: `(albedo / π) · emit · cos_surface / pdf`, where `albedo` is `scatter`'s attenuation and `cos_surface = dot(hit.normal, dir.unit())`. Skip the light if `pdf <= 0` or `cos_surface <= 0`.
- Shadow-ray occlusion interval is `(0.001, 1.0 - 1e-4)` (blockers strictly before the light at `t = 1`).
- `build_world` registers a `DiffuseLight` object as an NEE light only when its built geometry has `area() > 0`; the object is still added to the world geometry regardless (so unsamplable emitters still glow).
- `Point3` is a type alias for `Vec3`. Build output must stay pristine — no new warnings.
- Scope: correct for `Quad` and `Triangle` only. `Sphere`, composites (`IntersectGroup`/`BVH`), and transform wrappers keep `area()` at the default 0 (not registered as lights) — deferred.

---

### Task 1: `area()`, generic `pdf_value`, `random_dir` on `Intersect` + Quad/Triangle `area()`

**Files:**
- Modify: `src/ray/intersect.rs` (three new trait methods)
- Modify: `src/geometry/quad.rs` (`area()` override + tests)
- Modify: `src/geometry/triangle.rs` (`area()` override + test)

**Interfaces:**
- Consumes: existing `Intersect::sample_point`, `Intersect::intersect`, `Vec3` ops (`cross`, `length`, `length_squared`, `dot`, `unit`).
- Produces on the `Intersect` trait:
  - `fn area(&self) -> f32` (default `0.0`)
  - `fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32` (generic default)
  - `fn random_dir(&self, origin: Point3, rng: &mut SmallRng) -> Vec3` (default `sample_point(rng) - origin`)
  - real `area()` for `Quad` and `Triangle`.

- [ ] **Step 1: Write the failing tests**

Append to `src/geometry/quad.rs`:

```rust
#[cfg(test)]
mod area_pdf_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use std::sync::Arc;

    // Overhead light on the y = 2 plane, spanning x,z in [-1,1]; area = 4.
    fn overhead_quad() -> Quad {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        )
    }

    #[test]
    fn area_is_cross_product_magnitude() {
        assert!((overhead_quad().area() - 4.0).abs() < 1e-5);
    }

    #[test]
    fn pdf_value_matches_analytic() {
        // Origin directly below the quad centre; dir points at the centre (0,2,0).
        // dist^2 = 4, cos = 1, area = 4  =>  pdf = dist^2 / (cos * area) = 1.0
        let p = overhead_quad().pdf_value(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        assert!((p - 1.0).abs() < 1e-5, "pdf={}", p);
    }

    #[test]
    fn pdf_value_zero_on_miss() {
        // Pointing away from the overhead quad never hits it.
        let p = overhead_quad().pdf_value(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, -2.0, 0.0));
        assert_eq!(p, 0.0);
    }
}
```

Append to `src/geometry/triangle.rs`:

```rust
#[cfg(test)]
mod area_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use std::sync::Arc;

    #[test]
    fn area_is_half_cross_product() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            mat,
        );
        // |u x v| = |(2,0,0) x (0,3,0)| = 6; triangle area = 3.
        assert!((tri.area() - 3.0).abs() < 1e-5);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test area_pdf_tests area_tests 2>&1 | tail -20`
Expected: compile error — `area` and `pdf_value` are not yet members of `Intersect`.

- [ ] **Step 3: Add the three trait methods**

In `src/ray/intersect.rs`, add inside `pub trait Intersect` (after `sample_point`):

```rust
    /// Surface area, for area-light PDFs. Default 0 — a shape that returns 0 is
    /// treated as not directly sampleable (see `build_world`).
    fn area(&self) -> f32 {
        0.0
    }

    /// Solid-angle PDF of sampling direction `dir` (from `origin`) toward this
    /// object. Generic default: intersect self along `dir`, then convert the
    /// uniform `1/area` area PDF to solid angle via the hit distance and the
    /// light's facing cosine. Correct for any convex single surface that reports
    /// a real `area()`. `dir` may be unnormalized.
    fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32 {
        let ray = Ray::new(origin, dir);
        match self.intersect(&ray, &Interval::new(0.001, f32::INFINITY)) {
            None => 0.0,
            Some(hit) => {
                let dist2 = hit.t * hit.t * dir.length_squared();
                let cos = (dir.dot(&hit.normal) / dir.length()).abs();
                let a = self.area();
                if cos < 1e-8 || a <= 0.0 {
                    0.0
                } else {
                    dist2 / (cos * a)
                }
            }
        }
    }

    /// A random (unnormalized) direction from `origin` toward a point on this
    /// object. Reuses `sample_point`, so it composes through groups/BVH/transforms.
    fn random_dir(&self, origin: Point3, rng: &mut SmallRng) -> Vec3 {
        self.sample_point(rng) - origin
    }
```

- [ ] **Step 4: Add the Quad `area()` override**

In `src/geometry/quad.rs`, inside `impl Intersect for Quad` (alongside `sample_point`), add:

```rust
fn area(&self) -> f32 {
    self.u.cross(&self.v).length()
}
```

- [ ] **Step 5: Add the Triangle `area()` override**

In `src/geometry/triangle.rs`, inside `impl Intersect for Triangle`, add:

```rust
fn area(&self) -> f32 {
    0.5 * self.u.cross(&self.v).length()
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test area_pdf_tests area_tests 2>&1 | tail -20`
Expected: `area_is_cross_product_magnitude`, `pdf_value_matches_analytic`, `pdf_value_zero_on_miss`, `area_is_half_cross_product` all PASS.

- [ ] **Step 7: Confirm no new warnings, then commit**

Run: `cargo build 2>&1 | tail -3` (no new warnings from the changed files).

```bash
git add src/ray/intersect.rs src/geometry/quad.rs src/geometry/triangle.rs
git commit -m "feat: add area/pdf_value/random_dir for area-light sampling

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: register only `area() > 0` emitters in `build_world`

**Files:**
- Modify: `src/scene.rs` (`build_world` guard + test)

**Interfaces:**
- Consumes: `Intersect::area` (Task 1), `MaterialSpec::DiffuseLight`, `IntersectGroup`/`Light`.
- Produces: `build_world` that registers a light only when `geom.area() > 0`.

- [ ] **Step 1: Write the failing test**

Append to `src/scene.rs`:

```rust
#[cfg(test)]
mod registration_tests {
    use super::*;
    use crate::camera::CameraConfig;

    #[test]
    fn only_area_lights_are_registered() {
        let quad_light = ObjectSpec {
            name: "quad".to_string(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 5.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 1.0),
            },
            material: MaterialSpec::DiffuseLight { emit: Color::new(5.0, 5.0, 5.0) },
            transform: Transform::identity(),
        };
        let sphere_light = ObjectSpec {
            name: "sphere".to_string(),
            shape: Shape::Sphere { center: Point3::new(0.0, 0.0, 0.0), radius: 1.0 },
            material: MaterialSpec::DiffuseLight { emit: Color::new(5.0, 5.0, 5.0) },
            transform: Transform::identity(),
        };
        let scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![quad_light, sphere_light],
        };
        let world = build_world(&scene);
        // Sphere keeps area()=0 (deferred) => not registered; quad is.
        assert_eq!(world.lights.len(), 1, "only the quad (area>0) should register");
        // Both objects still live in the world geometry (the sphere still glows).
        assert_eq!(world.objects.len(), 2, "both objects remain in the world");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test registration_tests 2>&1 | tail -20`
Expected: FAIL — `world.lights.len()` is 2 (the sphere is still registered without the guard).

- [ ] **Step 3: Add the `area() > 0` guard**

In `src/scene.rs`, change the `if let` block inside `build_world` from:

```rust
        if let MaterialSpec::DiffuseLight { emit } = &obj.material {
            world.lights.push(Light {
                geom,
                emit: *emit,
            });
        }
```

to:

```rust
        if let MaterialSpec::DiffuseLight { emit } = &obj.material {
            // Only register emitters we can importance-sample (area() > 0).
            // Others (sphere/mesh/transformed) still glow when hit directly,
            // they're just not shadow-ray sampled.
            if geom.area() > 0.0 {
                world.lights.push(Light {
                    geom,
                    emit: *emit,
                });
            }
        }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test registration_tests 2>&1 | tail -20`
Expected: `only_area_lights_are_registered` PASS.

- [ ] **Step 5: Commit**

```bash
git add src/scene.rs
git commit -m "feat: register only area()>0 emitters as NEE lights

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: PDF-weight the direct-lighting integrator

**Files:**
- Modify: `src/camera/camera.rs` (`ray_color_direct` per-light body + test)

**Interfaces:**
- Consumes: `Intersect::random_dir`, `Intersect::pdf_value` (Task 1); `world.lights` (already populated).
- Produces: an unbiased (for planar lights) `ray_color_direct`.

- [ ] **Step 1: Write the failing test**

Append to `src/camera/camera.rs`:

```rust
#[cfg(test)]
mod pdf_direct_tests {
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

    fn test_camera() -> Camera {
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
            Point3::new(-100.0, 0.0, -100.0),
            Vec3::new(200.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 200.0),
            mat,
        ))
    }

    // Small (1x1) downward-facing light directly overhead at height h.
    fn small_light(h: f32) -> Arc<dyn Intersect> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0)));
        Arc::new(Quad::new(
            Point3::new(-0.5, h, -0.5),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            mat,
        ))
    }

    fn world_with_light(h: f32) -> IntersectGroup {
        let mut w = IntersectGroup::new();
        w.add(floor());
        let lq = small_light(h);
        w.add(lq.clone());
        w.lights.push(Light { geom: lq, emit: Color::new(5.0, 5.0, 5.0) });
        w
    }

    fn avg_direct(h: f32) -> f32 {
        let cam = test_camera();
        let world = world_with_light(h);
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(42);
        let n = 4000;
        let mut sum = 0.0;
        for _ in 0..n {
            sum += cam.ray_color_direct(&ray, &world, &mut rng).x;
        }
        sum / n as f32
    }

    #[test]
    fn direct_falls_off_with_inverse_square() {
        // A small overhead light approximates a point source, so doubling its
        // height should quarter the direct illumination (the dist^2 / cos_light
        // / area geometry term, all inside pdf_value).
        let near = avg_direct(10.0);
        let far = avg_direct(20.0);
        assert!(near > 0.0 && far > 0.0, "both should be lit: near={near} far={far}");
        let ratio = near / far;
        assert!((ratio - 4.0).abs() < 0.6, "expected ~4x falloff, got {ratio}");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test pdf_direct_tests 2>&1 | tail -20`
Expected: FAIL — the current biased integrator (`albedo * emit * cos`, no `dist²`/area term) does NOT fall off as `1/dist²`, so the ratio is far from 4.

- [ ] **Step 3: Rewrite the per-light loop body**

In `src/camera/camera.rs`, replace the entire per-light loop in `ray_color_direct` (the `for light in &world.lights { ... }` block, from `let lp = light.geom.sample_point(rng);` through the closing `}` of the loop body) with:

```rust
        let mut direct = Color::ZERO;
        for light in &world.lights {
            // Unnormalized direction toward a random point on the light; that
            // point sits at parameter t = 1 along `dir`.
            let dir = light.geom.random_dir(hit.p, rng);
            let pdf = light.geom.pdf_value(hit.p, dir);
            if pdf <= 0.0 {
                continue;
            }
            let unit = dir.unit();
            let cos_surface = hit.normal.dot(&unit);
            if cos_surface <= 0.0 {
                continue; // light is behind the surface
            }
            let shadow = Ray::new_t(hit.p, dir, ray.time);
            // Check for blockers strictly before the light (at t = 1).
            let shadow_interval = Interval::new(0.001, 1.0 - 1e-4);
            if world.intersect(&shadow, &shadow_interval).is_none() {
                // Geometry term (cos_light, area, dist^2) lives inside `pdf`.
                direct += (albedo / std::f32::consts::PI) * light.emit * cos_surface / pdf;
            }
        }
```

Leave the surrounding code (`emitted`, the `scatter` match, the final `emitted + direct`) unchanged.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test pdf_direct_tests 2>&1 | tail -20`
Expected: `direct_falls_off_with_inverse_square` PASS.

- [ ] **Step 5: Run the full suite and confirm a clean build**

Run: `cargo test 2>&1 | grep -E "test result:|error" ; cargo build 2>&1 | tail -3`
Expected: all tests pass; no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/camera/camera.rs
git commit -m "feat: PDF-weight direct lighting (unbiased for planar lights)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** `area()`/`pdf_value`/`random_dir` trait additions + Quad/Triangle `area()` (Task 1); the generic `pdf_value` geometry-term math (Task 1, tested analytically); `build_world` `area() > 0` registration guard with emitters still glowing (Task 2); PDF-weighted integrator with `(albedo/π)·emit·cos_surface/pdf`, `pdf<=0`/`cos<=0` skips, and the `(0.001, 1-1e-4)` shadow interval (Task 3); inverse-square verification (Task 3); deferred sphere/composite/transform left at `area()=0` (Global Constraints + no overrides added). All spec points covered.
- **Placeholder scan:** none — every code/command step is concrete.
- **Type consistency:** `area(&self) -> f32`, `pdf_value(&self, origin: Point3, dir: Vec3) -> f32`, `random_dir(&self, origin: Point3, rng: &mut SmallRng) -> Vec3` are defined in Task 1 and called identically in Tasks 2 (`geom.area()`) and 3 (`light.geom.random_dir`/`pdf_value`). `Light { geom, emit }` matches the existing struct. The integrator math matches the spec verbatim.
