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
    fn scatter(&self, ray: &Ray, _: &HitRecord) -> Option<(Ray, Color)> {
        return Some((ray.clone(), Color::new(1., 1., 1.)));
    }
}
