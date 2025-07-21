use std::{cmp::Ordering, sync::Arc};

use rand::Rng;

use crate::{
    group::IntersectGroup,
    interval::Interval,
    ray::{HitRecord, Intersect, Ray, AABB},
};

pub struct BVHNode {
    left: Arc<dyn Intersect>,
    right: Arc<dyn Intersect>,
    bbox: AABB,
    split_axis: u32,
}

impl BVHNode {
    pub fn new(left: Arc<dyn Intersect>, right: Arc<dyn Intersect>, split_axis: u32) -> Self {
        let box_left = left.bounding_box();
        let box_right = right.bounding_box();
        let bbox = AABB::from_boxes(&box_left, &box_right);
        BVHNode {
            left,
            right,
            bbox,
            split_axis,
        }
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
                BVHNode::new(left, right, 0)
            }
            _ => {
                let axis = Self::choose_split_axis(&objects);
                objects.sort_unstable_by(|a, b| Self::box_compare(a, b, axis));

                let mid = objects.len() / 2;
                let left_obj = BVHNode::build_recursively(&mut objects[..mid]);
                let right_obj = BVHNode::build_recursively(&mut objects[mid..]);
                BVHNode::new(left_obj, right_obj, axis)
            }
        };

        Arc::new(node)
    }

    // Instead of random axis selection:
    fn choose_split_axis(objects: &[Arc<dyn Intersect>]) -> u32 {
        let mut mins = [f32::INFINITY; 3];
        let mut maxs = [f32::NEG_INFINITY; 3];

        // in one pass, build the bounding‑box of the whole set
        for obj in objects {
            let bbox = obj.bounding_box();
            for i in 0usize..3usize {
                let iv = bbox.axis_interval(i as u32);
                mins[i] = mins[i].min(iv.min);
                maxs[i] = maxs[i].max(iv.max);
            }
        }

        let mut best = 0;
        let mut best_span = -f32::INFINITY;
        for i in 0..3 {
            let span = maxs[i] - mins[i];
            if span > best_span {
                best_span = span;
                best = i;
            }
        }
        best as u32
    }

    fn box_compare(a: &Arc<dyn Intersect>, b: &Arc<dyn Intersect>, axis_index: u32) -> Ordering {
        let a_bbox = a.bounding_box();
        let b_bbox = b.bounding_box();
        let a_min = a_bbox.axis_interval(axis_index).min;
        let b_min = b_bbox.axis_interval(axis_index).min;
        a_min.partial_cmp(&b_min).unwrap_or(Ordering::Equal)
    }
}

impl Intersect for BVHNode {
    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    #[inline(always)]
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord> {
        // 1) First do the trivial reject on this node’s box:
        if !self.bbox.intersect(ray, ray_t) {
            return None;
        }

        // 2) Decide child order *without* any distance tests:
        //    We store the split axis (0=x,1=y,2=z) when we built the BVH;
        //    if the ray’s direction along that axis is positive,
        //    traverse left first, otherwise right first.
        let (first, second) = if ray.direction[self.split_axis] >= 0.0 {
            (&self.left, &self.right)
        } else {
            (&self.right, &self.left)
        };

        // 3) Recurse into the nearer child:
        if let Some(hit1) = first.intersect(ray, ray_t) {
            // tighten the t‐max so the other side can’t return something farther than hit1
            let mut nt = *ray_t;
            nt.max = hit1.t;
            // if the farther child has *any* closer hit, take it instead
            if let Some(hit2) = second.intersect(ray, &nt) {
                return Some(hit2);
            }
            return Some(hit1);
        }

        // 4) If the near child missed, just try the far child:
        second.intersect(ray, ray_t)
    }
}
