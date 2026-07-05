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
pub trait AreaLight: Intersect {
    /// A (possibly unnormalized) direction from `origin` toward a point on this
    /// surface, sampled from canonical uniforms `(u, v)` in [0, 1)². Taking
    /// explicit uniforms (rather than an rng) lets the caller supply *stratified*
    /// numbers.
    fn sample_dir(&self, origin: Point3, u: f32, v: f32) -> Vec3;

    /// Solid-angle PDF of sampling direction `dir` (from `origin`) toward this
    /// surface. `dir` may be unnormalized.
    fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32;
}

/// Shared area→solid-angle PDF conversion for a flat/convex single surface of
/// known `area`: intersect `surface` along `dir`, then convert the uniform
/// `1/area` area density to a solid-angle density via the hit distance and the
/// light's facing cosine. Correct for any convex single surface that reports a
/// real `area`. Returns 0 on a miss or a grazing hit. `dir` may be unnormalized.
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
