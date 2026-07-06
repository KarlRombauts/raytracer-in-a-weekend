use core::f32;
use rand::prelude::*;

use crate::color::Color;
use crate::world::World;
use crate::integrator::common::{cosine_direction_from_uv, power_heuristic, russian_roulette};
use crate::integrator::Integrator;
use crate::interval::Interval;
use crate::material::Material;
use crate::ray::{Intersect, Ray};
use crate::sampling::SampleId;

/// Path tracer with next-event estimation and multiple importance sampling
/// (power heuristic). The low-variance default. The sky (miss-radiance and its
/// importance-sampled light) is owned by the World.
pub struct Mis {
    pub max_depth: u32,
}

impl Integrator for Mis {
    fn radiance(&self, ray: &Ray, world: &World, sample: SampleId, rng: &mut SmallRng) -> Color {
        let interval = Interval::new(0.001, f32::INFINITY);
        let mut color = Color::ZERO;
        let mut throughput = Color::ones();
        let mut current = ray.clone();
        // Whether emission at the next hit gets full weight: true for the camera
        // ray and after specular bounces (NEE can't sample those), false after a
        // diffuse bounce (NEE already accounted for direct light, so MIS-weight
        // the emission).
        let mut specular_bounce = true;
        let mut prev_brdf_pdf = 0.0_f32;

        for depth in 0..self.max_depth {
            let Some(hit) = world.intersect(&current, &interval) else {
                // The sky is a sampled light (if it's an environment map), so a
                // BSDF bounce that escapes to it is MIS-weighted against the sky's
                // pdf — mirroring the emitter branch, to avoid double-counting the
                // sky NEE below. Camera rays / specular bounces take it full weight.
                let sky = world.sky_radiance(&current.direction);
                if specular_bounce {
                    color += throughput * sky;
                } else {
                    let p_light = world.light_pdf(current.origin, current.direction);
                    let w = power_heuristic(prev_brdf_pdf, p_light);
                    color += throughput * sky * w;
                }
                break;
            };

            let emitted = hit.material.emitted(hit.u, hit.v, hit.p);
            if emitted != Color::ZERO {
                if specular_bounce {
                    color += throughput * emitted;
                } else {
                    let p_light = world.light_pdf(current.origin, current.direction);
                    let w = power_heuristic(prev_brdf_pdf, p_light);
                    color += throughput * emitted * w;
                }
            }

            let Some((scattered, atten, specular)) = hit.material.scatter_lobe(&current, &hit, rng)
            else {
                break; // pure light / absorber
            };

            // `specular` is the *sampled lobe*, not the material: a Glossy hit is
            // specular when it took its coat reflection and diffuse when it took
            // its base, so the base gets next-event estimation like a Lambertian.
            if specular {
                throughput = throughput * atten;
                current = scattered;
                specular_bounce = true;
                continue;
            }

            // Lambertian.
            let albedo = atten;

            // Stratify the NEE light sample (dim 1) and the BSDF bounce (dim 2) on
            // the camera ray's first (diffuse) hit; deeper bounces fall back to rng.
            let (lu, lv) = if depth == 0 { sample.stratified(1) } else { (rng.random(), rng.random()) };
            let (bu, bv) = if depth == 0 { sample.stratified(2) } else { (rng.random(), rng.random()) };

            // (1) Next-event estimation: sample a light, weight against the BRDF pdf.
            if let Some(ld) = world.sample_light_dir(hit.p, lu, lv, rng) {
                let p_light = world.light_pdf(hit.p, ld);
                let p_brdf = hit.material.scattering_pdf(&hit, &ld);
                if p_light > 0.0 && p_brdf > 0.0 {
                    let shadow = Ray::new_t(hit.p, ld, current.time);
                    // Radiance from whatever the shadow ray reaches: an occluder
                    // (or non-emitter) gives 0 = shadowed; a surface light gives
                    // its emission; an *escaping* ray reaches the sky (the env
                    // light). `p_light` is the marginal over ALL lights, so even if
                    // the ray reaches a different light than the one sampled, the
                    // estimator stays unbiased — don't "fix" this to the sampled
                    // light's own pdf.
                    let le = match world.intersect(&shadow, &interval) {
                        Some(lh) => lh.material.emitted(lh.u, lh.v, lh.p),
                        None => world.sky_radiance(&ld),
                    };
                    if le != Color::ZERO {
                        let w = power_heuristic(p_light, p_brdf);
                        color += throughput * w * albedo * (p_brdf / p_light) * le;
                    }
                }
            }

            // (2) BRDF bounce (cosine), weighted against the light pdf at the next hit.
            let dir = cosine_direction_from_uv(&hit.normal, bu, bv);
            let p_brdf = hit.material.scattering_pdf(&hit, &dir);
            if p_brdf <= 0.0 {
                break;
            }
            throughput = throughput * albedo;
            // Russian roulette: terminate the dim tail early (unbiased) instead
            // of always running to `max_depth`.
            match russian_roulette(throughput, depth, rng) {
                Some(t) => throughput = t,
                None => break,
            }
            prev_brdf_pdf = p_brdf;
            specular_bounce = false;
            current = Ray::new_t(hit.p, dir, current.time);
        }

        color
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Quad;
    use crate::world::World;
    use crate::material::DiffuseLight;
    use crate::ray::Ray;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn ceiling_light() -> Arc<Quad> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 2.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    fn sample() -> SampleId {
        SampleId { i: 0, j: 0, index: 0 }
    }

    #[test]
    fn empty_world_returns_the_flat_sky() {
        // A camera ray (throughput 1) that hits nothing returns the sky exactly.
        let mis = Mis { max_depth: 10 };
        let mut world = World::new();
        world.sky = crate::integrator::Sky::Flat(Color::new(0.2, 0.4, 0.6));
        let ray = Ray::new(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0));
        let mut rng = SmallRng::seed_from_u64(1);
        assert_eq!(mis.radiance(&ray, &world, sample(), &mut rng), Color::new(0.2, 0.4, 0.6));
    }

    #[test]
    fn a_ray_into_an_emitter_returns_its_emission() {
        // First hit is the emitter: emission is taken at full weight (camera ray),
        // scatter returns None, the path ends — a deterministic result.
        let mis = Mis { max_depth: 10 };
        let mut world = World::new();
        world.add(ceiling_light());
        let ray = Ray::new(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(1);
        let c = mis.radiance(&ray, &world, sample(), &mut rng);
        assert!((c.x - 5.0).abs() < 1e-4, "expected emitter colour, got {c:?}");
    }

    #[test]
    fn max_depth_zero_traces_nothing() {
        // With no bounces the loop never runs, so not even the sky is gathered.
        let mis = Mis { max_depth: 0 };
        let world = World::new();
        let ray = Ray::new(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0));
        let mut rng = SmallRng::seed_from_u64(1);
        assert_eq!(mis.radiance(&ray, &world, sample(), &mut rng), Color::ZERO);
    }
}

// Statistical tests migrated from camera.rs: they used to build a whole `Camera`
// to reach `ray_color`; now they construct a `Mis` directly. Same assertions —
// this is the extraction's payoff, the integrator tested through its own seam.
#[cfg(test)]
mod mixture_tests {
    use super::*;
    use crate::geometry::Quad;
    use crate::world::{Light, World};
    use crate::material::{DiffuseLight, Lambertian};
    use crate::ray::{Intersect, Ray};
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn integ() -> Mis {
        Mis { max_depth: 10 }
    }

    fn floor() -> Arc<dyn Intersect> {
        let mat = Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 0.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    // Large overhead emitter (covers a big solid angle so pure-GI sampling
    // converges with feasible sample counts).
    fn ceiling_light() -> Arc<Quad> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 2.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    fn avg_floor_color(register_light: bool) -> f32 {
        let integ = integ();
        let mut world = World::new();
        world.add(floor());
        let light = ceiling_light();
        world.add(light.clone());
        if register_light {
            world.lights.push(Light::Area { geom: light, emit: Color::new(5.0, 5.0, 5.0) });
        }
        // Look straight down at the floor centre.
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let n = 8000u32;
        let mut sum = 0.0;
        for s in 0..n {
            sum += integ.radiance(&ray, &world, SampleId { i: 0, j: 0, index: s }, &mut rng).x;
        }
        sum / n as f32
    }

    #[test]
    fn mixture_matches_pure_gi_mean() {
        // With the light registered, the diffuse bounce is mixture-sampled
        // (light + cosine); unregistered, it is pure cosine GI. Both estimators
        // are unbiased, so their means must agree (mixture only cuts variance).
        let with_nee = avg_floor_color(true);
        let pure_gi = avg_floor_color(false);
        assert!(with_nee > 0.0 && pure_gi > 0.0, "both lit: nee={with_nee} gi={pure_gi}");
        let rel = (with_nee - pure_gi).abs() / pure_gi;
        assert!(rel < 0.05, "means should agree (unbiased): nee={with_nee} gi={pure_gi} rel={rel}");
    }
}

#[cfg(test)]
mod mis_tests {
    use super::*;
    use crate::geometry::Quad;
    use crate::world::{Light, World};
    use crate::material::{DiffuseLight, Lambertian};
    use crate::ray::{Intersect, Ray};
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn integ() -> Mis {
        Mis { max_depth: 10 }
    }

    fn floor() -> Arc<dyn Intersect> {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.7, 0.7, 0.7)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 0.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    // Small, bright overhead light: pure GI rarely hits it (high variance);
    // NEE samples it every bounce (low variance).
    fn small_light() -> Arc<Quad> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(40.0, 40.0, 40.0)));
        Arc::new(Quad::new(
            Point3::new(-0.5, 4.0, -0.5),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            mat,
        ))
    }

    // Returns (mean, variance) of the .x channel over `n` samples.
    fn stats(register_light: bool, n: u32) -> (f32, f32) {
        let integ = integ();
        let mut world = World::new();
        world.add(floor());
        let l = small_light();
        world.add(l.clone());
        if register_light {
            world.lights.push(Light::Area { geom: l, emit: Color::new(40.0, 40.0, 40.0) });
        }
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        for s in 0..n {
            let x = integ.radiance(&ray, &world, SampleId { i: 0, j: 0, index: s }, &mut rng).x;
            sum += x;
            sum2 += x * x;
        }
        let mean = sum / n as f32;
        let var = (sum2 / n as f32) - mean * mean;
        (mean, var)
    }

    #[test]
    fn mis_cuts_variance_versus_pure_gi() {
        let n = 4000;
        let (mis_mean, mis_var) = stats(true, n);
        let (gi_mean, gi_var) = stats(false, n);
        assert!(mis_mean > 0.0 && gi_mean > 0.0, "both lit: mis={mis_mean} gi={gi_mean}");
        assert!(
            mis_var < 0.5 * gi_var,
            "expected MIS variance well below pure-GI: mis_var={mis_var} gi_var={gi_var}"
        );
    }
}

// Seam 1 for env-map MIS: the sky sampled as a light. A white floor lit by an
// environment map with one tiny bright "sun" — BSDF sampling rarely finds it
// (Naive is noisy), env NEE finds it every sample (Mis is clean).
#[cfg(test)]
mod env_mis_tests {
    use super::*;
    use crate::geometry::Quad;
    use crate::world::{Light, World};
    use crate::integrator::{Naive, Sky};
    use crate::material::Lambertian;
    use crate::ray::{Intersect, Ray};
    use crate::texture::env_map::EnvMap;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn sunny_world() -> World {
        let floor: Arc<dyn Intersect> = Arc::new(Quad::new(
            Point3::new(-50.0, 0.0, -50.0),
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 100.0),
            Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0))),
        ));
        // One bright texel high in the sky (row 1 of 8 ≈ near straight up); dim
        // elsewhere. The sun subtends a tiny solid angle.
        let (w, h) = (16usize, 8usize);
        let mut data = vec![[0.05f32; 3]; w * h];
        data[w + 8] = [200.0, 200.0, 200.0];
        let env = Arc::new(EnvMap::from_pixels(w, h, data));
        let mut world = World::new();
        world.add(floor);
        world.sky = Sky::Env(env.clone());
        world.lights.push(Light::Env(env));
        world
    }

    // (mean, variance) of the .x channel looking straight down at the lit floor.
    fn stats(integ: &dyn Integrator, n: u32) -> (f32, f32) {
        let world = sunny_world();
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let (mut sum, mut sum2) = (0.0f64, 0.0f64);
        for s in 0..n {
            let x = integ.radiance(&ray, &world, SampleId { i: 0, j: 0, index: s }, &mut rng).x as f64;
            sum += x;
            sum2 += x * x;
        }
        let mean = sum / n as f64;
        ((mean) as f32, (sum2 / n as f64 - mean * mean) as f32)
    }

    #[test]
    fn mis_cuts_sky_variance_versus_naive() {
        let n = 4000;
        let (mis_mean, mis_var) = stats(&Mis { max_depth: 4 }, n);
        let (naive_mean, naive_var) = stats(&Naive { max_depth: 4 }, n);
        assert!(mis_mean > 0.0 && naive_mean > 0.0, "both lit: mis={mis_mean} naive={naive_mean}");
        // The point: env NEE finds the sun every sample, so far lower variance.
        assert!(
            mis_var < 0.5 * naive_var,
            "expected MIS sky variance well below Naive: mis_var={mis_var} naive_var={naive_var}"
        );
    }

    // A large bright sky region — the whole upper hemisphere (rows 0..4 of 8) —
    // so Naive converges too, and a tight mean match proves env-MIS is unbiased
    // (correct weights, no double-counting of the sky).
    fn bright_dome_world() -> World {
        let floor: Arc<dyn Intersect> = Arc::new(Quad::new(
            Point3::new(-50.0, 0.0, -50.0),
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 100.0),
            Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0))),
        ));
        let (w, h) = (8usize, 8usize);
        let mut data = vec![[0.0f32; 3]; w * h];
        for y in 0..4 {
            for x in 0..w {
                data[y * w + x] = [2.0, 2.0, 2.0];
            }
        }
        let env = Arc::new(EnvMap::from_pixels(w, h, data));
        let mut world = World::new();
        world.add(floor);
        world.sky = Sky::Env(env.clone());
        world.lights.push(Light::Env(env));
        world
    }

    #[test]
    fn mis_agrees_with_naive_in_mean_on_a_broad_sky() {
        let n = 8000u32;
        let mean = |integ: &dyn Integrator| -> f32 {
            let world = bright_dome_world();
            let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
            let mut rng = SmallRng::seed_from_u64(3);
            let mut sum = 0.0f64;
            for s in 0..n {
                sum += integ.radiance(&ray, &world, SampleId { i: 0, j: 0, index: s }, &mut rng).x as f64;
            }
            (sum / n as f64) as f32
        };
        let (mis, naive) = (mean(&Mis { max_depth: 4 }), mean(&Naive { max_depth: 4 }));
        assert!(mis > 0.0 && naive > 0.0, "both lit: mis={mis} naive={naive}");
        let rel = (mis - naive).abs() / naive;
        assert!(rel < 0.08, "env-MIS must be unbiased (no double-count): mis={mis} naive={naive} rel={rel}");
    }
}
