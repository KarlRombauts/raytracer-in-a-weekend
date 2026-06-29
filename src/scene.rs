use std::path::Path;
use std::sync::Arc;

use crate::camera::CameraConfig;
use crate::color::Color;
use crate::geometry::{make_box, ObjData, Quad, Rotate, Scale, Sphere, Translate};
use crate::group::{IntersectGroup, Light};
use crate::material::{Dielectric, DiffuseLight, Glossy, Lambertian, Material, Metal};
use crate::texture::{
    CheckerTexture, ImageTexture, NoiseTexture, SolidColor, Texture,
};
use crate::ray::{Intersect, BVH};
use crate::vec3::{Point3, Vec3};

/// An embedded binary asset (image bytes now; meshes in Phase 2). Bytes are the
/// single source of truth, so a scene is self-contained and portable. `label`
/// is for display only (e.g. "earth.png").
#[derive(Clone)]
pub struct Asset {
    pub bytes: Arc<[u8]>,
    pub label: Option<String>,
}

impl Asset {
    /// An asset with no bytes yet — builds to the magenta placeholder until a
    /// file is chosen in the editor.
    pub fn empty() -> Self {
        Asset { bytes: Arc::from([] as [u8; 0]), label: None }
    }
}

/// The magenta sentinel used when an image asset fails to decode.
fn magenta() -> Arc<dyn Texture> {
    Arc::new(SolidColor::from_color(Color::new(1.0, 0.0, 1.0)))
}

/// Plain-data description of a texture, mirroring the core `Texture` types.
#[derive(Clone)]
pub enum TextureSpec {
    Solid { color: Color },
    Checker { scale: f32, even: CellTexture, odd: CellTexture },
    Noise { scale: f32 },
    Image { asset: Asset },
}

/// A checker cell. Deliberately omits `Checker`, so checker-in-checker
/// recursion is unrepresentable (one level of nesting only).
#[derive(Clone)]
pub enum CellTexture {
    Solid { color: Color },
    Noise { scale: f32 },
    Image { asset: Asset },
}

fn build_image(asset: &Asset) -> Arc<dyn Texture> {
    match ImageTexture::from_bytes(&asset.bytes) {
        Ok(t) => Arc::new(t),
        Err(_) => magenta(),
    }
}

impl CellTexture {
    fn build(&self) -> Arc<dyn Texture> {
        match self {
            CellTexture::Solid { color } => Arc::new(SolidColor::from_color(*color)),
            CellTexture::Noise { scale } => Arc::new(NoiseTexture::new(*scale)),
            CellTexture::Image { asset } => build_image(asset),
        }
    }

    fn preview_color(&self) -> Color {
        match self {
            CellTexture::Solid { color } => *color,
            CellTexture::Noise { .. } => Color::new(0.5, 0.5, 0.5),
            CellTexture::Image { .. } => Color::new(0.5, 0.5, 0.5),
        }
    }
}

impl TextureSpec {
    /// A bare flat color is just a solid texture.
    pub fn solid(color: Color) -> Self {
        TextureSpec::Solid { color }
    }

    pub fn build(&self) -> Arc<dyn Texture> {
        match self {
            TextureSpec::Solid { color } => Arc::new(SolidColor::from_color(*color)),
            TextureSpec::Checker { scale, even, odd } => {
                Arc::new(CheckerTexture::from_textures(*scale, even.build(), odd.build()))
            }
            TextureSpec::Noise { scale } => Arc::new(NoiseTexture::new(*scale)),
            TextureSpec::Image { asset } => build_image(asset),
        }
    }

    /// A representative flat color for the rasterized preview and the editor's
    /// type-switch carry-over. Cheap and deterministic — never decodes an image
    /// (the preview runs every frame), so images report a neutral gray.
    pub fn preview_color(&self) -> Color {
        match self {
            TextureSpec::Solid { color } => *color,
            TextureSpec::Checker { even, odd, .. } => {
                (even.preview_color() + odd.preview_color()) * 0.5
            }
            TextureSpec::Noise { .. } => Color::new(0.5, 0.5, 0.5),
            TextureSpec::Image { .. } => Color::new(0.5, 0.5, 0.5),
        }
    }
}

/// Plain-data description of a material. Built into an `Arc<dyn Material>` only
/// when the world is (re)assembled, so the editor can mutate it freely.
#[derive(Clone)]
pub enum MaterialSpec {
    Lambertian { albedo: TextureSpec },
    Glossy { albedo: TextureSpec, roughness: f32 },
    Metal { albedo: Color, fuzz: f32 },
    Dielectric { ior: f32, tint: Color, roughness: f32 },
    DiffuseLight { emit: TextureSpec },
}

impl MaterialSpec {
    fn build(&self) -> Arc<dyn Material> {
        match self {
            MaterialSpec::Lambertian { albedo } => {
                Arc::new(Lambertian::from_texture(albedo.build()))
            }
            MaterialSpec::Glossy { albedo, roughness } => {
                Arc::new(Glossy::from_texture(albedo.build(), *roughness))
            }
            MaterialSpec::Metal { albedo, fuzz } => Arc::new(Metal::new(*albedo, *fuzz)),
            MaterialSpec::Dielectric { ior, tint, roughness } => {
                Arc::new(Dielectric::new_glass(*ior, *tint, *roughness))
            }
            MaterialSpec::DiffuseLight { emit } => {
                Arc::new(DiffuseLight::from_texture(emit.build()))
            }
        }
    }
}

/// Plain-data description of a shape. `Mesh` is an escape hatch for prebuilt,
/// non-editable geometry (e.g. a loaded OBJ wrapped in a BVH) — it's stored as
/// a shared handle and ignores the object's material.
#[derive(Clone)]
pub enum Shape {
    Sphere { center: Point3, radius: f32 },
    Quad { q: Point3, u: Vec3, v: Vec3 },
    Box { a: Point3, b: Point3 },
    Mesh {
        object: Arc<dyn Intersect>,
        render: Arc<crate::geometry::RenderMesh>,
    },
}

impl Shape {
    fn build(&self, material: Arc<dyn Material>) -> Arc<dyn Intersect> {
        match self {
            Shape::Sphere { center, radius } => {
                Arc::new(Sphere::stationary(*center, *radius, material))
            }
            Shape::Quad { q, u, v } => Arc::new(Quad::new(*q, *u, *v, material)),
            Shape::Box { a, b } => Arc::new(make_box(*a, *b, material)),
            Shape::Mesh { object, .. } => object.clone(),
        }
    }

    /// Triangle geometry for the rasterized preview, in the shape's own
    /// definition space (the object's transform is applied separately as a
    /// model matrix).
    pub fn render_mesh(&self) -> crate::geometry::RenderMesh {
        use crate::geometry::RenderMesh;
        match self {
            Shape::Sphere { center, radius } => RenderMesh::sphere(*center, *radius, 16, 24),
            Shape::Quad { q, u, v } => RenderMesh::quad(*q, *u, *v),
            Shape::Box { a, b } => RenderMesh::unit_box(*a, *b),
            Shape::Mesh { render, .. } => RenderMesh {
                positions: render.positions.clone(),
                normals: render.normals.clone(),
            },
        }
    }
}

/// Scale and Euler rotation (degrees) about the object's own centre, followed
/// by a world translation.
#[derive(Clone)]
pub struct Transform {
    pub rotate: Vec3,
    pub scale: Vec3,
    pub translate: Vec3,
}

impl Transform {
    pub fn identity() -> Self {
        Transform {
            rotate: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            translate: Vec3::ZERO,
        }
    }
}

#[derive(Clone)]
pub struct ObjectSpec {
    pub name: String,
    pub shape: Shape,
    pub material: MaterialSpec,
    pub transform: Transform,
}

impl ObjectSpec {
    /// Load a Wavefront OBJ as a BVH-backed mesh object, auto-fitting it to
    /// `target_center` and `target_size` (the largest mesh dimension is scaled
    /// to roughly `target_size`). This keeps imports visible regardless of the
    /// OBJ's native units/origin. The mesh keeps the baked default material and
    /// is positioned via Transform, never per-vertex. Returns `None` if the
    /// file isn't readable.
    ///
    /// Note: the underlying loader panics on malformed OBJ content; we only
    /// guard the readability of the path here.
    pub fn from_obj(path: &Path, target_center: Vec3, target_size: f32) -> Option<ObjectSpec> {
        let path_str = path.to_str()?;
        std::fs::metadata(path).ok()?; // bail early if unreadable

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mesh".to_string());

        let material = MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.73, 0.73, 0.73)),
        };
        let obj = ObjData::load(path_str);
        let render = Arc::new(obj.render_mesh());
        let triangles = obj.into_triangles(material.build());
        let bvh = BVH::build(triangles);

        // Auto-fit: scale the mesh to the target size and recentre it. The
        // transform wraps the BVH (no rebuild) — scale pivots about the mesh's
        // own centre, then translate moves that centre to the target.
        let bbox = bvh.bounding_box();
        let c = bbox.center();
        let e = bbox.extent();
        let e_max = e.x.max(e.y).max(e.z).max(1e-6);
        let s = (target_size / e_max).max(1e-4);

        let transform = Transform {
            rotate: Vec3::ZERO,
            scale: Vec3::new(s, s, s),
            translate: target_center - c,
        };

        Some(ObjectSpec {
            name,
            shape: Shape::Mesh { object: Arc::new(bvh), render },
            material,
            transform,
        })
    }

    /// World-space centre of the object's base geometry, ignoring its transform.
    /// This is the pivot `build` rotates and scales about, and the point the GL
    /// preview centres on — so it's where the transform gizmo should sit.
    pub(crate) fn pivot(&self) -> Vec3 {
        self.shape.build(self.material.build()).bounding_box().center()
    }

    pub(crate) fn build(&self) -> Arc<dyn Intersect> {
        let t = &self.transform;
        let mut object = self.shape.build(self.material.build());

        // Apply scale and rotation about the object's own centre so editing
        // feels in-place, rather than swinging it around the world origin.
        let one = Vec3::new(1.0, 1.0, 1.0);
        if t.rotate != Vec3::ZERO || t.scale != one {
            let c = object.bounding_box().center();
            object = Arc::new(Translate::new(object, -c));
            if t.scale != one {
                object = Arc::new(Scale::new(object, t.scale));
            }
            if t.rotate != Vec3::ZERO {
                object = Arc::new(Rotate::new(object, t.rotate));
            }
            object = Arc::new(Translate::new(object, c));
        }
        if t.translate != Vec3::ZERO {
            object = Arc::new(Translate::new(object, t.translate));
        }
        object
    }
}

#[derive(Clone)]
pub struct Scene {
    pub camera: CameraConfig,
    pub objects: Vec<ObjectSpec>,
}

/// Assemble the renderable world from the scene description. Cheap enough to
/// call on every edit (Mesh handles are shared, not rebuilt). Emissive objects
/// are also registered in `world.lights` for direct light sampling.
pub fn build_world(scene: &Scene) -> IntersectGroup {
    let mut world = IntersectGroup::new();
    for obj in &scene.objects {
        let geom = obj.build();
        world.add(geom.clone());
        if let MaterialSpec::DiffuseLight { emit } = &obj.material {
            // Only register emitters we can importance-sample (area() > 0).
            // Others (sphere/mesh/transformed) still glow when hit directly,
            // they're just not shadow-ray sampled.
            if geom.area() > 0.0 {
                world.lights.push(Light {
                    geom,
                    // Emission is Solid-only in Phase 1, so `preview_color()`
                    // equals the true emitted colour exactly. If emission ever
                    // becomes a non-Solid texture, this would feed a
                    // representative average here — revisit then.
                    emit: emit.preview_color(),
                });
            }
        }
    }
    world
}

/// Rough world-space bounds `(min, max)` of the placeable primitives, using
/// their base shape parameters (ignoring transforms). Meshes are skipped — they
/// can't be bounded without building them. Used to auto-fit imported meshes into
/// the existing scene. Returns `None` if there are no such primitives.
pub fn placeable_bounds(objects: &[ObjectSpec]) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3::new(-f32::INFINITY, -f32::INFINITY, -f32::INFINITY);
    let mut any = false;

    for o in objects {
        let (lo, hi) = match &o.shape {
            Shape::Sphere { center, radius } => {
                let r = Vec3::new(*radius, *radius, *radius);
                (*center - r, *center + r)
            }
            Shape::Box { a, b } => (Vec3::min(a, b), Vec3::max(a, b)),
            Shape::Quad { q, u, v } => {
                let corners = [*q, *q + *u, *q + *v, *q + *u + *v];
                let mut lo = corners[0];
                let mut hi = corners[0];
                for c in &corners[1..] {
                    lo = Vec3::min(&lo, c);
                    hi = Vec3::max(&hi, c);
                }
                (lo, hi)
            }
            Shape::Mesh { .. } => continue,
        };
        min = Vec3::min(&min, &lo);
        max = Vec3::max(&max, &hi);
        any = true;
    }

    any.then_some((min, max))
}

#[cfg(test)]
mod light_tests {
    use super::*;
    use crate::scenes::cornell_box;

    #[test]
    fn cornell_box_collects_one_light() {
        let scene = cornell_box();
        let world = build_world(&scene);
        assert_eq!(world.lights.len(), 1, "expected exactly one light");
        assert_eq!(world.lights[0].emit, Color::new(15.0, 15.0, 15.0));
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    use crate::camera::CameraConfig;

    #[test]
    fn only_area_lights_are_registered() {
        let quad_light = ObjectSpec {
            name: "quad".to_string(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 5.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 1.0),
            },
            material: MaterialSpec::DiffuseLight { emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)) },
            transform: Transform::identity(),
        };
        let sphere_light = ObjectSpec {
            name: "sphere".to_string(),
            shape: Shape::Sphere { center: Point3::new(0.0, 0.0, 0.0), radius: 1.0 },
            material: MaterialSpec::DiffuseLight { emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)) },
            transform: Transform::identity(),
        };
        let scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![quad_light, sphere_light],
        };
        let world = build_world(&scene);
        // Sphere keeps area()=0 (deferred) => not registered; quad is.
        assert_eq!(world.lights.len(), 1, "only the quad (area>0) should register");
        // Both objects still live in the world geometry (the sphere still glows).
        assert_eq!(world.objects.len(), 2, "both objects remain in the world");
    }
}

#[cfg(test)]
mod render_mesh_tests {
    use super::*;
    use crate::vec3::{Point3, Vec3};

    #[test]
    fn primitive_shapes_produce_nonempty_meshes() {
        let sphere = Shape::Sphere { center: Point3::new(0.0, 0.0, 0.0), radius: 1.0 };
        let quad = Shape::Quad {
            q: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
        };
        let bx = Shape::Box { a: Point3::new(0.0, 0.0, 0.0), b: Point3::new(1.0, 1.0, 1.0) };
        assert!(!sphere.render_mesh().positions.is_empty());
        assert_eq!(quad.render_mesh().positions.len(), 6);
        assert_eq!(bx.render_mesh().positions.len(), 36);
    }
}

#[cfg(test)]
mod texture_spec_tests {
    use super::*;
    use crate::color::Color;
    use crate::vec3::Point3;

    #[test]
    fn solid_builds_and_previews_its_color() {
        let t = TextureSpec::solid(Color::new(0.2, 0.4, 0.6));
        let built = t.build();
        let c = built.value(0.0, 0.0, &Point3::new(0.0, 0.0, 0.0));
        assert!((c.x - 0.2).abs() < 1e-6 && (c.y - 0.4).abs() < 1e-6 && (c.z - 0.6).abs() < 1e-6);
        assert_eq!(t.preview_color(), Color::new(0.2, 0.4, 0.6));
    }

    #[test]
    fn checker_previews_the_average_of_its_cells() {
        let t = TextureSpec::Checker {
            scale: 1.0,
            even: CellTexture::Solid { color: Color::new(0.0, 0.0, 0.0) },
            odd: CellTexture::Solid { color: Color::new(1.0, 1.0, 1.0) },
        };
        let _ = t.build(); // builds without panic
        let p = t.preview_color();
        assert!((p.x - 0.5).abs() < 1e-6 && (p.y - 0.5).abs() < 1e-6 && (p.z - 0.5).abs() < 1e-6);
    }

    #[test]
    fn noise_previews_mid_gray() {
        let t = TextureSpec::Noise { scale: 4.0 };
        let _ = t.build();
        assert_eq!(t.preview_color(), Color::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn bad_image_builds_to_magenta_not_a_panic() {
        let t = TextureSpec::Image { asset: Asset { bytes: vec![1, 2, 3].into(), label: None } };
        let built = t.build(); // must not panic
        let c = built.value(0.5, 0.5, &Point3::new(0.0, 0.0, 0.0));
        assert_eq!(c, Color::new(1.0, 0.0, 1.0));
        // Image preview is a constant neutral gray (no per-frame decode).
        assert_eq!(t.preview_color(), Color::new(0.5, 0.5, 0.5));
    }
}
