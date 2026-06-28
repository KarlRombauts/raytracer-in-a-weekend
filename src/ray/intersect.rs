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
}
