//! Bundled sample scenes for the library (Home) screen, plus a pre-baked
//! thumbnail generator driven by `just thumbnails`.

use crate::camera::Camera;
use crate::render::ProgressiveRenderer;
use crate::scene::{build_world, MaterialSpec, ObjectSpec, Scene, Shape, TextureSpec, Transform};
use crate::vec3::{Point3, Vec3};

/// One bundled sample scene. `build` reconstructs the scene on demand; the
/// thumbnail PNG is embedded separately (see [`thumbnail_png`]) so this registry
/// compiles before any thumbnails exist.
pub struct Sample {
    pub name: &'static str,
    pub build: fn() -> Scene,
}

pub static SAMPLES: &[Sample] = &[Sample {
    name: "Cornell Box",
    build: crate::scenes::cornell_box::cornell_box,
}];

/// kebab-case filename slug: lowercase, non-alphanumerics collapsed to single
/// hyphens, trimmed. "Cornell Box" -> "cornell-box".
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut prev_hyphen = true; // also trims leading hyphens
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            out.push('-');
            prev_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Minimal starting scene for "New scene": a default camera plus a neutral
/// ground plane, so the viewport isn't empty and the user can Add Object.
pub fn new_scene() -> Scene {
    // Reuse the cornell camera config (valid dimensions/aspect) but replace the
    // objects with a single grey ground plane.
    let mut scene = crate::scenes::cornell_box::cornell_box();
    scene.objects = vec![ObjectSpec {
        name: "Ground".to_string(),
        shape: Shape::Quad {
            q: Point3::new(-5.0, 0.0, -5.0),
            u: Vec3::new(10.0, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, 10.0),
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(crate::color::Color::new(0.62, 0.62, 0.64)),
        },
        transform: Transform::identity(),
        hidden: false,
    }];
    scene
}

/// Render `build()`'s scene to PNG bytes at `width` px (height follows the
/// scene's aspect) with `samples` accumulation passes. Used by both the
/// thumbnail CLI and tests.
pub fn render_thumbnail_png(build: fn() -> Scene, width: u32, samples: u32) -> Vec<u8> {
    let mut scene = build();
    scene.camera.image_width = width.max(1);
    let world = build_world(&scene);
    let camera = Camera::from(scene.camera.clone());
    let mut r = ProgressiveRenderer::new(camera.image_width(), camera.image_height());
    for _ in 0..samples.max(1) {
        r.add_pass(&camera, &world);
    }
    r.to_png_bytes()
}

/// Render every sample to `assets/thumbnails/<slug>.png`. Invoked by
/// `just thumbnails` (`cargo run -- --gen-thumbnails`).
#[cfg(not(target_arch = "wasm32"))]
pub fn gen_thumbnails() -> std::io::Result<()> {
    let dir = std::path::Path::new("assets/thumbnails");
    std::fs::create_dir_all(dir)?;
    for sample in SAMPLES {
        let png = render_thumbnail_png(sample.build, 320, 128);
        let path = dir.join(format!("{}.png", slug(sample.name)));
        std::fs::write(&path, &png)?;
        println!("wrote {} ({} bytes)", path.display(), png.len());
    }
    Ok(())
}

/// Embedded pre-baked thumbnail PNG bytes for a sample (empty if unknown).
/// Files are produced by `just thumbnails`.
pub fn thumbnail_png(name: &str) -> &'static [u8] {
    match name {
        "Cornell Box" => include_bytes!("../../assets/thumbnails/cornell-box.png"),
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_basic_cases() {
        assert_eq!(slug("Cornell Box"), "cornell-box");
        assert_eq!(slug("  Spaced  Out  "), "spaced-out");
        assert_eq!(slug("Cornell — Clay!"), "cornell-clay");
    }

    #[test]
    fn new_scene_has_ground_and_valid_camera() {
        let s = new_scene();
        assert!(!s.objects.is_empty(), "new scene must have a ground plane");
        assert!(s.camera.image_width > 0);
    }

    #[test]
    fn every_sample_builds() {
        for sample in SAMPLES {
            let s = (sample.build)();
            assert!(
                s.camera.image_width > 0,
                "{} has zero-width camera",
                sample.name
            );
        }
    }

    #[test]
    fn thumbnail_renders_valid_png() {
        let png = render_thumbnail_png(SAMPLES[0].build, 64, 2);
        let img = image::load_from_memory(&png).expect("valid PNG");
        assert_eq!(img.width(), 64);
        assert!(img.height() > 0);
    }

    #[test]
    fn thumbnail_png_decodes_for_each_sample() {
        for sample in SAMPLES {
            let bytes = thumbnail_png(sample.name);
            assert!(!bytes.is_empty(), "{} thumbnail missing", sample.name);
            image::load_from_memory(bytes).expect("embedded thumbnail is valid PNG");
        }
    }
}
