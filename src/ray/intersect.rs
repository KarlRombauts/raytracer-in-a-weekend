use crate::{interval::Interval, ray::*};

pub trait Intersect: Send + Sync {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord>;

    fn bounding_box(&self) -> &AABB;
}
