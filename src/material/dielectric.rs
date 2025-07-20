use crate::{
    color::Color,
    material::Material,
    ray::{HitRecord, Ray},
    vec3::Vec3,
};
use rand::prelude::*;

pub struct Dielectric {
    refraction_index: f32,
}

impl Dielectric {
    pub fn new(refraction_index: f32) -> Self {
        Dielectric { refraction_index }
    }

    fn reflectance(&self, cosine: f32, refraction_index: f32) -> f32 {
        let mut r0 = (1.0 - refraction_index) / (1.0 + refraction_index);
        r0 = r0 * r0;
        return r0 + (1.0 - r0) * (1.0 - cosine).powi(5);
    }
}

impl Material for Dielectric {
    fn scatter(&self, ray_in: &Ray, hit_record: &HitRecord) -> Option<(Ray, Color)> {
        let attenuation = Color::new(1.0, 1.0, 1.0);

        let ri = if hit_record.front_face {
            1.0 / self.refraction_index
        } else {
            self.refraction_index
        };

        let unit_direction = ray_in.direction.unit();

        let cos_theta = hit_record.normal.dot(&(-unit_direction)).min(1.0);
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();

        let cannot_refract = ri * sin_theta > 1.0;

        let mut rng = rand::rng();

        let direction = if cannot_refract || self.reflectance(cos_theta, ri) > rng.random::<f32>() {
            Vec3::reflect(&unit_direction, &hit_record.normal)
        } else {
            Vec3::refract(&unit_direction, &hit_record.normal, ri)
        };

        let scattered = Ray::new_t(hit_record.p, direction, ray_in.time);

        Some((scattered, attenuation))
    }
}
