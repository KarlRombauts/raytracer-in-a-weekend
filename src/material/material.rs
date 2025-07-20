use crate::{
    color::Color,
    ray::{HitRecord, Ray},
};

pub trait Material: Send + Sync {
    fn scatter(&self, ray: &Ray, hit_record: &HitRecord) -> Option<(Ray, Color)>;
}
