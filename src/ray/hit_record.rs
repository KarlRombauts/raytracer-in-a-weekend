use crate::{material::Material, ray::AreaLight, ray::Ray, vec3::*};

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
    /// The hit object's area-light handle when it is a *registered* (sampleable)
    /// emitter, else `None` (ordinary surfaces and BSDF-only emitters). A dumb
    /// identity token: the integrator holds it to ask the World for this light's
    /// pdf (`World::light_pdf`) in the emitter-hit MIS branch, but never
    /// dereferences it — the pdf math lives in the World.
    pub light: Option<&'a dyn AreaLight>,
    pub u: f32,
    pub v: f32,
}

impl<'a> HitRecord<'a> {
    /// Build a shading record directly (front-facing, zero UVs, no light). Production
    /// code only ever builds records via [`from_geo`](Self::from_geo) — the World is
    /// the sole place a material is bound to a hit — so this is gated to tests
    /// that exercise a material against a fabricated hit.
    #[cfg(test)]
    pub fn new(t: f32, p: Point3, normal: Vec3, material: &'a dyn Material) -> Self {
        HitRecord {
            t,
            p,
            normal,
            front_face: true,
            material,
            light: None,
            u: 0.,
            v: 0.,
        }
    }

    /// Bind `material` and the hit object's `light` identity to a geometry hit,
    /// producing the shading record.
    pub fn from_geo(geo: GeoHit, material: &'a dyn Material, light: Option<&'a dyn AreaLight>) -> Self {
        HitRecord {
            t: geo.t,
            p: geo.p,
            normal: geo.normal,
            front_face: geo.front_face,
            material,
            light,
            u: geo.u,
            v: geo.v,
        }
    }
}
