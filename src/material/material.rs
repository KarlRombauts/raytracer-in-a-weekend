use rand::rngs::SmallRng;

use crate::{
    color::Color,
    ray::{HitRecord, Ray},
    vec3::Point3,
};

pub trait Material: Send + Sync {
    fn scatter(&self, ray: &Ray, hit_record: &HitRecord, rng: &mut SmallRng)
        -> Option<(Ray, Color)>;
    fn emitted(&self, u: f32, v: f32, p: Point3) -> Color {
        Color::zeros()
    }
}
