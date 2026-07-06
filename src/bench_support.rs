//! Shared scaffolding for the BVH benchmark harness — used by both `benches/bvh.rs`
//! and `examples/bvh_stats.rs`. It lives in the library because Cargo bench and
//! example targets are separate compilation units that can't `use` each other's
//! modules; keeping the mesh set, orientations, and ray-grid generator here is
//! what makes the timing bench and the counter diagnostic measure the *same*
//! thing (so the counts explain the timings). Native-only — it reads OBJ files
//! from disk, which `std::fs` can't do on wasm.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex, OnceLock};

use crate::geometry::ObjData;
use crate::ray::{Ray, AABB};
use crate::scene::MeshData;
use crate::vec3::Vec3;

/// Meshes benched, smallest first, so the size sweep is visible in the report.
pub const MESHES: [&str; 3] = ["teapot", "bunny", "dragon"];

/// Ray-grid resolution per orientation (`RES × RES` parallel rays).
pub const RES: usize = 64;

/// Load `assets/objs/<name>.obj` as a shared [`MeshData`], parsed at most once per
/// name for the whole process. The manifest-relative path is needed because
/// bench/example targets run from the crate root while `ObjData::load` is
/// cwd-relative; parsing is memoized so a big mesh isn't re-read across the
/// several benches that use it.
pub fn load_mesh(name: &'static str) -> Arc<MeshData> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<MeshData>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().unwrap();
    cache
        .entry(name)
        .or_insert_with(|| {
            let path = format!("{}/assets/objs/{name}.obj", env!("CARGO_MANIFEST_DIR"));
            let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
            let (verts, faces, uvs) = ObjData::parse(&raw).mesh_data();
            Arc::new(MeshData { verts, faces, uvs })
        })
        .clone()
}

/// The named camera orientations: the three principal axes (one is face-on down
/// the mesh's long axis) plus two obliques. Each is a unit view direction, so an
/// ordering/split-axis regression on an anisotropic mesh shows up as one
/// orientation costing far more than the others.
pub fn orientations() -> [(&'static str, Vec3); 5] {
    let d = |x, y, z| Vec3::new(x, y, z).unit();
    [
        ("x", d(1.0, 0.0, 0.0)),
        ("y", d(0.0, 1.0, 0.0)),
        ("z", d(0.0, 0.0, 1.0)),
        ("diag", d(1.0, 1.0, 1.0)),
        ("oblique", d(1.0, 0.3, 2.0)),
    ]
}

/// A `RES × RES` grid of parallel rays travelling along `dir`, spanning the whole
/// bounding sphere of `bbox` — so the batch is a deterministic mix of hits
/// (through the silhouette) and misses (the corners), independent of orientation.
pub fn orientation_rays(bbox: &AABB, dir: Vec3) -> Vec<Ray> {
    let center = bbox.center();
    let radius = (bbox.max_vec() - bbox.min_vec()).length() * 0.5;

    // An orthonormal frame (u, v) spanning the plane perpendicular to `dir`.
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
