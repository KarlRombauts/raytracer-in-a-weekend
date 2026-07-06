//! Deterministic BVH traversal-work diagnostic (see `.scratch/bvh-perf/PRD.md`).
//!
//! Prints the number of **node box tests** and **leaf primitive tests** the BVH
//! performs for a fixed ray grid through each mesh, per camera orientation. Unlike
//! the timing benches these numbers are hardware-independent, so they *explain* a
//! timing change: a bit-identical layout change leaves the counts unchanged (only
//! ns/ray moves), while a tree-quality change moves the counts.
//!
//!   cargo run --release --example bvh_stats --features bvh-stats
//!
//! Single-threaded by construction — the counters are thread-local.

use raytracer_in_a_weekend::bench_support::{load_mesh, orientation_rays, orientations, MESHES};
use raytracer_in_a_weekend::bvh_stats;
use raytracer_in_a_weekend::interval::Interval;

fn main() {
    let ti = Interval::new(0.001, f32::INFINITY);
    println!(
        "{:8} {:8} {:>7} {:>12} {:>12} {:>9} {:>9}",
        "mesh", "view", "rays", "box tests", "prim tests", "box/ray", "prim/ray"
    );
    for name in MESHES {
        let (bvh, _) = load_mesh(name).build();
        for (label, dir) in orientations() {
            let rays = orientation_rays(bvh.bounding_box(), dir);
            bvh_stats::reset();
            for ray in &rays {
                let _ = bvh.intersect(ray, &ti);
            }
            let (boxes, prims) = bvh_stats::snapshot();
            let n = rays.len() as f64;
            println!(
                "{name:8} {label:8} {:>7} {boxes:>12} {prims:>12} {:>9.1} {:>9.1}",
                rays.len(),
                boxes as f64 / n,
                prims as f64 / n,
            );
        }
    }
}
