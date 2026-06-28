use std::sync::Arc;

use crate::{
    color::Color,
    material::Material,
    ray::{HitRecord, Ray},
    texture::{SolidColor, Texture},
    vec3::Vec3,
};

pub struct Lambertian {
    texture: Arc<dyn Texture>,
}

impl Lambertian {
    pub fn from_color(albedo: Color) -> Self {
        Lambertian {
            texture: Arc::new(SolidColor::from_color(albedo)),
        }
    }
    pub fn from_texture(texture: Arc<dyn Texture>) -> Self {
        Lambertian { texture }
    }
}

impl Material for Lambertian {
    fn scatter(
        &self,
        ray_in: &crate::ray::Ray,
        hit_record: &HitRecord,
        rng: &mut rand::rngs::SmallRng,
    ) -> Option<(crate::ray::Ray, Color)> {
        let mut scatter_direction = hit_record.normal + Vec3::random_unit(rng);

        if scatter_direction.near_zero() {
            scatter_direction = hit_record.normal;
        }

        let scattered = Ray::new_t(hit_record.p, scatter_direction, ray_in.time);
        let attenuation = self
            .texture
            .value(hit_record.u, hit_record.v, &hit_record.p);

        Some((scattered, attenuation))
    }
}
