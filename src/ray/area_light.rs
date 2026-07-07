use crate::{
    interval::Interval,
    ray::{Intersect, Ray},
    vec3::{Point3, Vec3},
};

/// A surface the integrator can sample as a light (next-event estimation): it
/// hands back a direction from a shading point toward itself, and reports the
/// solid-angle density of that choice.
///
/// Only geometries that can genuinely act as area lights implement it — the
/// primitives (`Sphere`, `Quad`, `Triangle`). The transform decorators
/// (`Translate`/`Scale`/`Rotate`) deliberately do **not**: a transformed light
/// is baked into a fresh concrete primitive at world-build time instead. Because
/// `Light` holds an `Arc<dyn AreaLight>`, registering a geometry that can't be
/// sampled is a compile error rather than a silent `area() == 0`.
/// A sample toward an [`AreaLight`] from a shading point: the (unnormalized)
/// direction `wi`, the ray-parameter `t_light` at which the light's surface lies
/// along `wi` (so a shadow ray can be bounded to just short of it), and the
/// solid-angle `pdf`. Radiance is deliberately *not* here — the geometry is
/// material-agnostic; the World attaches emission (via [`Material::emitted`]).
///
/// [`Material::emitted`]: crate::material::Material::emitted
pub struct AreaLightSample {
    pub wi: Vec3,
    pub t_light: f32,
    pub pdf: f32,
}

pub trait AreaLight: Intersect {
    /// A (possibly unnormalized) direction from `origin` toward a point on this
    /// surface, sampled from canonical uniforms `(u, v)` in [0, 1)². Taking
    /// explicit uniforms (rather than an rng) lets the caller supply *stratified*
    /// numbers.
    fn sample_dir(&self, origin: Point3, u: f32, v: f32) -> Vec3;

    /// Solid-angle PDF of sampling direction `dir` (from `origin`) toward this
    /// surface. `dir` may be unnormalized. Used for MIS against a BSDF ray that
    /// hit this light from an *arbitrary* direction, so it intersects the surface.
    fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32;

    /// Sample a direction toward a point on this light from `origin`, returning
    /// the direction, the ray-parameter of the surface along it, and the
    /// solid-angle pdf — computed *together* so the hot next-event path pays at
    /// most one ray/surface intersection (none for a flat surface, whose sampled
    /// point is known directly).
    fn sample_toward(&self, origin: Point3, u: f32, v: f32) -> AreaLightSample;
}

/// Shared area→solid-angle PDF conversion for a flat/convex single surface of
/// known `area`: intersect `surface` along `dir`, then convert the uniform
/// `1/area` area density to a solid-angle density via the hit distance and the
/// light's facing cosine. Correct for any convex single surface that reports a
/// real `area`. Returns 0 on a miss or a grazing hit. `dir` may be unnormalized.
/// Fused sample for a flat/convex surface whose sampled `point` is known: the
/// point sits at `t = 1` along `wi = point - origin`, so its distance and facing
/// cosine — and thus the solid-angle pdf — come straight from the point, with no
/// ray/surface intersection. The pdf matches [`surface_pdf_value`] for the same
/// direction (both are `dist² / (cos · area)`).
pub fn surface_sample_toward(point: Point3, normal: Vec3, area: f32, origin: Point3) -> AreaLightSample {
    let wi = point - origin;
    let dist2 = wi.length_squared();
    let cos = (wi.dot(&normal) / wi.length()).abs();
    let pdf = if cos < 1e-8 || area <= 0.0 {
        0.0
    } else {
        dist2 / (cos * area)
    };
    AreaLightSample { wi, t_light: 1.0, pdf }
}

pub fn surface_pdf_value(surface: &dyn Intersect, area: f32, origin: Point3, dir: Vec3) -> f32 {
    let ray = Ray::new(origin, dir);
    match surface.intersect(&ray, &Interval::new(0.001, f32::INFINITY)) {
        None => 0.0,
        Some(hit) => {
            let dist2 = hit.t * hit.t * dir.length_squared();
            let cos = (dir.dot(&hit.normal) / dir.length()).abs();
            if cos < 1e-8 || area <= 0.0 {
                0.0
            } else {
                dist2 / (cos * area)
            }
        }
    }
}
