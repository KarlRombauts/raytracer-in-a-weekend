use crate::{
    color::Color,
    material::Material,
    ray::{HitRecord, Ray},
    vec3::Vec3,
};
use rand::prelude::*;

pub struct Dielectric {
    refraction_index: f32,
    /// Per-channel absorption coefficient (Beer–Lambert). Derived from the tint so
    /// that `tint` is the transmittance at unit thickness; 0 = perfectly clear.
    absorption: Color,
    /// Blurs reflection/refraction directions, like metal fuzz (0 = sharp).
    roughness: f32,
}

impl Dielectric {
    pub fn new(refraction_index: f32) -> Self {
        Self::new_glass(refraction_index, Color::new(1.0, 1.0, 1.0), 0.0)
    }

    pub fn new_glass(refraction_index: f32, tint: Color, roughness: f32) -> Self {
        Dielectric {
            refraction_index,
            absorption: Self::absorption_from_tint(tint),
            roughness,
        }
    }

    /// `tint` is the colour the glass transmits over one world-unit of thickness;
    /// Beer–Lambert needs the absorption coefficient `σ` with `transmittance =
    /// exp(-σ·d)`, so `σ = -ln(tint)`. Clamped away from 0 (a 0 channel would be
    /// `-ln(0) = ∞` = fully opaque in that channel, which is the intended limit).
    fn absorption_from_tint(tint: Color) -> Color {
        let s = |c: f32| -c.clamp(1e-4, 1.0).ln();
        Color::new(s(tint.x), s(tint.y), s(tint.z))
    }

    /// Beer–Lambert transmittance for a path segment. Only segments *inside* the
    /// glass attenuate: a back-face hit (`front_face == false`) ends an interior
    /// segment of length `t`, so it absorbs `exp(-σ·t)`. A front-face hit (the ray
    /// arrived through air — an entry or an external reflection) is unattenuated,
    /// which is what keeps surface reflections from being wrongly tinted.
    fn transmittance(&self, front_face: bool, t: f32) -> Color {
        if front_face {
            Color::ones()
        } else {
            Color::new(
                (-self.absorption.x * t).exp(),
                (-self.absorption.y * t).exp(),
                (-self.absorption.z * t).exp(),
            )
        }
    }

    fn reflectance(&self, cosine: f32, refraction_index: f32) -> f32 {
        let mut r0 = (1.0 - refraction_index) / (1.0 + refraction_index);
        r0 = r0 * r0;
        return r0 + (1.0 - r0) * (1.0 - cosine).powi(5);
    }
}

impl Material for Dielectric {
    fn is_specular(&self) -> bool {
        true
    }

    fn scatter(
        &self,
        ray_in: &Ray,
        hit_record: &HitRecord,
        rng: &mut rand::rngs::SmallRng,
    ) -> Option<(Ray, Color)> {
        // Beer–Lambert: attenuate by the distance the ray just travelled inside
        // the glass (an interior segment ends at this back-face hit). Entry hits
        // and external reflections (front faces) pass through unattenuated.
        let attenuation = self.transmittance(hit_record.front_face, hit_record.t);

        let ri = if hit_record.front_face {
            1.0 / self.refraction_index
        } else {
            self.refraction_index
        };

        let unit_direction = ray_in.direction.unit();

        let cos_theta = hit_record.normal.dot(&(-unit_direction)).min(1.0);
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();

        let cannot_refract = ri * sin_theta > 1.0;

        let mut direction =
            if cannot_refract || self.reflectance(cos_theta, ri) > rng.random::<f32>() {
                Vec3::reflect(&unit_direction, &hit_record.normal)
            } else {
                Vec3::refract(&unit_direction, &hit_record.normal, ri)
            };

        // Rough glass: jitter the outgoing direction, like metal fuzz.
        if self.roughness > 0.0 {
            direction = direction.unit() + self.roughness * Vec3::random_unit(rng);
        }

        let scattered = Ray::new_t(hit_record.p, direction, ray_in.time);

        Some((scattered, attenuation))
    }

    fn emitted(&self, u: f32, v: f32, p: crate::vec3::Point3) -> Color {
        return Color::zeros();
    }
}

#[cfg(test)]
mod absorption_tests {
    use super::*;

    #[test]
    fn clear_glass_never_absorbs() {
        let g = Dielectric::new(1.5); // tint = white
        for &t in &[0.0_f32, 1.0, 10.0] {
            let a = g.transmittance(false, t);
            assert!((a.x - 1.0).abs() < 1e-5 && (a.y - 1.0).abs() < 1e-5 && (a.z - 1.0).abs() < 1e-5,
                "clear glass attenuated at t={t}: {a:?}");
        }
    }

    #[test]
    fn coloured_glass_deepens_with_thickness() {
        // tint is the transmittance at unit thickness.
        let tint = Color::new(0.9, 0.5, 0.2);
        let g = Dielectric::new_glass(1.5, tint, 0.0);
        // At t = 1 the transmittance equals the tint.
        let one = g.transmittance(false, 1.0);
        assert!((one.x - 0.9).abs() < 1e-4 && (one.y - 0.5).abs() < 1e-4 && (one.z - 0.2).abs() < 1e-4,
            "t=1 should equal tint: {one:?}");
        // At t = 2 it is tint², i.e. deeper colour (more absorbed).
        let two = g.transmittance(false, 2.0);
        assert!((two.x - 0.81).abs() < 1e-4 && (two.y - 0.25).abs() < 1e-4 && (two.z - 0.04).abs() < 1e-4,
            "t=2 should be tint²: {two:?}");
        // The strongly-absorbed channel falls off fastest.
        assert!(two.z < two.y && two.y < two.x);
    }

    #[test]
    fn external_reflection_is_not_tinted() {
        // A front-face hit (entry / external reflection) never picks up glass colour.
        let g = Dielectric::new_glass(1.5, Color::new(0.2, 0.5, 0.9), 0.3);
        let a = g.transmittance(true, 5.0);
        assert_eq!(a, Color::ones());
    }
}
