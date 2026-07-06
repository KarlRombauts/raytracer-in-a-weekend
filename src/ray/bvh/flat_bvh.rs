use crate::interval::Interval;
use crate::vec3::Vec3;
use core::f32;
use std::fmt;

use crate::ray::{GeoHit, Intersect, Ray, AABB};

/// Number of buckets used per axis when evaluating split-plane candidates with
/// binned SAH. 12 is the usual sweet spot: enough resolution to find good
/// splits, few enough that the per-node bin sweep is negligible next to the
/// single primitive pass that fills the bins.
const BINS: usize = 12;

/// Subtrees with at least this many primitives are built on separate rayon
/// tasks (via `rayon::join`). Below it the join overhead outweighs the work, so
/// we recurse inline on the current thread.
const PARALLEL_THRESHOLD: usize = 8_192;

/// Hard depth cap. Balanced binned splits put a multi-million-triangle mesh at
/// ~22 deep, but degenerate (e.g. fully coplanar) inputs could otherwise recurse
/// past the fixed 64-entry traversal stack — force a leaf before that happens.
const MAX_DEPTH: u8 = 60;

/// Lightweight min/max bounds used only during construction. Unlike [`AABB`] it
/// skips the per-merge `pad_to_minimums` work, so binning millions of triangles
/// stays a tight loop; the final padded `AABB` is produced once per node in
/// [`Bounds::to_aabb`].
#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    const EMPTY: Bounds = Bounds {
        min: Vec3 {
            x: f32::INFINITY,
            y: f32::INFINITY,
            z: f32::INFINITY,
        },
        max: Vec3 {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
            z: f32::NEG_INFINITY,
        },
    };

    #[inline(always)]
    fn grow_point(&mut self, p: Vec3) {
        self.min = Vec3::min(&self.min, &p);
        self.max = Vec3::max(&self.max, &p);
    }

    #[inline(always)]
    fn grow_aabb(&mut self, b: &AABB) {
        self.min = Vec3::min(&self.min, &b.min_vec());
        self.max = Vec3::max(&self.max, &b.max_vec());
    }

    #[inline(always)]
    fn merge(&mut self, o: &Bounds) {
        self.min = Vec3::min(&self.min, &o.min);
        self.max = Vec3::max(&self.max, &o.max);
    }

    /// Surface-area heuristic half-area. Returns 0 for an empty box so that an
    /// empty split side contributes nothing (and never a NaN) to the cost.
    #[inline(always)]
    fn area(&self) -> f32 {
        let e = self.max - self.min;
        if e.x < 0.0 || e.y < 0.0 || e.z < 0.0 {
            return 0.0;
        }
        e.x * e.y + e.y * e.z + e.z * e.x
    }

    fn to_aabb(&self) -> AABB {
        AABB::new(
            Interval::new(self.min.x, self.max.x),
            Interval::new(self.min.y, self.max.y),
            Interval::new(self.min.z, self.max.z),
        )
    }
}

/// Intermediate, pointer-linked node produced by the (parallel) recursive build.
/// It is flattened into the cache-friendly `Vec<BVHFlatNode>` exactly once, in a
/// single sequential DFS, after the whole tree exists.
enum BuildNode {
    Leaf {
        first: u32,
        count: u32,
        bounds: Bounds,
        depth: u8,
    },
    Inner {
        left: Box<BuildNode>,
        right: Box<BuildNode>,
        bounds: Bounds,
        axis: u8,
        depth: u8,
    },
}

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
    pub fn build(mut primitives: Vec<T>) -> Self {
        if primitives.is_empty() {
            return BVH {
                primitives,
                nodes: vec![BVHFlatNode::default()],
            };
        }

        // 1. Build the spatial tree recursively, splitting work across rayon
        //    tasks for large subtrees. Primitives are partitioned in place; each
        //    task owns a disjoint slice, so no synchronisation is needed.
        let root = build_node(&mut primitives, 0, 0);

        // 2. Flatten into the contiguous node array the traversal walks. A leaf
        //    is encoded as `left == right == 0` (index 0 is the root, so it can
        //    never be a real child), exactly as before.
        let mut nodes = Vec::with_capacity(2 * primitives.len().max(1));
        flatten(&root, &mut nodes);

        BVH { primitives, nodes }
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
    fn intersectBVH(&self, ray: &Ray, ray_t: &Interval) -> Option<GeoHit> {
        // Fixed-size stack (avoid Vec overhead)
        const STACK_SIZE: usize = 64;
        let mut stack: [usize; STACK_SIZE] = [0; STACK_SIZE];
        let mut top: usize = 0;
        // Push root
        stack[top] = 0;
        top += 1;

        let mut closest_hit: Option<GeoHit> = None;
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

/// Recursively build the BVH over `prims`, whose slice starts at global index
/// `offset` in the primitive array. Returns the subtree root. Large subtrees
/// fork their two children onto separate rayon tasks.
fn build_node<T: Intersect>(prims: &mut [T], offset: u32, depth: u8) -> BuildNode {
    // One pass: the node's full bounds (for its AABB) and the bounds of the
    // primitive *centroids* (the domain we bin over — tighter and split-friendly
    // than the geometry bounds).
    let mut node_bounds = Bounds::EMPTY;
    let mut centroid_bounds = Bounds::EMPTY;
    for p in prims.iter() {
        node_bounds.grow_aabb(p.bounding_box());
        centroid_bounds.grow_point(p.center());
    }

    let count = prims.len() as u32;
    let leaf = |bounds| BuildNode::Leaf {
        first: offset,
        count,
        bounds,
        depth,
    };

    if count <= 2 || depth >= MAX_DEPTH {
        return leaf(node_bounds);
    }

    let Some((axis, split_pos, split_cost)) = find_binned_split(prims, &centroid_bounds) else {
        return leaf(node_bounds);
    };

    // Stop if no split beats keeping every primitive in one leaf (the SAH "leaf
    // cost" = count × surface area). Same termination rule as the old code.
    let leaf_cost = count as f32 * node_bounds.area();
    if split_cost >= leaf_cost {
        return leaf(node_bounds);
    }

    let mid = partition(prims, axis, split_pos);
    if mid == 0 || mid == prims.len() {
        // Degenerate partition (all primitives landed on one side); keep a leaf.
        return leaf(node_bounds);
    }

    let (left_slice, right_slice) = prims.split_at_mut(mid);
    let right_offset = offset + mid as u32;
    let next_depth = depth + 1;

    let (left, right) = if count as usize >= PARALLEL_THRESHOLD {
        rayon::join(
            || build_node(left_slice, offset, next_depth),
            || build_node(right_slice, right_offset, next_depth),
        )
    } else {
        (
            build_node(left_slice, offset, next_depth),
            build_node(right_slice, right_offset, next_depth),
        )
    };

    BuildNode::Inner {
        left: Box::new(left),
        right: Box::new(right),
        bounds: node_bounds,
        axis: axis as u8,
        depth,
    }
}

/// Binned SAH: bucket every primitive once per axis, then sweep the buckets to
/// evaluate all `BINS - 1` candidate planes in O(BINS) rather than rescanning
/// the primitives per candidate. Returns the best `(axis, split_pos, cost)`, or
/// `None` if the centroids are coincident on every axis (nothing to split).
fn find_binned_split<T: Intersect>(prims: &[T], centroid_bounds: &Bounds) -> Option<(u32, f32, f32)> {
    let mut best: Option<(u32, f32, f32)> = None;

    for axis in 0..3u32 {
        let cmin = centroid_bounds.min[axis];
        let cmax = centroid_bounds.max[axis];
        if cmin >= cmax {
            continue; // no extent on this axis
        }
        let scale = BINS as f32 / (cmax - cmin);

        let mut bin_bounds = [Bounds::EMPTY; BINS];
        let mut bin_count = [0u32; BINS];
        for p in prims {
            let c = p.center()[axis];
            let b = (((c - cmin) * scale) as usize).min(BINS - 1);
            bin_count[b] += 1;
            bin_bounds[b].grow_aabb(p.bounding_box());
        }

        // Prefix sweep: cumulative count/area of bins [0..=i].
        let mut left_area = [0f32; BINS - 1];
        let mut left_count = [0u32; BINS - 1];
        let mut acc = Bounds::EMPTY;
        let mut acc_count = 0u32;
        for i in 0..BINS - 1 {
            acc_count += bin_count[i];
            acc.merge(&bin_bounds[i]);
            left_count[i] = acc_count;
            left_area[i] = acc.area();
        }

        // Suffix sweep: cumulative count/area of bins [i+1..], combined with the
        // cached prefix to score the plane between bin i and i+1.
        let mut acc = Bounds::EMPTY;
        let mut acc_count = 0u32;
        for i in (0..BINS - 1).rev() {
            acc_count += bin_count[i + 1];
            acc.merge(&bin_bounds[i + 1]);

            let lc = left_count[i];
            let rc = acc_count;
            if lc == 0 || rc == 0 {
                continue;
            }
            let cost = lc as f32 * left_area[i] + rc as f32 * acc.area();
            if best.map_or(true, |(_, _, bc)| cost < bc) {
                let split_pos = cmin + (i as f32 + 1.0) / scale;
                best = Some((axis, split_pos, cost));
            }
        }
    }

    best
}

/// Partition `prims` in place so that every primitive with centroid below
/// `split` on `axis` precedes the rest. Returns the number on the left side.
fn partition<T: Intersect>(prims: &mut [T], axis: u32, split: f32) -> usize {
    let mut i = 0;
    let mut j = prims.len();
    while i < j {
        if prims[i].center()[axis] < split {
            i += 1;
        } else {
            j -= 1;
            prims.swap(i, j);
        }
    }
    i
}

/// Flatten the pointer-linked build tree into the contiguous node array via a
/// single sequential DFS, patching child indices once they are known.
fn flatten(node: &BuildNode, nodes: &mut Vec<BVHFlatNode>) -> u32 {
    let idx = nodes.len();
    nodes.push(BVHFlatNode::default()); // reserve this slot

    match node {
        BuildNode::Leaf {
            first,
            count,
            bounds,
            depth,
        } => {
            nodes[idx] = BVHFlatNode {
                left: 0,
                right: 0,
                aabb: bounds.to_aabb(),
                first_primitive: *first,
                primitive_count: *count,
                split_axis: 0,
                depth: *depth,
            };
        }
        BuildNode::Inner {
            left,
            right,
            bounds,
            axis,
            depth,
        } => {
            let l = flatten(left, nodes);
            let r = flatten(right, nodes);
            nodes[idx] = BVHFlatNode {
                left: l,
                right: r,
                aabb: bounds.to_aabb(),
                first_primitive: 0,
                primitive_count: 0,
                split_axis: *axis,
                depth: *depth,
            };
        }
    }

    idx as u32
}

impl<T: Intersect> Intersect for BVH<T> {
    fn center(&self) -> Vec3 {
        return self.nodes[0].aabb.center();
    }

    fn bounding_box(&self) -> &AABB {
        return &self.nodes[0].aabb;
    }

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<GeoHit> {
        return self.intersectBVH(ray, ray_t);
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
    use crate::geometry::Triangle;
    use crate::vec3::Point3;

    #[test]
    fn bvh_hits_interior_of_a_flat_axis_aligned_face() {
        use crate::interval::Interval;
        use crate::ray::Ray;
        use crate::vec3::Vec3;
        // A tessellated, axis-aligned, perfectly flat face (the z = 0 plane),
        // exactly like one side of an imported cube. Every triangle here is
        // coplanar, so interior BVH nodes get a zero-thickness bounding box.
        let n = 16; // grid resolution
        let mut tris = Vec::new();
        for gy in 0..n {
            for gx in 0..n {
                let (x0, y0) = (gx as f32, gy as f32);
                let (x1, y1) = (x0 + 1.0, y0 + 1.0);
                let p = |x: f32, y: f32| Point3::new(x, y, 0.0);
                tris.push(Triangle::from_points(&p(x0, y0), &p(x1, y0), &p(x1, y1)));
                tris.push(Triangle::from_points(&p(x0, y0), &p(x1, y1), &p(x0, y1)));
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

}
