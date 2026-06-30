use crate::geometry::triangle;
use crate::interval::Interval;
use crate::vec3::{Point3, Vec3};
use core::f32;
use std::fmt;
use std::{path::Display, sync::Arc};

use crate::ray::{hit_record, BVHNode, HitRecord, Intersect, Ray, AABB};

pub struct BVHFlatNode {
    pub left: u32,
    pub right: u32,
    pub aabb: AABB,
    pub first_primitive: u32,
    pub primitive_count: u32,
    pub split_axis: u8,
    pub depth: u8,
}

impl BVHFlatNode {
    pub fn is_leaf(&self) -> bool {
        self.left == 0 && self.right == 0
    }

    pub fn get_primative_bounds(&self) -> (usize, usize) {
        let start = self.first_primitive as usize;
        let end = (self.first_primitive + self.primitive_count) as usize;
        (start, end)
    }

    pub fn get_split_axis(&self) -> (u32, f32) {
        let extent = self.aabb.max_vec() - self.aabb.min_vec();
        let mut axis = 0;
        if extent.y > extent.x {
            axis = 1;
        }
        if extent.z > extent[axis] {
            axis = 2;
        }
        let split_pos: f32 = self.aabb.min_vec()[axis] + extent[axis] * 0.5;
        return (axis, split_pos);
    }
}

impl Default for BVHFlatNode {
    fn default() -> Self {
        BVHFlatNode {
            left: 0,
            right: 0,
            depth: 0,
            aabb: AABB::EMPTY,
            first_primitive: 0,
            primitive_count: 0,
            split_axis: 0,
        }
    }
}

pub struct BVH<T: Intersect> {
    primitives: Vec<T>,
    nodes: Vec<BVHFlatNode>,
}

impl<T: Intersect> BVH<T> {
    pub fn build(primitives: Vec<T>) -> Self {
        let mut root = BVHFlatNode::default();
        root.primitive_count = primitives.len() as u32;

        let mut bvh = BVH {
            primitives,
            nodes: vec![root],
        };

        bvh.update_node_bounds(0);
        bvh.subdivide(0);
        bvh
    }

    fn evaluateNodeSAHCost(&self, node_idx: usize) -> f32 {
        let node = &self.nodes[node_idx];
        let area = node.aabb.area();
        node.primitive_count as f32 * area
    }

    fn evaluateSAH(&self, node_idx: usize, axis: u32, split: f32) -> f32 {
        let mut left_box = AABB::EMPTY;
        let mut right_box = AABB::EMPTY;
        let mut left_count = 0;
        let mut right_count = 0;

        let node = &self.nodes[node_idx];
        let (start, end) = node.get_primative_bounds();
        for primitive in &self.primitives[start..end] {
            if primitive.center()[axis] < split {
                left_count += 1;
                left_box = AABB::from_boxes(&left_box, primitive.bounding_box());
            } else {
                right_count += 1;
                right_box = AABB::from_boxes(&right_box, primitive.bounding_box());
            }
        }

        let cost = left_count as f32 * left_box.area() + right_count as f32 * right_box.area();

        if cost > 0. {
            return cost;
        } else {
            return f32::INFINITY;
        }
    }

    fn find_split(&self, node_idx: usize) -> (f32, u32, f32) {
        let node = &self.nodes[node_idx];
        let mut best_axis = 0;
        let mut best_split: f32 = 0.;
        let mut best_cost = f32::INFINITY;

        // let (start, end) = node.get_primative_bounds();

        let num_splits = 10.min(node.primitive_count);
        for axis in 0..3 {
            let bounds_min = node.aabb.min_vec()[axis];
            let bounds_max = node.aabb.max_vec()[axis];
            if bounds_min == bounds_max {
                continue;
            }

            let scale = (bounds_max - bounds_min) / num_splits as f32;

            for i in 1..num_splits {
                let candidate_split = bounds_min + i as f32 * scale;
                let cost = self.evaluateSAH(node_idx, axis, candidate_split);

                if cost < best_cost {
                    best_split = candidate_split;
                    best_axis = axis;
                    best_cost = cost;
                }
            }

            // for primitive in &self.primitives[start..end] {
            //     let candidate_split = primitive.center()[axis];
            //     let cost = self.evaluateSAH(node_idx, axis, candidate_split);
            //
            //     if cost < best_cost {
            //         best_split = candidate_split;
            //         best_axis = axis;
            //         best_cost = cost;
            //     }
            // }
        }

        return (best_cost, best_axis, best_split);
    }

    fn subdivide(&mut self, node_idx: usize) {
        let nodes_used = self.nodes.len() as u32;
        let (split_cost, axis, split_pos) = self.find_split(node_idx);

        if split_cost >= self.evaluateNodeSAHCost(node_idx) {
            return;
        }

        let node = &mut self.nodes[node_idx];

        node.split_axis = axis as u8;

        let (start, end) = node.get_primative_bounds();
        let mut i = start;
        let mut j = end;

        while i < j {
            if self.primitives[i].center()[axis] < split_pos {
                i += 1;
            } else {
                j -= 1;
                self.primitives.swap(i, j);
            }
        }

        let left_count = i - start;
        if left_count == 0 || left_count as u32 == node.primitive_count {
            return;
        }

        let left_node_idx = nodes_used;
        node.left = left_node_idx;
        let mut left_node = BVHFlatNode::default();
        left_node.first_primitive = start as u32;
        left_node.primitive_count = left_count as u32;
        left_node.depth = node.depth + 1;

        let right_node_idx = nodes_used + 1;
        node.right = right_node_idx;
        let mut right_node = BVHFlatNode::default();
        right_node.first_primitive = i as u32;
        right_node.primitive_count = node.primitive_count - left_count as u32;
        right_node.depth = node.depth + 1;

        node.primitive_count = 0;
        self.nodes.push(left_node);
        self.nodes.push(right_node);

        self.update_node_bounds(left_node_idx as usize);
        self.update_node_bounds(right_node_idx as usize);
        self.subdivide(left_node_idx as usize);
        self.subdivide(right_node_idx as usize);
    }

    fn update_node_bounds(&mut self, node_idx: usize) {
        let node = &mut self.nodes[node_idx];
        let (start, end) = node.get_primative_bounds();

        for primative in &mut self.primitives[start..end] {
            let bbox = primative.bounding_box();
            node.aabb = AABB::from_boxes(&node.aabb, &bbox);
        }
    }

    pub fn get_stats(&self) -> BVHStats {
        let leaves = self.nodes.iter().filter(|x| x.is_leaf());
        let depths = leaves.clone().map(|x| x.depth as u32);
        let prim_counts = leaves.clone().map(|x| x.primitive_count);
        let leaf_count = leaves.clone().count();

        BVHStats {
            total_prim_count: self.primitives.len() as u32,
            node_count: self.nodes.len() as u32,
            leaf_count: leaf_count as u32,
            max_depth: depths.clone().max().unwrap(),
            min_depth: depths.clone().min().unwrap(),
            avg_depth: depths.clone().sum::<u32>() as f32 / leaf_count as f32,

            max_prim_count: prim_counts.clone().max().unwrap(),
            min_prim_count: prim_counts.clone().min().unwrap(),
            avg_prim_count: prim_counts.clone().sum::<u32>() as f32 / leaf_count as f32,
        }
    }

    #[inline(always)]
    fn intersectBVH(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        // Fixed-size stack (avoid Vec overhead)
        const STACK_SIZE: usize = 64;
        let mut stack: [usize; STACK_SIZE] = [0; STACK_SIZE];
        let mut top: usize = 0;
        // Push root
        stack[top] = 0;
        top += 1;

        let mut closest_hit: Option<HitRecord> = None;
        // Track current maximum t for interval
        let mut curr_max = ray_t.max;
        let min_t = ray_t.min;
        let dirs = &ray.direction;
        let nodes = &self.nodes;
        let prims = &self.primitives;

        while top > 0 {
            top -= 1;
            let node_idx = stack[top];
            let node = &nodes[node_idx];
            // AABB test with tightened interval
            if !node.aabb.intersect(
                ray,
                &Interval {
                    min: min_t,
                    max: curr_max,
                },
            ) {
                continue;
            }

            if node.is_leaf() {
                let (start, end) = node.get_primative_bounds();
                // Test primitives in leaf
                for prim in &prims[start..end] {
                    if let Some(hit) = prim.intersect(
                        ray,
                        &Interval {
                            min: min_t,
                            max: curr_max,
                        },
                    ) {
                        curr_max = hit.t;
                        closest_hit = Some(hit);
                    }
                }
            } else {
                // Determine traversal order
                let axis = node.split_axis as usize;
                let left = node.left as usize;
                let right = node.right as usize;
                // push farther child first
                if dirs[axis as u32] >= 0.0 {
                    if right != 0 {
                        stack[top] = right;
                        top += 1;
                    }
                    if left != 0 {
                        stack[top] = left;
                        top += 1;
                    }
                } else {
                    if left != 0 {
                        stack[top] = left;
                        top += 1;
                    }
                    if right != 0 {
                        stack[top] = right;
                        top += 1;
                    }
                }
            }
        }

        closest_hit
    }
}

impl<T: Intersect> Intersect for BVH<T> {
    fn center(&self) -> Vec3 {
        return self.nodes[0].aabb.center();
    }

    fn bounding_box(&self) -> &AABB {
        return &self.nodes[0].aabb;
    }

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        return self.intersectBVH(ray, ray_t);
    }

    fn sample_point(&self, u: f32, v: f32) -> Point3 {
        if self.primitives.is_empty() {
            return self.center();
        }
        // `u` selects the primitive; its fractional part is reused as a fresh
        // uniform for the primitive's own surface sample.
        let n = self.primitives.len();
        let scaled = u * n as f32;
        let i = (scaled as usize).min(n - 1);
        let u2 = scaled - i as f32;
        self.primitives[i].sample_point(u2, v)
    }
}

pub struct BVHStats {
    total_prim_count: u32,
    node_count: u32,
    leaf_count: u32,
    max_depth: u32,
    min_depth: u32,
    avg_depth: f32,
    max_prim_count: u32,
    min_prim_count: u32,
    avg_prim_count: f32,
}

impl fmt::Display for BVHStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Triangles: {}", self.total_prim_count)?;
        writeln!(f, "Node Count: {}", self.node_count)?;
        writeln!(f, "Leaf Count: {}", self.leaf_count)?;
        writeln!(f, "Leaf Depth:")?;
        writeln!(f, "  - Min: {}", self.min_depth)?;
        writeln!(f, "  - Max: {}", self.max_depth)?;
        writeln!(f, "  - Mean: {:.3}", self.avg_depth)?;
        writeln!(f, "Leaf Tris:")?;
        writeln!(f, "  - Min: {}", self.min_prim_count)?;
        writeln!(f, "  - Max: {}", self.max_prim_count)?;
        writeln!(f, "  - Mean: {:.3}", self.avg_prim_count)
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Triangle;
    use crate::material::Lambertian;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};
    use std::sync::Arc;

    #[test]
    fn bvh_hits_interior_of_a_flat_axis_aligned_face() {
        use crate::interval::Interval;
        use crate::ray::Ray;
        use crate::vec3::Vec3;
        // A tessellated, axis-aligned, perfectly flat face (the z = 0 plane),
        // exactly like one side of an imported cube. Every triangle here is
        // coplanar, so interior BVH nodes get a zero-thickness bounding box.
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let n = 16; // grid resolution
        let mut tris = Vec::new();
        for gy in 0..n {
            for gx in 0..n {
                let (x0, y0) = (gx as f32, gy as f32);
                let (x1, y1) = (x0 + 1.0, y0 + 1.0);
                let p = |x: f32, y: f32| Point3::new(x, y, 0.0);
                tris.push(Triangle::from_points(&p(x0, y0), &p(x1, y0), &p(x1, y1), mat.clone()));
                tris.push(Triangle::from_points(&p(x0, y0), &p(x1, y1), &p(x0, y1), mat.clone()));
            }
        }
        let bvh = BVH::build(tris);

        // Fire a ray straight down (-z) into every cell. Sample off the cell
        // diagonal (0.3, 0.6) so each ray lands in the strict interior of one
        // triangle — this isolates the flat-AABB cull (the real holes bug) from
        // the measure-zero "ray exactly on a shared edge" case.
        let ray_t = Interval::new(0.001, f32::INFINITY);
        let mut misses = 0;
        for gy in 0..n {
            for gx in 0..n {
                let origin = Point3::new(gx as f32 + 0.3, gy as f32 + 0.6, 1.0);
                let ray = Ray::new(origin, Vec3::new(0.0, 0.0, -1.0));
                if bvh.intersect(&ray, &ray_t).is_none() {
                    misses += 1;
                }
            }
        }
        assert_eq!(misses, 0, "{misses} interior rays missed a flat face (holes)");
    }

    #[test]
    fn bvh_samples_point_on_a_primitive() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let t1 = Triangle::from_points(
            &Point3::new(0.0, 0.0, 0.0),
            &Point3::new(1.0, 0.0, 0.0),
            &Point3::new(0.0, 1.0, 0.0),
            mat.clone(),
        );
        let t2 = Triangle::from_points(
            &Point3::new(0.0, 0.0, 5.0),
            &Point3::new(1.0, 0.0, 5.0),
            &Point3::new(0.0, 1.0, 5.0),
            mat,
        );
        let bvh = BVH::build(vec![t1, t2]);
        let mut rng = SmallRng::seed_from_u64(6);
        for _ in 0..500 {
            let p = bvh.sample_point(rng.random::<f32>(), rng.random::<f32>());
            assert!(
                p.x >= -1e-4 && p.y >= -1e-4 && p.x + p.y <= 1.0 + 1e-4,
                "bad bary: {:?}",
                p
            );
            assert!(
                p.z.abs() < 1e-3 || (p.z - 5.0).abs() < 1e-3,
                "off both tris: z={}",
                p.z
            );
        }
    }
}
