use std::{cmp::Ordering, sync::Arc};

use rand::Rng;

use crate::{group::IntersectGroup, interval::Interval, ray::*};

pub struct BVHNode {
    left: Arc<dyn Intersect>,
    right: Arc<dyn Intersect>,
    bbox: AABB,
}

impl BVHNode {
    pub fn new(left: Arc<dyn Intersect>, right: Arc<dyn Intersect>) -> Self {
        let box_left = left.bounding_box();
        let box_right = right.bounding_box();
        let bbox = AABB::from_boxes(&box_left, &box_right);
        BVHNode { left, right, bbox }
    }
    pub fn from_group(group: IntersectGroup) -> Arc<dyn Intersect> {
        let mut objects = group.objects.clone();
        Self::build_recursively(&mut objects)
    }

    pub fn build_recursively(objects: &mut [Arc<dyn Intersect>]) -> Arc<dyn Intersect> {
        let node = match objects.len() {
            0 => panic!("build_recursively called with no objects"),
            1 => {
                let leaf = objects[0].clone();
                return leaf;
            }
            2 => {
                let left = objects[0].clone();
                let right = objects[1].clone();
                BVHNode::new(left, right)
            }
            _ => {
                let axis: u32 = rand::rng().random_range(0..3);
                objects.sort_unstable_by(|a, b| Self::box_compare(a, b, axis));

                let mid = objects.len() / 2;
                let left_obj = BVHNode::build_recursively(&mut objects[..mid]);
                let right_obj = BVHNode::build_recursively(&mut objects[mid..]);
                BVHNode::new(left_obj, right_obj)
            }
        };

        Arc::new(node)
    }

    fn box_compare(a: &Arc<dyn Intersect>, b: &Arc<dyn Intersect>, axis_index: u32) -> Ordering {
        let a_min = a.bounding_box().axis_interval(axis_index).min;
        let b_min = b.bounding_box().axis_interval(axis_index).min;
        return a_min.partial_cmp(&b_min).unwrap_or(Ordering::Equal);
    }
}

impl Intersect for BVHNode {
    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord> {
        if !self.bbox.intersect(ray, ray_t) {
            return None;
        }

        if let Some(lrec) = self.left.intersect(ray, ray_t) {
            // shrink the search interval to [ray_t.min, lrec.t]
            let new_t = Interval::new(ray_t.min, lrec.t as f32);
            // only descend right if it *could* beat lrec.t
            if let Some(rrec) = self.right.intersect(ray, &new_t) {
                return Some(rrec);
            }
            return Some(lrec);
        }
        self.right.intersect(ray, ray_t)
    }
}
