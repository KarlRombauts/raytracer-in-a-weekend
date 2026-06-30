use std::sync::Arc;

use rand::prelude::*;

use crate::{
    color::Color,
    material::{microfacet, Material},
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

    pub fn from_texture(texture: Arc<dyn Texture>, roughness: f32) -> Self {
        Glossy { texture, roughness }
    }
}

impl Material for Glossy {
    fn is_specular(&self) -> bool {
        true
    }

    fn scatter(
        &self,
        ray_in: &Ray,
        hit_record: &HitRecord,
        rng: &mut rand::rngs::SmallRng,
    ) -> Option<(Ray, Color)> {
        // Single-lobe callers (if any) just drop the lobe flag.
        self.scatter_lobe(ray_in, hit_record, rng)
            .map(|(scattered, atten, _)| (scattered, atten))
    }

    /// Stochastically pick a lobe by Fresnel: the clear-coat reflection (a delta
    /// at `roughness == 0`, so *specular* → the integrator BSDF-samples it) or
    /// the diffuse base (*non-specular* → the integrator light-samples it, just
    /// like a Lambertian). Reporting the diffuse lobe as non-specular is what
    /// lets next-event estimation light the base; flagging the whole material
    /// specular (the old behaviour) starved the base of direct light and left
    /// black specks on shadowed-but-lit diffuse pixels.
    ///
    /// Selecting a lobe with its Fresnel probability and returning that lobe's
    /// albedo (white coat / base colour) keeps the estimate unbiased: the
    /// per-lobe selection probability cancels the Fresnel split. Because the
    /// lobe coin gates *both* the NEE and BSDF-bounce strategies equally, the
    /// diffuse branch reduces to ordinary Lambertian MIS conditioned on the base
    /// being chosen — so `scattering_pdf` is the bare cosine pdf (no Fresnel
    /// factor; see below).
    fn scatter_lobe(
        &self,
        ray_in: &Ray,
        hit_record: &HitRecord,
        rng: &mut rand::rngs::SmallRng,
    ) -> Option<(Ray, Color, bool)> {
        let unit_in = ray_in.direction.unit();
        let cos_theta = (-unit_in).dot(&hit_record.normal).clamp(0.0, 1.0);
        let fresnel = COAT_R0 + (1.0 - COAT_R0) * (1.0 - cos_theta).powi(5);

        if fresnel > rng.random::<f32>() {
            // Clear-coat reflection (white). Fresnel already chose this lobe, so
            // the coat carries no extra colour; roughness gives it a GGX shape.
            let alpha = self.roughness * self.roughness;
            let (dir, weight) = if alpha < microfacet::MIRROR_ALPHA {
                (Vec3::reflect(&unit_in, &hit_record.normal).unit(), 1.0)
            } else {
                match microfacet::ggx_reflect(&unit_in, &hit_record.normal, alpha, rng) {
                    // White coat: only the masking-shadowing weight applies.
                    Some((wo, _cos_half, g2_over_g1)) => (wo, g2_over_g1),
                    None => return None,
                }
            };
            if dir.dot(&hit_record.normal) <= 0.0 {
                return None;
            }
            let scattered = Ray::new_t(hit_record.p, dir, ray_in.time);
            Some((scattered, Color::new(weight, weight, weight), true))
        } else {
            let mut dir = hit_record.normal + Vec3::random_unit(rng);
            if dir.near_zero() {
                dir = hit_record.normal;
            }
            let scattered = Ray::new_t(hit_record.p, dir, ray_in.time);
            let attenuation = self
                .texture
                .value(hit_record.u, hit_record.v, &hit_record.p);
            Some((scattered, attenuation, false))
        }
    }

    /// Cosine pdf of the diffuse base, matching `Lambertian`. The integrator only
    /// consults this on the diffuse branch (after the base lobe was chosen), so no
    /// Fresnel weighting belongs here — the lobe selection already accounts for it.
    fn scattering_pdf(&self, hit_record: &HitRecord, dir: &Vec3) -> f32 {
        let cos = hit_record.normal.dot(&dir.unit());
        cos.max(0.0) / std::f32::consts::PI
    }
}

#[cfg(test)]
mod lobe_tests {
    use super::*;
    use crate::ray::HitRecord;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::f32::consts::PI;

    // A hit at the origin with an up normal; `cos_in` sets the view angle by
    // choosing the incoming ray direction's tilt toward the normal.
    fn hit_and_ray(cos_in: f32) -> (Ray, HitRecord<'static>) {
        let n = Vec3::new(0.0, 1.0, 0.0);
        // incoming direction with -dir·n = cos_in (heading down toward surface)
        let sin = (1.0 - cos_in * cos_in).max(0.0).sqrt();
        let dir = Vec3::new(sin, -cos_in, 0.0);
        let ray = Ray::new_t(Point3::new(0.0, 1.0, 0.0), dir, 0.0);
        let hit = HitRecord::new(1.0, Point3::new(0.0, 0.0, 0.0), n, leak_lambertian());
        (ray, hit)
    }

    // A 'static Material reference for HitRecord::new in tests.
    fn leak_lambertian() -> &'static dyn Material {
        use crate::material::Lambertian;
        Box::leak(Box::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0))))
    }

    #[test]
    fn head_on_is_mostly_diffuse_grazing_is_mostly_specular() {
        let g = Glossy::new(Color::new(0.7, 0.7, 0.7), 0.0);
        let count_specular = |cos_in: f32| {
            let mut rng = SmallRng::seed_from_u64(1);
            let mut spec = 0;
            for _ in 0..4000 {
                let (ray, hit) = hit_and_ray(cos_in);
                if let Some((_, _, s)) = g.scatter_lobe(&ray, &hit, &mut rng) {
                    if s {
                        spec += 1;
                    }
                }
            }
            spec
        };
        // Near head-on (cos≈1): Fresnel ≈ 0.04 → mostly the diffuse base.
        let head_on = count_specular(0.999);
        assert!(head_on < 400, "head-on should be mostly diffuse, got {head_on}/4000 specular");
        // Near grazing (cos≈0): Fresnel → 1 → almost all coat reflection.
        let grazing = count_specular(0.02);
        assert!(grazing > 3600, "grazing should be mostly specular, got {grazing}/4000");
    }

    #[test]
    fn diffuse_lobe_returns_base_colour_coat_returns_white() {
        let albedo = Color::new(0.2, 0.5, 0.8);
        let g = Glossy::new(albedo, 0.0);
        let mut rng = SmallRng::seed_from_u64(7);
        let (mut saw_diffuse, mut saw_coat) = (false, false);
        for _ in 0..4000 {
            let (ray, hit) = hit_and_ray(0.5);
            let (_, atten, specular) = g.scatter_lobe(&ray, &hit, &mut rng).unwrap();
            if specular {
                saw_coat = true;
                assert_eq!(atten, Color::new(1.0, 1.0, 1.0), "coat reflection must be white");
            } else {
                saw_diffuse = true;
                assert_eq!(atten, albedo, "diffuse base must return its albedo");
            }
        }
        assert!(saw_diffuse && saw_coat, "both lobes should occur at a mid angle");
    }

    #[test]
    fn diffuse_scattering_pdf_is_cosine_over_pi() {
        let g = Glossy::new(Color::new(0.7, 0.7, 0.7), 0.0);
        let (_, hit) = hit_and_ray(1.0);
        // straight up the normal -> 1/PI
        let up = g.scattering_pdf(&hit, &Vec3::new(0.0, 1.0, 0.0));
        assert!((up - 1.0 / PI).abs() < 1e-6, "up={up}");
        // below the surface -> 0
        assert_eq!(g.scattering_pdf(&hit, &Vec3::new(0.0, -1.0, 0.0)), 0.0);
    }
}
