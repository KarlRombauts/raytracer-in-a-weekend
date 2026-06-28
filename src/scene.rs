use std::sync::Arc;

use crate::camera::CameraConfig;
use crate::color::Color;
use crate::geometry::{make_box, Quad, Rotate, Scale, Sphere, Translate};
use crate::group::IntersectGroup;
use crate::material::{Dielectric, DiffuseLight, Lambertian, Material, Metal};
use crate::ray::Intersect;
use crate::vec3::{Point3, Vec3};

/// Plain-data description of a material. Built into an `Arc<dyn Material>` only
/// when the world is (re)assembled, so the editor can mutate it freely.
#[derive(Clone)]
pub enum MaterialSpec {
    Lambertian { albedo: Color },
    Metal { albedo: Color, fuzz: f32 },
    Dielectric { ior: f32 },
    DiffuseLight { emit: Color },
}

impl MaterialSpec {
    fn build(&self) -> Arc<dyn Material> {
        match self {
            MaterialSpec::Lambertian { albedo } => Arc::new(Lambertian::from_color(*albedo)),
            MaterialSpec::Metal { albedo, fuzz } => Arc::new(Metal::new(*albedo, *fuzz)),
            MaterialSpec::Dielectric { ior } => Arc::new(Dielectric::new(*ior)),
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

    pub fn is_editable(&self) -> bool {
        !matches!(self, Shape::Mesh(_))
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
