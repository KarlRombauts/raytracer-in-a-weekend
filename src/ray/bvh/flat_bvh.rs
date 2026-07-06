use crate::interval::Interval;
use crate::vec3::Vec3;
use core::f32;

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
    },
    Inner {
        left: Box<BuildNode>,
        right: Box<BuildNode>,
        bounds: Bounds,
        axis: u8,
    },
}

/// Leaf flag — the top bit of [`BVHFlatNode::meta`]. Set on leaves (whose low
/// bits hold the primitive count), clear on interior nodes (whose low bits hold
/// the split axis). Primitive counts and axes are tiny, so they never reach it.
const LEAF_BIT: u32 = 1 << 31;

/// A flattened BVH node, packed to 32 bytes so two fit in a 64-byte cache line.
///
/// The left child of an interior node is always the next node in the array
/// (`idx + 1`): [`flatten`] emits the whole left subtree immediately after the
/// parent, so the left index is redundant and derived, not stored. Only the right
/// child (interior) or first-primitive index (leaf) needs a slot — `offset`.
/// `meta` packs the leaf flag with either the split axis or the primitive count.
struct BVHFlatNode {
    aabb: AABB,
    /// Interior: right-child index. Leaf: first-primitive index.
    offset: u32,
    /// Bit 31 = leaf. Interior: low bits = split axis (0/1/2). Leaf: low 31 bits =
    /// primitive count.
    meta: u32,
}

impl BVHFlatNode {
    /// An interior node with the given bounds, right-child index, and split axis
    /// (0/1/2). Its left child is implicit — the next node in the array.
    fn interior(aabb: AABB, right: u32, axis: u8) -> Self {
        BVHFlatNode { aabb, offset: right, meta: axis as u32 }
    }

    /// A leaf over `count` primitives starting at `first`.
    fn leaf(aabb: AABB, first: u32, count: u32) -> Self {
        debug_assert!(count < LEAF_BIT, "leaf primitive count too large to pack");
        BVHFlatNode { aabb, offset: first, meta: LEAF_BIT | count }
    }

    fn is_leaf(&self) -> bool {
        self.meta & LEAF_BIT != 0
    }

    /// Right-child index (interior nodes only; the left child is `idx + 1`).
    fn right_child(&self) -> usize {
        self.offset as usize
    }

    /// Split axis 0/1/2 (interior nodes only).
    fn split_axis(&self) -> u32 {
        self.meta & !LEAF_BIT
    }

    /// The `[start, end)` primitive range (leaf nodes only).
    fn leaf_range(&self) -> (usize, usize) {
        let start = self.offset as usize;
        let count = (self.meta & !LEAF_BIT) as usize;
        (start, start + count)
    }
}

impl Default for BVHFlatNode {
    fn default() -> Self {
        // An empty leaf: the single node of an empty BVH (tests its empty AABB,
        // iterates no primitives, misses cleanly), and a safe placeholder for a
        // slot `flatten` is about to overwrite.
        BVHFlatNode::leaf(AABB::EMPTY, 0, 0)
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

        // 2. Flatten into the contiguous node array the traversal walks. Each
        //    interior node's left child is the next node in the array, so only the
        //    right child index is stored (see `flatten` / `BVHFlatNode`).
        let mut nodes = Vec::with_capacity(2 * primitives.len().max(1));
        flatten(&root, &mut nodes);

        BVH { primitives, nodes }
    }

    /// Early-exit occlusion traversal: `true` as soon as any primitive is hit
    /// within `ray_t`. Because any blocker ends the query, it needs no
    /// front-to-back ordering and no `curr_max` tightening, and it builds no hit
    /// record — so it is simpler and cheaper than [`closest_hit`](Self::closest_hit).
    /// Leaves defer to each primitive's own `occluded`, so a two-level BVH
    /// early-exits through an object's inner BVH too.
    fn any_hit(&self, ray: &Ray, ray_t: &Interval) -> bool {
        const STACK_SIZE: usize = 64;
        let mut stack: [usize; STACK_SIZE] = [0; STACK_SIZE];
        let mut top: usize = 0;
        stack[top] = 0;
        top += 1;

        let nodes = &self.nodes;
        let prims = &self.primitives;

        while top > 0 {
            top -= 1;
            let node_idx = stack[top];
            let node = &nodes[node_idx];
            #[cfg(feature = "bvh-stats")]
            super::stats::count_box();
            if !node.aabb.intersect(ray, ray_t) {
                continue;
            }

            if node.is_leaf() {
                let (start, end) = node.leaf_range();
                for prim in &prims[start..end] {
                    #[cfg(feature = "bvh-stats")]
                    super::stats::count_primitive();
                    if prim.occluded(ray, ray_t) {
                        return true;
                    }
                }
            } else {
                // Any hit ends the query, so child order doesn't matter.
                stack[top] = node_idx + 1;
                top += 1;
                stack[top] = node.right_child();
                top += 1;
            }
        }
        false
    }

    /// Closest hit along `ray` within `ray_t`, together with the primitive that
    /// produced it. The BVH's single closest-hit traversal: the [`Intersect`] impl
    /// calls it and drops the primitive, while a two-level BVH keeps it to resolve
    /// which object was struck (the bare [`GeoHit`] carries no identity). `None` on
    /// a miss.
    #[inline(always)]
    pub fn closest_hit(&self, ray: &Ray, ray_t: &Interval) -> Option<(GeoHit, &T)> {
        const STACK_SIZE: usize = 64;
        let mut stack: [usize; STACK_SIZE] = [0; STACK_SIZE];
        let mut top: usize = 0;
        stack[top] = 0;
        top += 1;

        let mut closest: Option<(GeoHit, &T)> = None;
        let mut curr_max = ray_t.max;
        let min_t = ray_t.min;
        let dirs = &ray.direction;
        let nodes = &self.nodes;
        let prims = &self.primitives;

        while top > 0 {
            top -= 1;
            let node_idx = stack[top];
            let node = &nodes[node_idx];
            #[cfg(feature = "bvh-stats")]
            super::stats::count_box();
            if !node.aabb.intersect(ray, &Interval { min: min_t, max: curr_max }) {
                continue;
            }

            if node.is_leaf() {
                let (start, end) = node.leaf_range();
                for prim in &prims[start..end] {
                    #[cfg(feature = "bvh-stats")]
                    super::stats::count_primitive();
                    if let Some(hit) = prim.intersect(ray, &Interval { min: min_t, max: curr_max }) {
                        curr_max = hit.t;
                        closest = Some((hit, prim));
                    }
                }
            } else {
                // Interior nodes always have both children: the left is the next
                // node (idx + 1), the right is stored. Push the farther child first
                // so the nearer is popped and tested first (front-to-back).
                let axis = node.split_axis();
                let left = node_idx + 1;
                let right = node.right_child();
                let (first, second) = if dirs[axis] >= 0.0 { (right, left) } else { (left, right) };
                stack[top] = first;
                stack[top + 1] = second;
                top += 2;
            }
        }

        closest
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
        BuildNode::Leaf { first, count, bounds } => {
            nodes[idx] = BVHFlatNode::leaf(bounds.to_aabb(), *first, *count);
        }
        BuildNode::Inner { left, right, bounds, axis } => {
            // Emit the left subtree first, so the left child is always `idx + 1`
            // and needn't be stored; then the right subtree, whose root index is.
            let l = flatten(left, nodes);
            debug_assert_eq!(l, idx as u32 + 1, "left child must be the next node");
            let r = flatten(right, nodes);
            nodes[idx] = BVHFlatNode::interior(bounds.to_aabb(), r, *axis);
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
        // The same traversal identity-carrying callers use; the primitive is
        // dropped here (and optimizes away when this inlines).
        self.closest_hit(ray, ray_t).map(|(hit, _)| hit)
    }

    fn occluded(&self, ray: &Ray, ray_t: &Interval) -> bool {
        self.any_hit(ray, ray_t)
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::geometry::Triangle;
    use crate::vec3::Point3;

    #[test]
    fn flat_node_is_two_per_cache_line() {
        // 32 bytes so two nodes fit in one 64-byte cache line — the point of the
        // compaction. The AABB is 24 bytes; the child/primitive/axis/leaf data
        // packs into the remaining 8.
        assert_eq!(std::mem::size_of::<BVHFlatNode>(), 32);
    }

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

    #[test]
    fn closest_hit_reports_the_winning_primitive() {
        use crate::geometry::Sphere;
        use crate::interval::Interval;
        use crate::ray::Ray;
        use crate::vec3::{Point3, Vec3};

        // Three unit spheres strung along +x at x = 0, 4, 8.
        let spheres = vec![
            Sphere::stationary(Point3::new(0.0, 0.0, 0.0), 1.0),
            Sphere::stationary(Point3::new(4.0, 0.0, 0.0), 1.0),
            Sphere::stationary(Point3::new(8.0, 0.0, 0.0), 1.0),
        ];
        let bvh = BVH::build(spheres);
        let ray_t = Interval::new(0.001, f32::INFINITY);

        // From -x the first sphere (x=0) is closest; from +x the last (x=8) is.
        let from_left = Ray::new(Point3::new(-10.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let (hit, winner) = bvh.closest_hit(&from_left, &ray_t).expect("hits the near sphere");
        assert_eq!(winner.center(), Point3::new(0.0, 0.0, 0.0), "nearest from the left");
        // The winner-returning query agrees with the bare-GeoHit path on distance.
        assert_eq!(hit.t, bvh.intersect(&from_left, &ray_t).unwrap().t);

        let from_right = Ray::new(Point3::new(20.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0));
        let (_, winner) = bvh.closest_hit(&from_right, &ray_t).expect("hits the near sphere");
        assert_eq!(winner.center(), Point3::new(8.0, 0.0, 0.0), "nearest from the right");

        // A ray that clears every sphere reports no winner.
        let miss = Ray::new(Point3::new(-10.0, 10.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert!(bvh.closest_hit(&miss, &ray_t).is_none());
    }
}

#[cfg(test)]
mod occlusion_tests {
    use super::*;
    use crate::geometry::Sphere;
    use crate::interval::Interval;
    use crate::ray::Ray;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    fn spheres() -> BVH<Sphere> {
        let centers = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 1.0, -2.0),
            Point3::new(-2.5, -1.0, 1.5),
            Point3::new(1.0, -2.0, 3.0),
            Point3::new(-1.5, 2.0, -3.0),
        ];
        BVH::build(centers.iter().map(|c| Sphere::stationary(*c, 0.8)).collect())
    }

    #[test]
    fn occluded_agrees_with_closest_hit() {
        // An occlusion query is exactly "does any surface intersect within the
        // interval" — the same fact `intersect(...).is_some()` reports. Over a
        // battery of random rays *and* random distance bounds (so the t_max cutoff
        // is exercised), the fast boolean must agree with the closest-hit truth.
        let bvh = spheres();
        let mut rng = SmallRng::seed_from_u64(0x0CC1);
        for _ in 0..1000 {
            let origin = Point3::new(
                rng.random_range(-8.0..8.0),
                rng.random_range(-8.0..8.0),
                rng.random_range(-8.0..8.0),
            );
            let target = Point3::new(
                rng.random_range(-3.0..3.0),
                rng.random_range(-3.0..3.0),
                rng.random_range(-3.0..3.0),
            );
            let ray = Ray::new(origin, target - origin);
            let ti = Interval::new(0.001, rng.random_range(0.5..1.5));
            assert_eq!(
                bvh.occluded(&ray, &ti),
                bvh.intersect(&ray, &ti).is_some(),
                "occluded disagreed with closest-hit for interval {ti:?}"
            );
        }
    }
}

#[cfg(all(test, feature = "bvh-stats"))]
mod stats_tests {
    use super::*;
    use crate::geometry::Sphere;
    use crate::interval::Interval;
    use crate::ray::bvh::stats;
    use crate::ray::Ray;
    use crate::vec3::{Point3, Vec3};

    // Three unit spheres strung along +x at x = 0, 4, 8.
    fn three_spheres() -> BVH<Sphere> {
        BVH::build(vec![
            Sphere::stationary(Point3::new(0.0, 0.0, 0.0), 1.0),
            Sphere::stationary(Point3::new(4.0, 0.0, 0.0), 1.0),
            Sphere::stationary(Point3::new(8.0, 0.0, 0.0), 1.0),
        ])
    }

    #[test]
    fn counts_primitive_tests_during_traversal() {
        let bvh = three_spheres();
        let ray = Ray::new(Point3::new(-10.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let ti = Interval::new(0.001, f32::INFINITY);

        stats::reset();
        let _ = bvh.closest_hit(&ray, &ti);
        let (_boxes, prims) = stats::snapshot();
        // The ray hit a sphere, so at least one primitive was tested — and never
        // more than the three that exist.
        assert!(prims >= 1, "expected at least one primitive test, got {prims}");
        assert!(prims <= 3, "expected at most three primitive tests, got {prims}");
    }

    #[test]
    fn counts_node_box_tests_during_traversal() {
        let bvh = three_spheres();
        let ray = Ray::new(Point3::new(-10.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let ti = Interval::new(0.001, f32::INFINITY);

        stats::reset();
        let _ = bvh.closest_hit(&ray, &ti);
        let (boxes, _prims) = stats::snapshot();
        // Traversal always tests at least the root node's bounding box.
        assert!(boxes >= 1, "expected at least the root box test, got {boxes}");
    }

    // A fixed battery of rays around the cluster — deterministic, no rng.
    fn ray_battery() -> Vec<Ray> {
        let mut rays = Vec::new();
        for k in 0..64 {
            let a = k as f32 * 0.19;
            let origin = Point3::new(12.0 * a.cos(), 3.0 * (a * 0.5).sin(), 12.0 * a.sin());
            let target = Point3::new(4.0, 0.0, 0.0); // cluster centre
            rays.push(Ray::new(origin, target - origin));
        }
        rays
    }

    fn counts_for(bvh: &BVH<Sphere>, rays: &[Ray]) -> (u64, u64) {
        let ti = Interval::new(0.001, f32::INFINITY);
        stats::reset();
        for ray in rays {
            let _ = bvh.closest_hit(ray, &ti);
        }
        stats::snapshot()
    }

    #[test]
    fn counts_are_deterministic_across_runs() {
        let bvh = three_spheres();
        let rays = ray_battery();
        // The counter is a pure function of (tree, rays): two identical runs must
        // agree exactly — nothing about traversal work is nondeterministic.
        assert_eq!(counts_for(&bvh, &rays), counts_for(&bvh, &rays));
    }

    // `n` unit-ish spheres packed along the same x ∈ [0, 8] segment.
    fn packed_spheres(n: usize) -> BVH<Sphere> {
        let denom = (n as f32 - 1.0).max(1.0);
        let v = (0..n)
            .map(|i| Sphere::stationary(Point3::new(8.0 * i as f32 / denom, 0.0, 0.0), 0.4))
            .collect();
        BVH::build(v)
    }

    #[test]
    fn a_denser_cluster_records_more_primitive_tests() {
        let rays = ray_battery();
        let (_, sparse) = counts_for(&packed_spheres(4), &rays);
        let (_, dense) = counts_for(&packed_spheres(48), &rays);
        // More primitives in the same volume ⇒ a ray crossing the cluster tests
        // more of them. (The BVH makes this grow sub-linearly, not not-at-all.)
        assert!(
            dense > sparse,
            "denser cluster should record more primitive tests: dense={dense} sparse={sparse}"
        );
    }
}
