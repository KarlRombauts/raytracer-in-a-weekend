use crate::interval::Interval;
use crate::ray::*;
use crate::vec3::Vec3;
use std::sync::Arc;

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

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let mut closest_hit: Option<HitRecord> = None;
        let mut closest_t = ray_t.max;

        for object in &self.objects {
            if let Some(hit_record) = object.intersect(ray, &Interval::new(ray_t.min, closest_t)) {
                closest_t = hit_record.t as f32;
                closest_hit = Some(hit_record);
            }
        }

        closest_hit
    }
}
