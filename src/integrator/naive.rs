use core::f32;
use rand::prelude::*;

use crate::color::Color;
use crate::world::World;
use crate::integrator::common::{cosine_direction_from_uv, russian_roulette};
use crate::integrator::Integrator;
use crate::interval::Interval;
use crate::ray::Ray;
use crate::sampling::SampleId;

/// Naive path tracer: BSDF sampling only. Emission is taken whenever a ray lands
/// on an emitter (no next-event estimation, no MIS) — noisier than `Mis`, and the
/// baseline it is compared against. The sky is owned by the World.
pub struct Naive {
    pub max_depth: u32,
}

impl Integrator for Naive {
    fn radiance(&self, ray: &Ray, world: &World, sample: SampleId, rng: &mut SmallRng) -> Color {
        let interval = Interval::new(0.001, f32::INFINITY);
        let mut color = Color::ZERO;
        let mut throughput = Color::ones();
        let mut current = ray.clone();

        for depth in 0..self.max_depth {
            let Some(hit) = world.intersect(&current, &interval) else {
                color += throughput * world.sky_radiance(&current.direction);
                break;
            };

            // No NEE, so emission is always taken at full weight (nothing else
            // could double-count it).
            let emitted = hit.material.emitted(hit.u, hit.v, hit.p);
            if emitted != Color::ZERO {
                color += throughput * emitted;
            }

            let Some((scattered, atten, specular)) = hit.material.scatter_lobe(&current, &hit, rng)
            else {
                break; // pure light / absorber
            };

            if specular {
                throughput = throughput * atten;
                current = scattered;
                continue;
            }

            // Diffuse: a cosine-weighted BSDF bounce. The cosine pdf cancels the
            // cosine term, leaving `albedo` as the throughput factor. Stratify the
            // first bounce (dim 2); deeper bounces fall back to rng.
            let (bu, bv) = if depth == 0 { sample.stratified(2) } else { (rng.random(), rng.random()) };
            let dir = cosine_direction_from_uv(&hit.normal, bu, bv);
            throughput = throughput * atten;
            match russian_roulette(throughput, depth, rng) {
                Some(t) => throughput = t,
                None => break,
            }
            current = Ray::new_t(hit.p, dir, current.time);
        }

        color
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Quad;
    use crate::world::{Light, Object, World};
    use crate::integrator::Mis;
    use crate::material::{DiffuseLight, Lambertian};
    use crate::ray::Ray;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn floor() -> Object {
        Object {
            geometry: Arc::new(Quad::new(
                Point3::new(-5.0, 0.0, -5.0),
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 10.0),
            )),
            material: Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0))),
        }
    }

    fn ceiling_light() -> Arc<Quad> {
        Arc::new(Quad::new(
            Point3::new(-5.0, 2.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
        ))
    }

    fn lit_world() -> World {
        let mut w = World::new();
        w.add(floor());
        let light = ceiling_light();
        w.add(Object {
            geometry: light.clone(),
            material: Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0))),
        });
        // Registered for NEE so Mis can shadow-sample it; Naive ignores lights.
        w.lights.push(Light::Area { geom: light, emit: Color::new(5.0, 5.0, 5.0) });
        w
    }

    // Average the .x channel of `n` radiance samples of the lit floor, looking
    // straight down at its centre.
    fn avg_floor(integ: &dyn Integrator, n: u32) -> f32 {
        let world = lit_world();
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let mut sum = 0.0;
        for s in 0..n {
            sum += integ.radiance(&ray, &world, SampleId { i: 0, j: 0, index: s }, &mut rng).x;
        }
        sum / n as f32
    }

    #[test]
    fn naive_lights_a_diffuse_floor_via_bsdf_bounces() {
        // No NEE: the floor is lit only because cosine bounces sometimes land on
        // the (large) emitter. The mean must still be positive.
        let naive = Naive { max_depth: 10 };
        assert!(avg_floor(&naive, 8000) > 0.0, "naive should light the floor by bouncing onto the emitter");
    }

    #[test]
    fn empty_world_returns_the_flat_sky() {
        let naive = Naive { max_depth: 10 };
        let mut world = World::new();
        world.sky = crate::integrator::Sky::Flat(Color::new(0.2, 0.4, 0.6));
        let ray = Ray::new(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0));
        let mut rng = SmallRng::seed_from_u64(1);
        assert_eq!(
            naive.radiance(&ray, &world, SampleId { i: 0, j: 0, index: 0 }, &mut rng),
            Color::new(0.2, 0.4, 0.6)
        );
    }

    #[test]
    fn naive_and_mis_agree_in_mean() {
        // Both are unbiased estimators of the same scene, so their means must
        // agree; MIS just has lower variance. This is the proof the seam is real.
        let naive = Naive { max_depth: 10 };
        let mis = Mis { max_depth: 10 };
        let (n, m) = (avg_floor(&naive, 8000), avg_floor(&mis, 8000));
        assert!(n > 0.0 && m > 0.0, "both lit: naive={n} mis={m}");
        let rel = (n - m).abs() / m;
        assert!(rel < 0.05, "naive and mis should agree in mean: naive={n} mis={m} rel={rel}");
    }
}
