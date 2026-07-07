//! Slim oversized `.scene` files by decimating their embedded meshes.
//!
//! The sample scenes baked in full-resolution meshes (a 2.35M-tri dragon, etc.).
//! This is a two-phase pipeline around an external Blender decimation step:
//!
//!   1. cargo run --release --example scene_slim -- extract <dir> assets/scenes/*.scene
//!   2. blender --background --python tools/decimate.py -- <dir> 150000
//!   3. cargo run --release --example scene_slim -- rebake <dir>
//!
//! `extract` dumps every mesh over the target to `<dir>/<stem>__<i>.obj` + a
//! manifest; Blender writes `<...>.dec.obj`; `rebake` swaps the decimated geometry
//! back into each scene (keeping transforms, materials, camera, and the preview)
//! and rewrites the `.scene` in place, reporting the size drop.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use raytracer_in_a_weekend::geometry::ObjData;
use raytracer_in_a_weekend::scene::{MeshData, Shape};
use raytracer_in_a_weekend::scene_file;
use raytracer_in_a_weekend::vec3::Vec3;

const TARGET: usize = 150_000;

fn stem(path: &str) -> String {
    Path::new(path).file_stem().unwrap().to_string_lossy().into_owned()
}

fn bbox(verts: &[Vec3]) -> (Vec3, Vec3) {
    let mut lo = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut hi = -lo;
    for v in verts {
        lo = Vec3::new(lo.x.min(v.x), lo.y.min(v.y), lo.z.min(v.z));
        hi = Vec3::new(hi.x.max(v.x), hi.y.max(v.y), hi.z.max(v.z));
    }
    (lo, hi)
}

fn write_obj(path: &str, verts: &[Vec3], faces: &[[u32; 3]]) {
    let mut w = BufWriter::new(File::create(path).unwrap());
    for v in verts {
        writeln!(w, "v {} {} {}", v.x, v.y, v.z).unwrap();
    }
    for f in faces {
        // OBJ is 1-indexed.
        writeln!(w, "f {} {} {}", f[0] + 1, f[1] + 1, f[2] + 1).unwrap();
    }
}

fn extract(dir: &str, scenes: &[String]) {
    fs::create_dir_all(dir).unwrap();
    let mut manifest = String::new();
    for scene_path in scenes {
        let loaded = scene_file::decode(&fs::read(scene_path).unwrap()).unwrap();
        for (i, obj) in loaded.scene.objects.iter().enumerate() {
            if let Shape::Mesh { data, .. } = &obj.shape {
                // Only decimate untextured meshes over the target (UV meshes would
                // lose their per-triangle UVs through the OBJ round-trip).
                if data.faces.len() > TARGET && data.uvs.is_empty() {
                    let obj_path = format!("{dir}/{}__{i}.obj", stem(scene_path));
                    write_obj(&obj_path, &data.verts, &data.faces);
                    manifest.push_str(&format!(
                        "{scene_path}\t{i}\t{}\t{}\t{obj_path}\n",
                        obj.name, data.faces.len()
                    ));
                    println!("extracted {} '{}' ({} tris)", stem(scene_path), obj.name, data.faces.len());
                }
            }
        }
    }
    fs::write(format!("{dir}/manifest.tsv"), &manifest).unwrap();
    println!("\nmanifest: {dir}/manifest.tsv — run the Blender decimation next.");
}

fn rebake(dir: &str) {
    let manifest = fs::read_to_string(format!("{dir}/manifest.tsv")).unwrap();
    // scene_path -> [(object_index, decimated_obj_path)]
    let mut by_scene: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    for line in manifest.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (scene_path, i, obj_path) = (f[0].to_string(), f[1].parse::<usize>().unwrap(), f[4]);
        let dec = obj_path.strip_suffix(".obj").unwrap().to_string() + ".dec.obj";
        by_scene.entry(scene_path).or_default().push((i, dec));
    }

    for (scene_path, edits) in by_scene {
        let bytes = fs::read(&scene_path).unwrap();
        let old_mb = bytes.len() as f64 / 1_048_576.0;
        let loaded = scene_file::decode(&bytes).unwrap();
        let mut scene = loaded.scene;
        for (i, dec_path) in edits {
            // Original bbox (still present at objects[i]) for a sanity check.
            let orig_bbox = match &scene.objects[i].shape {
                Shape::Mesh { data, .. } => bbox(&data.verts),
                _ => panic!("object {i} in {scene_path} is not a mesh"),
            };
            let (verts, faces, _uvs) = ObjData::parse(&fs::read_to_string(&dec_path).unwrap()).mesh_data();
            let new_bbox = bbox(&verts);
            let drift = (new_bbox.0 - orig_bbox.0).length() + (new_bbox.1 - orig_bbox.1).length();
            let extent = (orig_bbox.1 - orig_bbox.0).length().max(1e-6);
            assert!(
                drift / extent < 0.05,
                "decimated mesh {dec_path} bbox drifted {:.1}% — axis mismatch?",
                100.0 * drift / extent
            );
            let data = MeshData { verts, faces, uvs: vec![] };
            let (object, render) = data.build();
            let name = scene.objects[i].name.clone();
            let tris = data.faces.len();
            scene.objects[i].shape = Shape::Mesh { data: Arc::new(data), object, render };
            println!("  {} '{name}' -> {tris} tris", stem(&scene_path));
        }
        let out = scene_file::encode(&scene, loaded.name.as_deref(), &loaded.preview);
        let new_mb = out.len() as f64 / 1_048_576.0;
        fs::write(&scene_path, &out).unwrap();
        println!("{scene_path}: {old_mb:.1} MB -> {new_mb:.1} MB\n");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("extract") => extract(&args[1], &args[2..]),
        Some("rebake") => rebake(&args[1]),
        _ => eprintln!("usage: scene_slim extract <dir> <scene...> | rebake <dir>"),
    }
}
