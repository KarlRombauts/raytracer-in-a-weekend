use std::sync::Arc;

use rand::prelude::*;

use crate::{
    color::Color,
    material::Material,
    ray::{HitRecord, Ray},
    texture::{SolidColor, Texture},
    vec3::Vec3,
};

/// Normal-incidence reflectance of the clear coat (≈ a dielectric of IOR 1.5).
const COAT_R0: f32 = 0.04;

/// A diffuse base under a Fresnel-weighted dielectric clear coat. The coat
/// reflection is *uncoloured* (white), so the surface reads as glossy plastic
/// rather than tinted metal. Schlick's Fresnel keeps the reflection subtle
/// head-on and strong at grazing angles. `roughness` blurs the reflection:
/// 0 = sharp clear coat, 1 = satin.
pub struct Glossy {
    texture: Arc<dyn Texture>,
    roughness: f32,
}

impl Glossy {
    pub fn new(albedo: Color, roughness: f32) -> Self {
        Glossy {
            texture: Arc::new(SolidColor::from_color(albedo)),
            roughness,
        }
    }
}

impl Material for Glossy {
    fn scatter(
        &self,
        ray_in: &Ray,
        hit_record: &HitRecord,
        rng: &mut rand::rngs::SmallRng,
    ) -> Option<(Ray, Color)> {
        let unit_in = ray_in.direction.unit();
        let cos_theta = (-unit_in).dot(&hit_record.normal).clamp(0.0, 1.0);
        let fresnel = COAT_R0 + (1.0 - COAT_R0) * (1.0 - cos_theta).powi(5);

        if fresnel > rng.random::<f32>() {
            // Clear-coat reflection (white), blurred by roughness like metal fuzz.
            let reflected = Vec3::reflect(&unit_in, &hit_record.normal).unit();
            let mut dir = reflected + self.roughness * Vec3::random_unit(rng);
            if dir.dot(&hit_record.normal) <= 0.0 {
                dir = reflected; // keep the bounce above the surface
            }
            let scattered = Ray::new_t(hit_record.p, dir, ray_in.time);
            Some((scattered, Color::new(1.0, 1.0, 1.0)))
        } else {
            let mut dir = hit_record.normal + Vec3::random_unit(rng);
            if dir.near_zero() {
                dir = hit_record.normal;
            }
            let scattered = Ray::new_t(hit_record.p, dir, ray_in.time);
            let attenuation = self
                .texture
                .value(hit_record.u, hit_record.v, &hit_record.p);
            Some((scattered, attenuation))
        }
    }
}
