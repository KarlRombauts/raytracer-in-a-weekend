use crate::{interval::Interval, ray::*, vec3::Vec3};

pub trait Intersect: Send + Sync {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<GeoHit>;

    fn bounding_box(&self) -> &AABB;

    fn center(&self) -> Vec3;
}
