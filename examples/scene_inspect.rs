//! Diagnostic: break down what makes a `.scene` file large.
//!
//!   cargo run --release --example scene_inspect -- assets/scenes/*.scene
//!
//! Reports, per file: total size, the baked preview PNG size, the geometry+material
//! blob size (scene re-encoded with an empty preview), per-object embedded triangle
//! counts, and embedded texture-asset bytes — so it's obvious whether the bulk is
//! full-resolution meshes, embedded textures, or the preview image.

use raytracer_in_a_weekend::scene::{MaterialSpec, TextureSpec};
use raytracer_in_a_weekend::scene_file;
use std::fs;

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}

/// Sum the embedded image-asset bytes reachable from a material.
fn texture_bytes(m: &MaterialSpec) -> usize {
    let tex = |t: &TextureSpec| -> usize {
        match t {
            TextureSpec::Image { asset, .. } => asset.bytes.len(),
            _ => 0,
        }
    };
    match m {
        MaterialSpec::Lambertian { albedo } => tex(albedo),
        MaterialSpec::Glossy { albedo, .. } => tex(albedo),
        MaterialSpec::DiffuseLight { emit } => tex(emit),
        _ => 0,
    }
}

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: read error: {e}");
                continue;
            }
        };
        let loaded = match scene_file::decode(&bytes) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("{path}: decode error: {e}");
                continue;
            }
        };

        // Geometry+material blob = the scene re-encoded with no preview.
        let no_preview = scene_file::encode(&loaded.scene, loaded.name.as_deref(), &[]);

        let mut total_tris = 0usize;
        let mut total_tex_bytes = 0usize;
        let mut mesh_objs = 0usize;
        let mut per_obj = Vec::new();
        for o in &loaded.scene.objects {
            let tris = o.shape.render_mesh().positions.len() / 3;
            let tb = texture_bytes(&o.material);
            if tris > 1000 {
                mesh_objs += 1;
                per_obj.push((o.name.clone(), tris, tb));
            }
            total_tris += tris;
            total_tex_bytes += tb;
        }

        println!("\n=== {path} ===");
        println!(
            "  file: {:.1} MB   preview(PNG): {:.1} MB   geo+material: {:.1} MB",
            mb(bytes.len()),
            mb(loaded.preview.len()),
            mb(no_preview.len()),
        );
        println!(
            "  objects: {}   embedded triangles: {}   texture bytes: {:.1} MB",
            loaded.scene.objects.len(),
            total_tris,
            mb(total_tex_bytes),
        );
        // Heaviest meshes first.
        per_obj.sort_by_key(|(_, t, _)| std::cmp::Reverse(*t));
        for (name, tris, tb) in per_obj.iter().take(8) {
            println!("    mesh '{name}': {tris} tris, {:.1} MB textures", mb(*tb));
        }
        if mesh_objs > 8 {
            println!("    … and {} more mesh objects", mesh_objs - 8);
        }
    }
}
