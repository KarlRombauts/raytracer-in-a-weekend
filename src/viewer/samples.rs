//! Bundled sample scenes for the library (Home) screen. Each sample is a
//! `.scene` file under `assets/scenes/`, loaded on demand, with a matching
//! `.png` thumbnail saved beside it.

use crate::scene::{MaterialSpec, ObjectSpec, Scene, Shape, TextureSpec, Transform};
use crate::vec3::{Point3, Vec3};

/// One bundled sample scene. `file` is the basename (no extension): the scene is
/// `assets/scenes/<file>.scene`, fetched on click ([`scene_url`]). The card
/// thumbnail is a small PNG embedded in the binary (see [`sample_thumbnail`]) so
/// cards render instantly on both native and the web. `res` is the scene's
/// render resolution, shown on the card (the scene itself isn't decoded to learn
/// it — too large).
pub struct Sample {
    pub name: &'static str,
    pub file: &'static str,
    pub res: (u32, u32),
}

pub static SAMPLES: &[Sample] = &[
    Sample { name: "Cornell Box", file: "cornell-box", res: (600, 600) },
    Sample { name: "Cornell Box II", file: "cornell-box-2", res: (600, 600) },
    Sample { name: "Glass Block", file: "glass-block", res: (960, 600) },
    Sample { name: "Teapot", file: "teapot", res: (600, 600) },
    Sample { name: "Spoons", file: "spoons", res: (900, 600) },
    Sample { name: "Sample Objects II", file: "sample-objects-2", res: (920, 600) },
    Sample { name: "Sculpture", file: "sculpture", res: (600, 600) },
    Sample { name: "Sample Objects", file: "sample-objects", res: (960, 720) },
];

/// Where a sample's `.scene` lives: a filesystem path on native, an HTTP path
/// (relative to the page) in the browser. Trunk copies `assets/scenes` into the
/// bundle, so the same string resolves on both via [`crate::platform::fetch_file`].
pub fn scene_url(file: &str) -> String {
    format!("assets/scenes/{file}.scene")
}

/// A sample's card thumbnail — a small PNG baked into the binary, so cards show
/// instantly and offline on both native and the web (the full-res scene PNGs on
/// disk aren't reachable from a browser). Empty for an unknown file.
pub fn sample_thumbnail(file: &str) -> &'static [u8] {
    match file {
        "cornell-box" => include_bytes!("../../assets/thumbnails/cornell-box.png"),
        "cornell-box-2" => include_bytes!("../../assets/thumbnails/cornell-box-2.png"),
        "glass-block" => include_bytes!("../../assets/thumbnails/glass-block.png"),
        "teapot" => include_bytes!("../../assets/thumbnails/teapot.png"),
        "spoons" => include_bytes!("../../assets/thumbnails/spoons.png"),
        "sample-objects-2" => include_bytes!("../../assets/thumbnails/sample-objects-2.png"),
        "sculpture" => include_bytes!("../../assets/thumbnails/sculpture.png"),
        "sample-objects" => include_bytes!("../../assets/thumbnails/sample-objects.png"),
        _ => &[],
    }
}

/// Decode a sample's `.scene` straight from disk. Native-only — the app loads
/// samples through [`crate::platform::fetch_file`] (which fetches over HTTP on
/// the web); this is kept for tests that verify the bundled scenes decode.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_sample(file: &str) -> Option<Scene> {
    let bytes = std::fs::read(scene_url(file)).ok()?;
    crate::scene_file::decode(&bytes).ok().map(|loaded| loaded.scene)
}

/// Minimal starting scene for "New scene": a default camera plus a neutral
/// ground plane, so the viewport isn't empty and the user can Add Object.
pub fn new_scene() -> Scene {
    // A 5×5 ground plane centred on the origin (x, z ∈ [-2.5, 2.5]), at y = 0 so
    // added objects sit on it.
    let ground = ObjectSpec {
        name: "Ground".to_string(),
        shape: Shape::Quad {
            q: Point3::new(-2.5, 0.0, -2.5),
            u: Vec3::new(5.0, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, 5.0),
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(crate::color::Color::new(0.62, 0.62, 0.64)),
        },
        transform: Transform::identity(),
        hidden: false,
    };

    // A three-quarter view from the front, slightly elevated and aimed just
    // above the ground centre, so the whole plate is in frame with headroom for
    // objects the user adds. Sky-blue background rather than the Cornell void.
    let camera = crate::camera::CameraConfig::builder()
        .aspect_ratio(1.0)
        .image_width(600)
        .samples(200)
        .max_depth(50)
        .fov(38.0)
        .look_from(Vec3::new(3.8, 3.3, 6.6))
        .look_at(Vec3::new(0.0, 0.4, 0.0))
        .background(crate::color::Color::new(0.70, 0.80, 1.0))
        .dof_angle(0.0)
        .build();

    Scene {
        camera,
        objects: vec![ground],
    }
}

/// Sample thumbnails now ship alongside each `.scene` as `assets/scenes/<file>.png`
/// (saved with the scene), so there's nothing to pre-bake. Kept so the
/// `--gen-thumbnails` CLI entry still resolves.
#[cfg(not(target_arch = "wasm32"))]
pub fn gen_thumbnails() -> std::io::Result<()> {
    println!("Sample thumbnails ship as assets/scenes/<file>.png; nothing to generate.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scene_has_ground_and_valid_camera() {
        let s = new_scene();
        assert!(!s.objects.is_empty(), "new scene must have a ground plane");
        assert!(s.camera.image_width > 0);
    }

    /// Every registered sample's `.scene` and `.png` exist on disk, decode, and
    /// the scene has a usable camera. Native-only (no filesystem on wasm).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn every_sample_file_loads_with_a_thumbnail() {
        for sample in SAMPLES {
            let scene = load_sample(sample.file)
                .unwrap_or_else(|| panic!("{} ({}) failed to load", sample.name, sample.file));
            assert!(
                scene.camera.image_width > 0,
                "{} has zero-width camera",
                sample.name
            );
            let thumb = sample_thumbnail(sample.file);
            assert!(!thumb.is_empty(), "{} thumbnail missing", sample.name);
            image::load_from_memory(thumb).expect("sample thumbnail is a valid PNG");
        }
    }
}
