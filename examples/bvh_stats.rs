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

use std::fs;

use raytracer_in_a_weekend::bvh_stats;
use raytracer_in_a_weekend::geometry::ObjData;
use raytracer_in_a_weekend::interval::Interval;
use raytracer_in_a_weekend::ray::{Ray, AABB};
use raytracer_in_a_weekend::scene::MeshData;
use raytracer_in_a_weekend::vec3::Vec3;

const MESHES: [&str; 3] = ["teapot", "bunny", "dragon"];
const RES: usize = 64;

fn load_mesh(name: &str) -> MeshData {
    let path = format!("{}/assets/objs/{name}.obj", env!("CARGO_MANIFEST_DIR"));
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (verts, faces, uvs) = ObjData::parse(&raw).mesh_data();
    MeshData { verts, faces, uvs }
}

fn orientations() -> [(&'static str, Vec3); 5] {
    let d = |x, y, z| Vec3::new(x, y, z).unit();
    [
        ("x", d(1.0, 0.0, 0.0)),
        ("y", d(0.0, 1.0, 0.0)),
        ("z", d(0.0, 0.0, 1.0)),
        ("diag", d(1.0, 1.0, 1.0)),
        ("oblique", d(1.0, 0.3, 2.0)),
    ]
}

fn orientation_rays(bbox: &AABB, dir: Vec3) -> Vec<Ray> {
    let center = bbox.center();
    let radius = (bbox.max_vec() - bbox.min_vec()).length() * 0.5;
    let up = if dir.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let u = dir.cross(&up).unit();
    let v = dir.cross(&u);
    let start = center - dir * (radius * 2.0);
    let mut rays = Vec::with_capacity(RES * RES);
    for gy in 0..RES {
        for gx in 0..RES {
            let su = (gx as f32 / (RES - 1) as f32 - 0.5) * 2.0 * radius;
            let sv = (gy as f32 / (RES - 1) as f32 - 0.5) * 2.0 * radius;
            rays.push(Ray::new(start + u * su + v * sv, dir));
        }
    }
    rays
}

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
