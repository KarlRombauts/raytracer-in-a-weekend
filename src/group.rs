use crate::interval::Interval;
use crate::ray::*;
use crate::vec3::Vec3;
use std::sync::Arc;

/// A plain list of hittables, accelerated by a bounding box, used as a piece of
/// geometry *inside* an object — e.g. the six quads of a box, or a loaded mesh.
/// It is itself an [`Intersect`], so it nests inside transforms and BVHs.
///
/// This is deliberately *not* the top-level scene container: that is
/// [`World`](crate::world::World), which additionally owns the lights and the
/// sky and attaches materials at the hit.
pub struct IntersectGroup {
    pub objects: Vec<Arc<dyn Intersect>>,
    bbox: AABB,
}

impl IntersectGroup {
    pub fn new() -> Self {
        IntersectGroup {
            objects: Vec::new(),
            bbox: AABB::EMPTY,
        }
    }

    pub fn add(&mut self, object: Arc<dyn Intersect>) -> &mut Self {
        self.bbox = AABB::from_boxes(&self.bbox, object.bounding_box());
        self.objects.push(object);
        self
    }

    pub fn from_object(object: Arc<dyn Intersect>) -> Self {
        let mut group = IntersectGroup::new();
        group.add(object);
        group
    }

    pub fn clear(&mut self) {
        self.objects.clear();
    }
}

impl Intersect for IntersectGroup {
    fn center(&self) -> Vec3 {
        self.bbox.center()
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<GeoHit> {
        let mut closest_hit: Option<GeoHit> = None;
        let mut closest_t = ray_t.max;

        for object in &self.objects {
            if let Some(hit_record) = object.intersect(ray, &Interval::new(ray_t.min, closest_t)) {
                closest_t = hit_record.t;
                closest_hit = Some(hit_record);
            }
        }

        closest_hit
    }
}
