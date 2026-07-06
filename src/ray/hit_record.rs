use crate::{material::Material, ray::Ray, vec3::*};

/// A material-agnostic surface hit — everything geometry can report about where a
/// ray met a surface, with no material. This is what [`Intersect`](crate::ray::Intersect)
/// returns; the [`World`](crate::world::World) attaches the hit object's material
/// to produce a [`HitRecord`] for shading.
pub struct GeoHit {
    pub t: f32,
    pub p: Point3,
    pub normal: Vec3,
    pub front_face: bool,
    pub u: f32,
    pub v: f32,
}

impl GeoHit {
    pub fn new(t: f32, p: Point3, normal: Vec3) -> Self {
        GeoHit {
            t,
            p,
            normal,
            front_face: true,
            u: 0.,
            v: 0.,
        }
    }

    pub fn set_face_normal(&mut self, ray: &Ray, outward_normal: &Vec3) {
        self.front_face = ray.direction.dot(outward_normal) < 0.0;
        self.normal = if self.front_face {
            *outward_normal
        } else {
            -*outward_normal
        };
    }
}

/// A fully-resolved surface hit ready for shading: a [`GeoHit`]'s surface data
/// plus the material bound to it. Constructed by the [`World`](crate::world::World)
/// at the closest hit — the geometry never carries a material of its own — and
/// consumed by the integrators.
pub struct HitRecord<'a> {
    pub t: f32,
    pub p: Point3,
    pub normal: Vec3,
    pub front_face: bool,
    pub material: &'a dyn Material,
    pub u: f32,
    pub v: f32,
}

impl<'a> HitRecord<'a> {
    /// Build a shading record directly (front-facing, zero UVs). The World builds
    /// records via [`from_geo`](Self::from_geo); this is a convenience for unit
    /// tests that exercise a material against a fabricated hit.
    pub fn new(t: f32, p: Point3, normal: Vec3, material: &'a dyn Material) -> Self {
        HitRecord {
            t,
            p,
            normal,
            front_face: true,
            material,
            u: 0.,
            v: 0.,
        }
    }

    /// Bind `material` to a geometry hit, producing the shading record.
    pub fn from_geo(geo: GeoHit, material: &'a dyn Material) -> Self {
        HitRecord {
            t: geo.t,
            p: geo.p,
            normal: geo.normal,
            front_face: geo.front_face,
            material,
            u: geo.u,
            v: geo.v,
        }
    }
}
