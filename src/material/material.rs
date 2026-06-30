use rand::rngs::SmallRng;

use crate::{
    color::Color,
    ray::{HitRecord, Ray},
    vec3::{Point3, Vec3},
};

pub trait Material: Send + Sync {
    fn scatter(&self, ray: &Ray, hit_record: &HitRecord, rng: &mut SmallRng)
        -> Option<(Ray, Color)>;
    fn emitted(&self, u: f32, v: f32, p: Point3) -> Color {
        Color::zeros()
    }

    /// Like [`scatter`](Self::scatter), but also reports whether the *sampled
    /// lobe* is specular (delta / near-delta) for this bounce. The integrator
    /// uses this per-hit flag — not the constant [`is_specular`](Self::is_specular)
    /// — to decide whether to run next-event estimation: a delta lobe can't be
    /// light-sampled, but a material's *diffuse* lobe must be (else it's only lit
    /// when a BSDF bounce randomly hits a light → fireflies / black specks).
    ///
    /// Default: a single-lobe material is uniformly specular-or-not, so delegate
    /// to `scatter` and `is_specular`. Multi-lobe materials (e.g. `Glossy`, a
    /// Fresnel coat over a diffuse base) override this to report the lobe they
    /// actually sampled.
    fn scatter_lobe(
        &self,
        ray: &Ray,
        hit_record: &HitRecord,
        rng: &mut SmallRng,
    ) -> Option<(Ray, Color, bool)> {
        let specular = self.is_specular();
        self.scatter(ray, hit_record, rng)
            .map(|(scattered, atten)| (scattered, atten, specular))
    }

    /// True for delta / near-delta BRDFs (mirror, glass, glossy coat) that are
    /// traced with their own scattered ray rather than mixture light-sampled.
    fn is_specular(&self) -> bool {
        false
    }

    /// Solid-angle PDF that this BRDF scatters into `dir` at `hit`. Default 0;
    /// diffuse materials override. `dir` need not be normalized.
    fn scattering_pdf(&self, _hit: &HitRecord, _dir: &Vec3) -> f32 {
        0.0
    }
}
