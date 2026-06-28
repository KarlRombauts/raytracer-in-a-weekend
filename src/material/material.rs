use rand::rngs::SmallRng;

use crate::{
    color::Color,
    ray::{HitRecord, Ray},
    vec3::{Point3, Vec3},
};

pub trait Material: Send + Sync {
    fn scatter(&self, ray: &Ray, hit_record: &HitRecord, rng: &mut SmallRng)
        -> Option<(Ray, Color)>;
    fn emitted(&self, u: f32, v: f32, p: Point3) -> Color {
        Color::zeros()
    }

    /// True for delta / near-delta BRDFs (mirror, glass, glossy coat) that are
    /// traced with their own scattered ray rather than mixture light-sampled.
    fn is_specular(&self) -> bool {
        false
    }

    /// Solid-angle PDF that this BRDF scatters into `dir` at `hit`. Default 0;
    /// diffuse materials override. `dir` need not be normalized.
    fn scattering_pdf(&self, _hit: &HitRecord, _dir: &Vec3) -> f32 {
        0.0
    }
}
