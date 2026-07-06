use crate::{interval::Interval, ray::*, vec3::Vec3};

pub trait Intersect: Send + Sync {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<GeoHit>;

    fn bounding_box(&self) -> &AABB;

    fn center(&self) -> Vec3;

    /// Whether *any* surface blocks `ray` within `ray_t` — an occlusion / shadow
    /// query. It needs only the boolean, never the nearest hit or its shading
    /// data, so the default answers it via a closest-hit but acceleration
    /// structures override it to early-exit on the first blocker.
    fn occluded(&self, ray: &Ray, ray_t: &Interval) -> bool {
        self.intersect(ray, ray_t).is_some()
    }
}
