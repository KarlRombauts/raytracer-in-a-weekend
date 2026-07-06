use crate::color::Color;
use crate::integrator::Sky;
use crate::interval::Interval;
use crate::ray::{AreaLight, Intersect, Ray, AABB, HitRecord};
use crate::texture::env_map::EnvMap;
use crate::vec3::{Point3, Vec3};
use rand::rngs::SmallRng;
use rand::Rng;
use std::sync::Arc;

/// A light the integrator can sample directly (next-event estimation): either an
/// emissive surface (an [`AreaLight`] plus its constant emission) or the
/// environment map, sampled by direction. Both answer "a direction toward me and
/// its solid-angle pdf" — the env light's pdf is position-independent (it is
/// infinitely far away), so it ignores the shading origin.
pub enum Light {
    Area { geom: Arc<dyn AreaLight>, emit: Color },
    Env(Arc<EnvMap>),
}

/// The built, acceleration-backed runtime structure the path tracer walks: the
/// scene's geometry plus the scene-global state the integrator needs — the set
/// of directly-sampleable [`Light`]s and the [`Sky`] seen on a miss. Produced
/// from a `Scene` by `build_world`.
///
/// Distinct from [`IntersectGroup`](crate::group::IntersectGroup), which is a
/// plain list of hittables used *inside* objects (boxes, meshes); the World is
/// the top level and is not itself an `Intersect`.
pub struct World {
    pub objects: Vec<Arc<dyn Intersect>>,
    pub lights: Vec<Light>,
    /// The radiance a ray sees when it hits nothing. Owned by the World so its
    /// miss-shading and its light-sampling (when it's an environment map) share
    /// one source of truth.
    pub sky: Sky,
    bbox: AABB,
}

impl World {
    pub fn new() -> Self {
        World {
            objects: Vec::new(),
            lights: Vec::new(),
            sky: Sky::Flat(Color::ZERO),
            bbox: AABB::EMPTY,
        }
    }

    pub fn add(&mut self, object: Arc<dyn Intersect>) -> &mut Self {
        self.bbox = AABB::from_boxes(&self.bbox, object.bounding_box());
        self.objects.push(object);
        self
    }

    pub fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    /// Closest hit along `ray` within `ray_t`, or `None` on a miss.
    pub fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let mut closest_hit: Option<HitRecord> = None;
        let mut closest_t = ray_t.max;

        for object in &self.objects {
            if let Some(hit_record) = object.intersect(ray, &Interval::new(ray_t.min, closest_t)) {
                closest_t = hit_record.t as f32;
                closest_hit = Some(hit_record);
            }
        }

        closest_hit
    }

    /// Radiance seen along `dir` by a ray that hit nothing.
    pub fn sky_radiance(&self, dir: &Vec3) -> Color {
        self.sky.radiance(dir)
    }

    /// Average solid-angle PDF of sampling `dir` (from `origin`) toward any
    /// registered light. 0 when there are no lights.
    pub fn light_pdf(&self, origin: Point3, dir: Vec3) -> f32 {
        if self.lights.is_empty() {
            return 0.0;
        }
        let sum: f32 = self
            .lights
            .iter()
            .map(|l| match l {
                Light::Area { geom, .. } => geom.pdf_value(origin, dir),
                Light::Env(env) => env.direction_pdf(&dir),
            })
            .sum();
        sum / self.lights.len() as f32
    }

    /// A (unnormalized) direction from `origin` toward a registered light: `rng`
    /// chooses which light (discrete), and the canonical uniforms `(u, v)` sample
    /// a point on its surface — so the surface sample can be stratified by the
    /// caller. `None` when there are no lights.
    pub fn sample_light_dir(
        &self,
        origin: Point3,
        u: f32,
        v: f32,
        rng: &mut SmallRng,
    ) -> Option<Vec3> {
        if self.lights.is_empty() {
            return None;
        }
        let i = rng.random_range(0..self.lights.len());
        Some(match &self.lights[i] {
            Light::Area { geom, .. } => geom.sample_dir(origin, u, v),
            // (u, v) are the two canonical uniforms; the env sampler's own pdf is
            // recomputed by `light_pdf` (the marginal over all lights).
            Light::Env(env) => env.sample_direction(u, v).0,
        })
    }
}

#[cfg(test)]
mod light_mixture_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    // Overhead quad light on the y=2 plane, area 4 (same as the analytic
    // pdf_value setup: pdf_value((0,0,0),(0,2,0)) == 1.0).
    fn overhead_light() -> Arc<dyn AreaLight> {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        ))
    }

    #[test]
    fn light_pdf_is_zero_without_lights() {
        let w = World::new();
        assert_eq!(w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn light_pdf_averages_single_light() {
        let mut w = World::new();
        w.lights.push(Light::Area { geom: overhead_light(), emit: Color::new(1.0, 1.0, 1.0) });
        // One light => average == that light's pdf_value; analytic value is 1.0.
        let p = w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        assert!((p - 1.0).abs() < 1e-5, "p={p}");
    }

    #[test]
    fn sample_light_dir_is_none_without_lights() {
        let w = World::new();
        let mut rng = SmallRng::seed_from_u64(1);
        assert!(w.sample_light_dir(Point3::new(0.0, 0.0, 0.0), 0.5, 0.5, &mut rng).is_none());
    }

    #[test]
    fn sample_light_dir_points_toward_the_light() {
        let mut w = World::new();
        w.lights.push(Light::Area { geom: overhead_light(), emit: Color::new(1.0, 1.0, 1.0) });
        let mut rng = SmallRng::seed_from_u64(2);
        for _ in 0..100 {
            let (u, v) = (rng.random::<f32>(), rng.random::<f32>());
            let d = w.sample_light_dir(Point3::new(0.0, 0.0, 0.0), u, v, &mut rng).unwrap();
            assert!(d.y > 0.0, "expected upward dir toward overhead light, got {:?}", d);
        }
    }

    #[test]
    fn env_light_participates_in_light_sampling() {
        use crate::texture::env_map::EnvMap;
        // A World whose only light is an environment map with one bright texel.
        let (ew, eh) = (8usize, 4usize);
        let mut data = vec![[0.05f32; 3]; ew * eh];
        data[ew + 6] = [80.0, 80.0, 80.0];
        let env = Arc::new(EnvMap::from_pixels(ew, eh, data));
        let mut w = World::new();
        w.lights.push(Light::Env(env.clone()));
        let origin = Point3::new(0.0, 0.0, 0.0);

        // A single env light => the World's light_pdf is exactly the env's pdf.
        let (dir, _) = env.sample_direction(0.3, 0.7);
        assert!(
            (w.light_pdf(origin, dir) - env.direction_pdf(&dir)).abs() < 1e-4,
            "world light_pdf should equal the env pdf for a single env light"
        );

        // Sampling the World's lights points at the bright sky (read the direction
        // back through the env's radiance lookup).
        let mut rng = SmallRng::seed_from_u64(4);
        let mut bright = 0;
        for _ in 0..1000 {
            let d = w.sample_light_dir(origin, rng.random(), rng.random(), &mut rng).unwrap();
            if env.sample(&d).x > 1.0 {
                bright += 1;
            }
        }
        assert!(bright as f32 / 1000.0 > 0.9, "env light sampling should point at the bright sky, {bright}/1000");
    }
}
