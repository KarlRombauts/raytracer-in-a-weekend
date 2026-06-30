use crate::{
    color::Color,
    material::{microfacet, Material},
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

impl Metal {
    /// Schlick Fresnel for a conductor: the reflectance rises from the metal's
    /// own colour at normal incidence (`F0 = albedo`) toward white at grazing
    /// angles. This is what gives real metal its bright, slightly desaturated
    /// rim; a constant `albedo` (the old behaviour) reads as flat painted metal.
    fn fresnel(&self, cos_theta: f32) -> Color {
        let c = (1.0 - cos_theta.clamp(0.0, 1.0)).powi(5);
        self.albedo + (Color::ones() - self.albedo) * c
    }
}

impl Material for Metal {
    fn is_specular(&self) -> bool {
        true
    }

    fn scatter(
        &self,
        ray_in: &Ray,
        hit_record: &HitRecord,
        rng: &mut rand::rngs::SmallRng,
    ) -> Option<(crate::ray::Ray, Color)> {
        // GGX roughness from a perceptual slider: alpha = roughness² (Disney/UE
        // convention) so the control feels linear.
        let alpha = self.roughness * self.roughness;
        if alpha < microfacet::MIRROR_ALPHA {
            // Perfect mirror: exact reflection, Fresnel at the macro incidence.
            let cos_theta = (-ray_in.direction.unit()).dot(&hit_record.normal).clamp(0.0, 1.0);
            let reflected = Vec3::reflect(&ray_in.direction, &hit_record.normal).unit();
            if reflected.dot(&hit_record.normal) <= 0.0 {
                return None;
            }
            return Some((Ray::new_t(hit_record.p, reflected, ray_in.time), self.fresnel(cos_theta)));
        }
        // Rough metal: GGX microfacet reflection. Throughput = F(half-angle) ·
        // G2/G1 (the masking-shadowing weight that VNDF sampling leaves behind).
        let (wo, cos_half, g2_over_g1) =
            microfacet::ggx_reflect(&ray_in.direction, &hit_record.normal, alpha, rng)?;
        let attenuation = self.fresnel(cos_half) * g2_over_g1;
        Some((Ray::new_t(hit_record.p, wo, ray_in.time), attenuation))
    }
}

#[cfg(test)]
mod fresnel_tests {
    use super::*;
    use crate::vec3::Point3;

    #[test]
    fn head_on_reflects_the_metal_colour_grazing_goes_white() {
        let albedo = Color::new(0.9, 0.6, 0.3); // copper-ish
        let m = Metal::new(albedo, 0.0);
        // Head-on (cos = 1): reflectance == albedo.
        let f0 = m.fresnel(1.0);
        assert!((f0.x - 0.9).abs() < 1e-5 && (f0.y - 0.6).abs() < 1e-5 && (f0.z - 0.3).abs() < 1e-5);
        // Grazing (cos = 0): reflectance == white.
        let fg = m.fresnel(0.0);
        assert!((fg.x - 1.0).abs() < 1e-5 && (fg.y - 1.0).abs() < 1e-5 && (fg.z - 1.0).abs() < 1e-5);
        // Monotonic brightening toward grazing.
        assert!(m.fresnel(0.3).y > m.fresnel(0.8).y);
    }

    #[test]
    fn reflectance_never_dims_below_albedo() {
        let albedo = Color::new(0.2, 0.4, 0.8);
        let m = Metal::new(albedo, 0.1);
        for &cos in &[0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let f = m.fresnel(cos);
            assert!(f.x >= albedo.x - 1e-6 && f.y >= albedo.y - 1e-6 && f.z >= albedo.z - 1e-6);
            assert!(f.x <= 1.0 + 1e-6 && f.y <= 1.0 + 1e-6 && f.z <= 1.0 + 1e-6);
        }
        let _ = Point3::new(0.0, 0.0, 0.0);
    }
}
