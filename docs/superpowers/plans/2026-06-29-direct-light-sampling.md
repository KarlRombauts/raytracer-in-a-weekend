# Direct Light Sampling (Shadow Rays, No PDF) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add shadow-ray direct lighting to the path tracer — sample a random point on each light, test occlusion, and shade visible diffuse surfaces — with no PDF/geometry weighting yet (intentionally biased; the next steps add the bounce and the PDF).

**Architecture:** Extend the existing `Intersect` trait with a `sample_point` method (default = bounding-box center, overridden by sampleable shapes), collect emissive objects into a `lights` list on the world `IntersectGroup` at build time, and add a `ray_color_direct` integrator on `Camera` that the progressive viewer calls instead of the path-tracing `ray_color`.

**Tech Stack:** Rust, existing `Vec3`/`Intersect`/`IntersectGroup`, `rand::rngs::SmallRng`.

## Global Constraints

- `sample_point` must be object-safe: it takes `&mut SmallRng` (a concrete type), NOT a generic `impl Rng`, so `dyn Intersect` keeps working.
- The shadow-ray occlusion test uses interval `(0.001, dist - 0.001)` so the light's own surface is never counted as an occluder.
- No PDF and no geometry term this step: the direct contribution is exactly `albedo * light.emit * cos_theta`, where `cos_theta = dot(hit.normal, dir_to_light)`.
- Sampling is uniform over the whole light surface — no front-facing restriction, no area-weighting.
- The existing `Camera::ray_color` (path tracer) must remain and stay referenced (the offline `Camera::render` keeps calling it); only `Camera::sample_pixel` switches to `ray_color_direct`. This preserves the path tracer for the next step and avoids a dead-code warning.
- `Point3` is a type alias for `Vec3` (`crate::vec3::Point3`); either name compiles.
- Build output must stay pristine — no new warnings.

---

### Task 1: `Intersect::sample_point` + leaf-shape overrides (Quad, Triangle, Sphere)

**Files:**
- Modify: `src/ray/intersect.rs` (add trait method with default)
- Modify: `src/geometry/quad.rs` (override + test)
- Modify: `src/geometry/triangle.rs` (override + test)
- Modify: `src/geometry/sphere.rs` (override + test)

**Interfaces:**
- Consumes: nothing new.
- Produces: `fn sample_point(&self, rng: &mut SmallRng) -> Point3` on the `Intersect` trait, with a default returning `self.center()`, and real overrides for `Quad`, `Triangle`, `Sphere`.

- [ ] **Step 1: Add the trait method with a default**

In `src/ray/intersect.rs`, change the imports line and add the method. The file currently reads:

```rust
use crate::{interval::Interval, ray::*, vec3::Vec3};

pub trait Intersect: Send + Sync {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>>;

    fn bounding_box(&self) -> &AABB;

    fn center(&self) -> Vec3;
}
```

Replace it with:

```rust
use crate::{interval::Interval, ray::*, vec3::{Point3, Vec3}};
use rand::rngs::SmallRng;

pub trait Intersect: Send + Sync {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>>;

    fn bounding_box(&self) -> &AABB;

    fn center(&self) -> Vec3;

    /// Sample a point on this object's surface, for light sampling. The default
    /// returns the bounding-box center; shapes that can act as lights override
    /// it. Takes a concrete `SmallRng` to stay object-safe (`dyn Intersect`).
    fn sample_point(&self, _rng: &mut SmallRng) -> Point3 {
        self.center()
    }
}
```

- [ ] **Step 2: Write the failing tests for the leaf shapes**

Append to `src/geometry/quad.rs`:

```rust
#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn sampled_point_lies_on_quad() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let q = Quad::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            mat,
        );
        let mut rng = SmallRng::seed_from_u64(1);
        for _ in 0..500 {
            let p = q.sample_point(&mut rng);
            assert!(p.z.abs() < 1e-5, "off-plane: {:?}", p);
            assert!((0.0..=2.0).contains(&p.x), "x out: {}", p.x);
            assert!((0.0..=3.0).contains(&p.y), "y out: {}", p.y);
        }
    }
}
```

Append to `src/geometry/triangle.rs`:

```rust
#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn sampled_point_is_inside_triangle() {
        // Vertices q=(0,0,0), q+u=(1,0,0), q+v=(0,1,0): a sample (x,y,0) must
        // satisfy the barycentric bounds x>=0, y>=0, x+y<=1.
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat,
        );
        let mut rng = SmallRng::seed_from_u64(2);
        for _ in 0..500 {
            let p = tri.sample_point(&mut rng);
            assert!(p.z.abs() < 1e-5, "off-plane: {:?}", p);
            assert!(p.x >= -1e-5 && p.y >= -1e-5, "negative bary: {:?}", p);
            assert!(p.x + p.y <= 1.0 + 1e-5, "outside tri: {:?}", p);
        }
    }
}
```

Append to `src/geometry/sphere.rs`:

```rust
#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn sampled_point_is_on_sphere_surface() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let center = Point3::new(1.0, 2.0, 3.0);
        let radius = 5.0;
        let s = Sphere::stationary(center, radius, mat);
        let mut rng = SmallRng::seed_from_u64(3);
        for _ in 0..500 {
            let p = s.sample_point(&mut rng);
            let r = (p - center).length();
            assert!((r - radius).abs() < 1e-3, "off-surface: r={}", r);
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test sample_tests 2>&1 | tail -20`
Expected: compile error — `sample_point` is defined (default), but the assertions fail because the default returns `center()` (the quad/triangle tests will fail their bounds, the sphere test will fail the radius check). Either a failure or, if it compiles, FAILED assertions — both confirm the override is needed.

- [ ] **Step 4: Implement the Quad override**

In `src/geometry/quad.rs`, add these imports near the top (after the existing `use` lines):

```rust
use rand::rngs::SmallRng;
use rand::Rng;
```

Then inside `impl Intersect for Quad` (alongside `center`/`intersect`/`bounding_box`), add:

```rust
fn sample_point(&self, rng: &mut SmallRng) -> crate::vec3::Point3 {
    let a: f32 = rng.random();
    let b: f32 = rng.random();
    self.q + a * self.u + b * self.v
}
```

- [ ] **Step 5: Implement the Triangle override**

In `src/geometry/triangle.rs`, add near the top:

```rust
use rand::rngs::SmallRng;
use rand::Rng;
```

Then inside `impl Intersect for Triangle`, add (uniform barycentric sampling; vertices are `q`, `q+u`, `q+v`):

```rust
fn sample_point(&self, rng: &mut SmallRng) -> crate::vec3::Point3 {
    let r1: f32 = rng.random();
    let r2: f32 = rng.random();
    let su = r1.sqrt();
    let p1 = self.q;
    let p2 = self.q + self.u;
    let p3 = self.q + self.v;
    (1.0 - su) * p1 + (su * (1.0 - r2)) * p2 + (su * r2) * p3
}
```

- [ ] **Step 6: Implement the Sphere override**

In `src/geometry/sphere.rs`, add near the top:

```rust
use rand::rngs::SmallRng;
```

Then inside `impl Intersect for Sphere`, add (the sphere stores its center as a `Ray` for motion blur; sample at time 0):

```rust
fn sample_point(&self, rng: &mut SmallRng) -> crate::vec3::Point3 {
    let center = self.center.at(0.0);
    center + self.radius * Vec3::random_unit(rng)
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test sample_tests 2>&1 | tail -20`
Expected: `sampled_point_lies_on_quad`, `sampled_point_is_inside_triangle`, `sampled_point_is_on_sphere_surface` all PASS.

- [ ] **Step 8: Confirm no new warnings, then commit**

Run: `cargo build 2>&1 | grep -c "warning: unused"` (the count should not increase relative to the pre-existing baseline; the new code introduces none).

```bash
git add src/ray/intersect.rs src/geometry/quad.rs src/geometry/triangle.rs src/geometry/sphere.rs
git commit -m "feat: add Intersect::sample_point for quad, triangle, sphere

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `sample_point` for composites and transforms

**Files:**
- Modify: `src/group.rs` (`IntersectGroup` override + test)
- Modify: `src/ray/bvh/flat_bvh.rs` (`BVH` override + test)
- Modify: `src/geometry/transform.rs` (`Translate`/`Scale`/`Rotate` overrides + test)

**Interfaces:**
- Consumes: `Intersect::sample_point` (Task 1).
- Produces: `sample_point` overrides on `IntersectGroup`, `BVH<T>`, `Translate`, `Scale`, `Rotate` so light sampling composes through collections and transforms.

- [ ] **Step 1: Write the failing test for `IntersectGroup`**

Append to `src/group.rs`:

```rust
#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn group_samples_one_of_its_children() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let q_a = Arc::new(Quad::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat.clone(),
        ));
        let q_b = Arc::new(Quad::new(
            Point3::new(10.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat,
        ));
        let mut g = IntersectGroup::new();
        g.add(q_a);
        g.add(q_b);

        let mut rng = SmallRng::seed_from_u64(4);
        let (mut hit_a, mut hit_b) = (false, false);
        for _ in 0..500 {
            let p = g.sample_point(&mut rng);
            let in_a = (0.0..=1.0).contains(&p.x);
            let in_b = (10.0..=11.0).contains(&p.x);
            assert!(in_a || in_b, "point on neither child: {:?}", p);
            hit_a |= in_a;
            hit_b |= in_b;
        }
        assert!(hit_a && hit_b, "expected to sample both children");
    }
}
```

- [ ] **Step 2: Write the failing test for `Translate`**

Append to `src/geometry/transform.rs`:

```rust
#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn translate_forwards_sampled_point() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let quad = Arc::new(Quad::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            mat,
        ));
        let offset = Vec3::new(5.0, 1.0, -3.0);
        let t = Translate::new(quad, offset);
        let mut rng = SmallRng::seed_from_u64(5);
        for _ in 0..500 {
            let p = t.sample_point(&mut rng);
            assert!((5.0..=7.0).contains(&p.x), "x {}", p.x);
            assert!((1.0..=3.0).contains(&p.y), "y {}", p.y);
            assert!((p.z + 3.0).abs() < 1e-5, "z {}", p.z);
        }
    }
}
```

- [ ] **Step 3: Write the failing test for `BVH`**

Append to `src/ray/bvh/flat_bvh.rs`:

```rust
#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Triangle;
    use crate::material::Lambertian;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn bvh_samples_point_on_a_primitive() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let t1 = Triangle::from_points(
            &Point3::new(0.0, 0.0, 0.0),
            &Point3::new(1.0, 0.0, 0.0),
            &Point3::new(0.0, 1.0, 0.0),
            mat.clone(),
        );
        let t2 = Triangle::from_points(
            &Point3::new(0.0, 0.0, 5.0),
            &Point3::new(1.0, 0.0, 5.0),
            &Point3::new(0.0, 1.0, 5.0),
            mat,
        );
        let bvh = BVH::build(vec![t1, t2]);
        let mut rng = SmallRng::seed_from_u64(6);
        for _ in 0..500 {
            let p = bvh.sample_point(&mut rng);
            assert!(
                p.x >= -1e-4 && p.y >= -1e-4 && p.x + p.y <= 1.0 + 1e-4,
                "bad bary: {:?}",
                p
            );
            assert!(
                p.z.abs() < 1e-3 || (p.z - 5.0).abs() < 1e-3,
                "off both tris: z={}",
                p.z
            );
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test sample_tests 2>&1 | tail -25`
Expected: the three new tests FAIL — the default `sample_point` returns `center()`, so points land at collection/transform centers, violating the per-child / forwarded bounds.

- [ ] **Step 5: Implement the `IntersectGroup` override**

In `src/group.rs`, add near the top imports:

```rust
use crate::color::Color;
use crate::vec3::Point3;
use rand::rngs::SmallRng;
use rand::Rng;
```

Inside `impl Intersect for IntersectGroup`, add:

```rust
fn sample_point(&self, rng: &mut SmallRng) -> Point3 {
    if self.objects.is_empty() {
        return self.center();
    }
    let i = rng.random_range(0..self.objects.len());
    self.objects[i].sample_point(rng)
}
```

(The `Color` import is unused until Task 3; if `cargo build` warns about it here, defer adding `use crate::color::Color;` to Task 3 instead.)

- [ ] **Step 6: Implement the `BVH` override**

In `src/ray/bvh/flat_bvh.rs`, add near the top imports:

```rust
use crate::vec3::Point3;
use rand::rngs::SmallRng;
use rand::Rng;
```

Inside `impl<T: Intersect> Intersect for BVH<T>` (around line 320, alongside `center`/`bounding_box`/`intersect`), add:

```rust
fn sample_point(&self, rng: &mut SmallRng) -> Point3 {
    if self.primitives.is_empty() {
        return self.center();
    }
    let i = rng.random_range(0..self.primitives.len());
    self.primitives[i].sample_point(rng)
}
```

- [ ] **Step 7: Implement the transform overrides**

In `src/geometry/transform.rs`, add near the top imports:

```rust
use crate::vec3::Point3;
use rand::rngs::SmallRng;
```

Inside `impl Intersect for Translate`, add:

```rust
fn sample_point(&self, rng: &mut SmallRng) -> Point3 {
    self.object.sample_point(rng) + self.offset
}
```

Inside `impl Intersect for Scale`, add:

```rust
fn sample_point(&self, rng: &mut SmallRng) -> Point3 {
    self.object.sample_point(rng) * self.scale
}
```

Inside `impl Intersect for Rotate`, add (reuses the module-private `apply` matrix helper, mirroring how `intersect` maps `hit.p` back to world):

```rust
fn sample_point(&self, rng: &mut SmallRng) -> Point3 {
    apply(&self.fwd, self.object.sample_point(rng))
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test sample_tests 2>&1 | tail -25`
Expected: all `sample_tests` PASS (Task 1's three plus `group_samples_one_of_its_children`, `translate_forwards_sampled_point`, `bvh_samples_point_on_a_primitive`).

- [ ] **Step 9: Commit**

```bash
git add src/group.rs src/ray/bvh/flat_bvh.rs src/geometry/transform.rs
git commit -m "feat: sample_point for IntersectGroup, BVH, and transforms

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `Light` list on the world, populated by `build_world`

**Files:**
- Modify: `src/group.rs` (`Light` struct + `lights` field)
- Modify: `src/scene.rs` (collect lights in `build_world` + test)

**Interfaces:**
- Consumes: `IntersectGroup`, `MaterialSpec::DiffuseLight { emit }`, `ObjectSpec::build`.
- Produces:
  - `pub struct Light { pub geom: Arc<dyn Intersect>, pub emit: Color }` in `src/group.rs`.
  - `pub lights: Vec<Light>` field on `IntersectGroup`, empty by default, populated by `build_world`.

- [ ] **Step 1: Write the failing test**

Append to `src/scene.rs` (a new test module):

```rust
#[cfg(test)]
mod light_tests {
    use super::*;
    use crate::scenes::cornell_box;

    #[test]
    fn cornell_box_collects_one_light() {
        let scene = cornell_box();
        let world = build_world(&scene);
        assert_eq!(world.lights.len(), 1, "expected exactly one light");
        assert_eq!(world.lights[0].emit, Color::new(15.0, 15.0, 15.0));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test light_tests 2>&1 | tail -20`
Expected: compile error — `IntersectGroup` has no field `lights` and `Light` does not exist.

- [ ] **Step 3: Add the `Light` struct and `lights` field**

In `src/group.rs`, ensure these imports are present near the top (add any missing):

```rust
use crate::color::Color;
use std::sync::Arc;
```

Add the `Light` struct (above or below `IntersectGroup`):

```rust
/// An emissive object the integrator can sample directly (next event
/// estimation). Pairs a sampleable geometry handle with a constant emission.
pub struct Light {
    pub geom: Arc<dyn Intersect>,
    pub emit: Color,
}
```

Add the field to `IntersectGroup` and initialise it in `new`:

```rust
pub struct IntersectGroup {
    pub objects: Vec<Arc<dyn Intersect>>,
    pub lights: Vec<Light>,
    bbox: AABB,
}
```

```rust
pub fn new() -> Self {
    IntersectGroup {
        objects: Vec::new(),
        lights: Vec::new(),
        bbox: AABB::EMPTY,
    }
}
```

- [ ] **Step 4: Collect lights in `build_world`**

In `src/scene.rs`, change the import of `IntersectGroup` to also bring in `Light`:

```rust
use crate::group::{IntersectGroup, Light};
```

Replace `build_world` with:

```rust
/// Assemble the renderable world from the scene description. Cheap enough to
/// call on every edit (Mesh handles are shared, not rebuilt). Emissive objects
/// are also registered in `world.lights` for direct light sampling.
pub fn build_world(scene: &Scene) -> IntersectGroup {
    let mut world = IntersectGroup::new();
    for obj in &scene.objects {
        let geom = obj.build();
        world.add(geom.clone());
        if let MaterialSpec::DiffuseLight { emit } = &obj.material {
            world.lights.push(Light {
                geom,
                emit: *emit,
            });
        }
    }
    world
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test light_tests 2>&1 | tail -20`
Expected: `cornell_box_collects_one_light` PASS.

- [ ] **Step 6: Commit**

```bash
git add src/group.rs src/scene.rs
git commit -m "feat: collect emissive objects into world.lights

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `ray_color_direct` integrator and viewer switch

**Files:**
- Modify: `src/camera/camera.rs` (add `ray_color_direct`, switch `sample_pixel`, add test)

**Interfaces:**
- Consumes: `IntersectGroup` (with `lights`), `Intersect::sample_point`, `Material::scatter`/`emitted`, `Light`.
- Produces: `fn ray_color_direct(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color` on `Camera`; `sample_pixel` calls it.

- [ ] **Step 1: Write the failing test**

Append to `src/camera/camera.rs` (a new test module):

```rust
#[cfg(test)]
mod direct_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::group::{IntersectGroup, Light};
    use crate::material::{DiffuseLight, Lambertian};
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

    fn floor() -> Arc<Quad> {
        let mat = Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0)));
        Arc::new(Quad::new(
            Point3::new(-1.0, 0.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        ))
    }

    fn light_quad() -> Arc<Quad> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0)));
        Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        ))
    }

    #[test]
    fn occluded_point_is_darker_than_lit_point() {
        let cam = test_camera();
        // Camera ray straight down onto the floor centre at (0,0,0).
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));

        let mut lit = IntersectGroup::new();
        lit.add(floor());
        lit.add(light_quad());
        lit.lights.push(Light { geom: light_quad(), emit: Color::new(5.0, 5.0, 5.0) });

        let mut rng = SmallRng::seed_from_u64(1);
        let lit_color = cam.ray_color_direct(&ray, &lit, &mut rng);
        assert!(lit_color.x > 0.0, "expected lit floor, got {:?}", lit_color);

        // Same scene plus a blocker quad between the floor and the light.
        let mut blocked = IntersectGroup::new();
        blocked.add(floor());
        blocked.add(light_quad());
        let blocker_mat = Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0)));
        blocked.add(Arc::new(Quad::new(
            Point3::new(-1.0, 1.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            blocker_mat,
        )));
        blocked.lights.push(Light { geom: light_quad(), emit: Color::new(5.0, 5.0, 5.0) });
        let occ_color = cam.ray_color_direct(&ray, &blocked, &mut rng);
        assert!(
            occ_color.x < lit_color.x,
            "occluded should be darker: lit {:?} occ {:?}",
            lit_color,
            occ_color
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test direct_tests 2>&1 | tail -20`
Expected: compile error — `Camera::ray_color_direct` does not exist.

- [ ] **Step 3: Implement `ray_color_direct`**

In `src/camera/camera.rs`, inside `impl Camera` (next to `ray_color`), add:

```rust
/// Direct-lighting integrator: shade a hit by sampling each light with a
/// shadow ray. No indirect bounce and no PDF/geometry weighting yet — the
/// contribution is `albedo * emit * cos`, so the result is intentionally
/// biased (a stepping stone toward full next-event estimation).
fn ray_color_direct(
    &self,
    ray: &Ray,
    world: &IntersectGroup,
    rng: &mut SmallRng,
) -> Color {
    let interval = Interval::new(0.001, f32::INFINITY);
    let Some(hit) = world.intersect(ray, &interval) else {
        return self.background;
    };

    let emitted = hit.material.emitted(hit.u, hit.v, hit.p);

    // No scatter => the surface is a light (or pure absorber); show its emission.
    let Some((_, albedo)) = hit.material.scatter(ray, &hit, rng) else {
        return emitted;
    };

    let mut direct = Color::ZERO;
    for light in &world.lights {
        let lp = light.geom.sample_point(rng);
        let to = lp - hit.p;
        let dist = to.length();
        if dist <= 0.0 {
            continue;
        }
        let dir = to / dist;
        let cos = hit.normal.dot(&dir);
        if cos <= 0.0 {
            continue; // light is behind the surface
        }
        let shadow = Ray::new_t(hit.p, dir, ray.time);
        // Stop just short of the light so its own surface is not an occluder.
        let shadow_interval = Interval::new(0.001, dist - 0.001);
        if world.intersect(&shadow, &shadow_interval).is_none() {
            direct += albedo * light.emit * cos;
        }
    }

    emitted + direct
}
```

- [ ] **Step 4: Switch `sample_pixel` to the direct integrator**

In `src/camera/camera.rs`, `sample_pixel` currently ends with:

```rust
let ray = self.get_ray(i, j, sample_index, rng);
self.ray_color(&ray, self.max_depth, world, rng)
```

Change the last line to:

```rust
let ray = self.get_ray(i, j, sample_index, rng);
self.ray_color_direct(&ray, world, rng)
```

Leave `Camera::ray_color` and the offline `Camera::render` loop untouched — `render` keeps calling `ray_color`, which both preserves the path tracer for the next step and keeps `ray_color` referenced (no dead-code warning).

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test direct_tests 2>&1 | tail -20`
Expected: `occluded_point_is_darker_than_lit_point` PASS.

- [ ] **Step 6: Run the full suite and confirm a clean build**

Run: `cargo test 2>&1 | grep -E "test result:|error" ; cargo build 2>&1 | tail -3`
Expected: all tests pass; build finishes with no new warnings.

- [ ] **Step 7: Commit**

```bash
git add src/camera/camera.rs
git commit -m "feat: direct light sampling integrator (shadow rays, no PDF)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** `Intersect::sample_point` + leaf overrides (Task 1); composites/transforms incl. "collection of planes/triangles" via `IntersectGroup`/`BVH` and transform forwarding (Task 2); `Light { geom, emit }` and `lights` on `IntersectGroup` populated by `build_world` scanning `DiffuseLight` (Task 3); `ray_color_direct` with miss→background, emitted, scatter-derived albedo, per-light random `sample_point`, `cos<=0` skip, occlusion on `(0.001, dist-0.001)`, `albedo*emit*cos`, `sample_pixel` switch (Task 4); uniform/no-front-facing and no-PDF documented as constraints; tests for surface sampling, light collection, and occlusion all present. All spec points covered.
- **Placeholder scan:** none — every code/command step is concrete. (Step 5 of Task 2 notes a conditional import to keep the build warning-free; that is a contingency, not a placeholder.)
- **Type consistency:** `sample_point(&self, rng: &mut SmallRng) -> Point3` is identical across the trait and all overrides; `Light { geom: Arc<dyn Intersect>, emit: Color }` and `lights: Vec<Light>` match between definition (Task 3) and use (Task 4); `ray_color_direct(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color` matches between definition and the `sample_pixel`/test call sites.
