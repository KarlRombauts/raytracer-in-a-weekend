use crate::{
    color::Color,
    material::Material,
    ray::{HitRecord, Ray},
    vec3::Vec3,
};

pub struct Metal {
    albedo: Color,
    roughness: f32,
}

impl Metal {
    pub fn new(albedo: Color, roughness: f32) -> Self {
        Metal { albedo, roughness }
    }
}

impl Material for Metal {
    fn scatter(&self, ray_in: &Ray, hit_record: &HitRecord) -> Option<(crate::ray::Ray, Color)> {
        let mut reflected = Vec3::reflect(&ray_in.direction, &hit_record.normal);
        reflected = reflected.unit() + (self.roughness * Vec3::random_unit());
        let scattered = Ray::new_t(hit_record.p, reflected, ray_in.time);
        let attenuation = self.albedo;
        if scattered.direction.dot(&hit_record.normal) > 0.0 {
            return Some((scattered, attenuation));
        }
        return None;
    }
}
