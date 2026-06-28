use std::path::Path;
use std::sync::Arc;

use crate::camera::CameraConfig;
use crate::color::Color;
use crate::geometry::{make_box, ObjData, Quad, Rotate, Scale, Sphere, Translate};
use crate::group::IntersectGroup;
use crate::material::{Dielectric, DiffuseLight, Glossy, Lambertian, Material, Metal};
use crate::ray::{Intersect, BVH};
use crate::vec3::{Point3, Vec3};

/// Plain-data description of a material. Built into an `Arc<dyn Material>` only
/// when the world is (re)assembled, so the editor can mutate it freely.
#[derive(Clone)]
pub enum MaterialSpec {
    Lambertian { albedo: Color },
    Glossy { albedo: Color, roughness: f32 },
    Metal { albedo: Color, fuzz: f32 },
    Dielectric { ior: f32, tint: Color, roughness: f32 },
    DiffuseLight { emit: Color },
}

impl MaterialSpec {
    fn build(&self) -> Arc<dyn Material> {
        match self {
            MaterialSpec::Lambertian { albedo } => Arc::new(Lambertian::from_color(*albedo)),
            MaterialSpec::Glossy { albedo, roughness } => {
                Arc::new(Glossy::new(*albedo, *roughness))
            }
            MaterialSpec::Metal { albedo, fuzz } => Arc::new(Metal::new(*albedo, *fuzz)),
            MaterialSpec::Dielectric {
                ior,
                tint,
                roughness,
            } => Arc::new(Dielectric::new_glass(*ior, *tint, *roughness)),
            MaterialSpec::DiffuseLight { emit } => Arc::new(DiffuseLight::from_color(*emit)),
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
    Mesh(Arc<dyn Intersect>),
}

impl Shape {
    fn build(&self, material: Arc<dyn Material>) -> Arc<dyn Intersect> {
        match self {
            Shape::Sphere { center, radius } => {
                Arc::new(Sphere::stationary(*center, *radius, material))
            }
            Shape::Quad { q, u, v } => Arc::new(Quad::new(*q, *u, *v, material)),
            Shape::Box { a, b } => Arc::new(make_box(*a, *b, material)),
            Shape::Mesh(object) => object.clone(),
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
            albedo: Color::new(0.73, 0.73, 0.73),
        };
        let triangles = ObjData::load(path_str).into_triangles(material.build());
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
            shape: Shape::Mesh(Arc::new(bvh)),
            material,
            transform,
        })
    }

    fn build(&self) -> Arc<dyn Intersect> {
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
/// call on every edit (Mesh handles are shared, not rebuilt).
pub fn build_world(scene: &Scene) -> IntersectGroup {
    let mut world = IntersectGroup::new();
    for obj in &scene.objects {
        world.add(obj.build());
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
            Shape::Mesh(_) => continue,
        };
        min = Vec3::min(&min, &lo);
        max = Vec3::max(&max, &hi);
        any = true;
    }

    any.then_some((min, max))
}
