use std::sync::Arc;

use crate::interval::Interval;
use crate::material::Material;
use crate::ray::{Intersect, HitRecord, Ray, AABB};
use crate::vec3::Vec3;

/// Wraps a prebuilt, material-agnostic intersect handle (a mesh BVH) and
/// overrides every hit's material with `material`. This lets a mesh's material
/// be changed by swapping one `Arc` at world-build time, with **no BVH rebuild**
/// — the spatial structure doesn't depend on the material.
///
/// An `Intersect` decorator, sibling to [`Translate`](super::Translate) /
/// [`Scale`](super::Scale) / [`Rotate`](super::Rotate): same "intersect like my
/// inner, change one thing on the way out" shape.
pub(crate) struct MaterialOverride {
    inner: Arc<dyn Intersect>,
    material: Arc<dyn Material>,
}

impl MaterialOverride {
    pub(crate) fn new(inner: Arc<dyn Intersect>, material: Arc<dyn Material>) -> Self {
        MaterialOverride { inner, material }
    }
}

impl Intersect for MaterialOverride {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let mut hit = self.inner.intersect(ray, ray_t)?;
        hit.material = &*self.material;
        Some(hit)
    }
    fn bounding_box(&self) -> &AABB {
        self.inner.bounding_box()
    }
    fn center(&self) -> Vec3 {
        self.inner.center()
    }
}
