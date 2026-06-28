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

    fn scattering_pdf(&self, hit_record: &HitRecord, dir: &crate::vec3::Vec3) -> f32 {
        let cos = hit_record.normal.dot(&dir.unit());
        (cos.max(0.0)) / std::f32::consts::PI
    }
}

#[cfg(test)]
mod pdf_tests {
    use super::*;
    use crate::material::{Dielectric, Glossy, Material, Metal};
    use crate::vec3::{Point3, Vec3};

    #[test]
    fn lambertian_scattering_pdf_is_cosine_over_pi() {
        let lam = Lambertian::from_color(Color::new(0.0, 0.0, 0.0));
        let hit = crate::ray::HitRecord::new(
            1.0,
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            &lam,
        );
        // dir straight up the normal: cos = 1 => 1/PI
        let up = lam.scattering_pdf(&hit, &Vec3::new(0.0, 1.0, 0.0));
        assert!((up - 1.0 / std::f32::consts::PI).abs() < 1e-6, "up={up}");
        // dir parallel to the surface: cos = 0 => 0
        let side = lam.scattering_pdf(&hit, &Vec3::new(1.0, 0.0, 0.0));
        assert!(side.abs() < 1e-6, "side={side}");
        // dir below the surface: clamped to 0
        let down = lam.scattering_pdf(&hit, &Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(down, 0.0);
    }

    #[test]
    fn specular_flags_are_correct() {
        assert!(!Lambertian::from_color(Color::new(0.0, 0.0, 0.0)).is_specular());
        assert!(Metal::new(Color::new(0.5, 0.5, 0.5), 0.0).is_specular());
        assert!(Dielectric::new(1.5).is_specular());
        assert!(Glossy::new(Color::new(0.5, 0.5, 0.5), 0.0).is_specular());
    }
}
