use crate::{
    color::Color,
    material::Material,
    ray::{HitRecord, Ray},
};

pub struct Blank {}

impl Blank {
    pub fn new() -> Self {
        Blank {}
    }
}
impl Material for Blank {
    fn scatter(
        &self,
        ray: &Ray,
        _: &HitRecord,
        _rng: &mut rand::rngs::SmallRng,
    ) -> Option<(Ray, Color)> {
        return Some((ray.clone(), Color::new(1., 1., 1.)));
    }
    fn emitted(&self, u: f32, v: f32, p: crate::vec3::Point3) -> Color {
        return Color::zeros();
    }
}
