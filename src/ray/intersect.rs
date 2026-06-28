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
