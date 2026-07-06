use crate::color::Color;
use crate::integrator::Sky;
use crate::interval::Interval;
use crate::material::Material;
use crate::ray::{AreaLight, GeoHit, HitRecord, Intersect, Ray, AABB};
use crate::vec3::{Point3, Vec3};
use rand::rngs::SmallRng;
use rand::Rng;
use std::sync::Arc;

/// A placed, shadeable item in the World: a piece of material-agnostic geometry
/// paired with the one material bound to it. The geometry (its transform already
/// baked in) reports a [`GeoHit`]; the World attaches this `material` to produce
/// the shading [`HitRecord`]. Mirrors the document's `ObjectSpec { shape,
/// material, transform }`, so reusing a mesh means sharing its `geometry` handle
/// across objects with different materials — no per-object geometry rebuild.
///
/// An emissive object that can be sampled exactly also carries its `light`
/// handle, so it is the *single* registration for both its geometry and its
/// next-event-estimation role — the World derives its light set from the objects
/// that have one, with no parallel light list.
pub struct Object {
    pub geometry: Arc<dyn Intersect>,
    pub material: Arc<dyn Material>,
    /// `Some` when this object is a next-event-estimation light — a directly
    /// sampleable emitter, the same underlying primitive as `geometry`. `None`
    /// for ordinary surfaces and for emitters we can't sample exactly (ellipsoid
    /// / box / mesh — they still glow when hit, just aren't shadow-ray sampled).
    pub light: Option<Arc<dyn AreaLight>>,
}

/// The built, acceleration-backed runtime structure the path tracer walks: the
/// scene's geometry plus the scene-global state the integrator needs — the
/// directly-sampleable lights and the [`Sky`] seen on a miss. Produced from a
/// `Scene` by `build_world`.
///
/// Lights have one source of truth: an emissive [`Object`]'s own `light` handle
/// (tracked by index in `lights`), plus the environment map when `sky` is one.
/// Distinct from [`IntersectGroup`](crate::group::IntersectGroup), a plain list
/// of hittables used *inside* objects; the World is the top level and is not
/// itself an `Intersect`.
pub struct World {
    pub objects: Vec<Object>,
    /// Indices into `objects` of the sampleable emitters, in registration order —
    /// the discrete set next-event estimation chooses among (alongside the env
    /// light, when `sky` is an environment map). Derived as objects are added.
    lights: Vec<usize>,
    /// The radiance a ray sees when it hits nothing. Owned by the World so its
    /// miss-shading and its light-sampling (when it's an environment map) share
    /// one source of truth — the env map lives here, not in a second light list.
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

    pub fn add(&mut self, object: Object) -> &mut Self {
        self.bbox = AABB::from_boxes(&self.bbox, object.geometry.bounding_box());
        // A sampleable emitter registers itself as a light — one place, derived
        // from the object, no parallel list to keep in sync.
        if object.light.is_some() {
            self.lights.push(self.objects.len());
        }
        self.objects.push(object);
        self
    }

    pub fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    /// Closest hit along `ray` within `ray_t`, or `None` on a miss. Geometry
    /// reports a material-agnostic [`GeoHit`]; the winning object's material is
    /// attached here to produce the shading [`HitRecord`].
    pub fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let mut closest: Option<(GeoHit, &Arc<dyn Material>)> = None;
        let mut closest_t = ray_t.max;

        for object in &self.objects {
            if let Some(geo) = object
                .geometry
                .intersect(ray, &Interval::new(ray_t.min, closest_t))
            {
                closest_t = geo.t;
                closest = Some((geo, &object.material));
            }
        }

        closest.map(|(geo, material)| HitRecord::from_geo(geo, material.as_ref()))
    }

    /// Radiance seen along `dir` by a ray that hit nothing.
    pub fn sky_radiance(&self, dir: &Vec3) -> Color {
        self.sky.radiance(dir)
    }

    /// How many directly-sampleable lights there are: the emissive objects plus
    /// the environment map (when `sky` is one).
    pub fn light_count(&self) -> usize {
        self.lights.len() + matches!(self.sky, Sky::Env(_)) as usize
    }

    /// The objects registered as sampleable area lights, in registration order.
    pub fn area_light_objects(&self) -> impl Iterator<Item = &Object> {
        self.lights.iter().map(|&i| &self.objects[i])
    }

    /// The area-light handle of the `k`th registered emitter. Every index in
    /// `self.lights` points at an object with `light: Some`, so this can't fail.
    fn area_light(&self, k: usize) -> &Arc<dyn AreaLight> {
        self.objects[self.lights[k]]
            .light
            .as_ref()
            .expect("lights indexes only sampleable emitters")
    }

    /// Average solid-angle PDF of sampling `dir` (from `origin`) toward any
    /// registered light. 0 when there are no lights. The env light (if any) is
    /// the last light in the set.
    pub fn light_pdf(&self, origin: Point3, dir: Vec3) -> f32 {
        let n = self.light_count();
        if n == 0 {
            return 0.0;
        }
        let mut sum: f32 = (0..self.lights.len())
            .map(|k| self.area_light(k).pdf_value(origin, dir))
            .sum();
        if let Sky::Env(env) = &self.sky {
            sum += env.direction_pdf(&dir);
        }
        sum / n as f32
    }

    /// A (unnormalized) direction from `origin` toward a registered light: `rng`
    /// chooses which light (discrete), and the canonical uniforms `(u, v)` sample
    /// a point on its surface — so the surface sample can be stratified by the
    /// caller. `None` when there are no lights. The env light is the last slot.
    pub fn sample_light_dir(
        &self,
        origin: Point3,
        u: f32,
        v: f32,
        rng: &mut SmallRng,
    ) -> Option<Vec3> {
        let n = self.light_count();
        if n == 0 {
            return None;
        }
        let i = rng.random_range(0..n);
        Some(if i < self.lights.len() {
            self.area_light(i).sample_dir(origin, u, v)
        } else {
            // The env light: (u, v) are the two canonical uniforms; the env
            // sampler's own pdf is recomputed by `light_pdf` (the marginal).
            match &self.sky {
                Sky::Env(env) => env.sample_direction(u, v).0,
                Sky::Flat(_) => unreachable!("env light slot without an env sky"),
            }
        })
    }
}

#[cfg(test)]
mod light_mixture_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::DiffuseLight;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    // An overhead quad light on the y=2 plane, area 4 (pdf_value((0,0,0),(0,2,0))
    // == 1.0), registered as an Object that owns its area-light role.
    fn light_object() -> Object {
        let quad = Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ));
        Object {
            geometry: quad.clone(),
            material: Arc::new(DiffuseLight::from_color(Color::new(1.0, 1.0, 1.0))),
            light: Some(quad),
        }
    }

    #[test]
    fn light_pdf_is_zero_without_lights() {
        let w = World::new();
        assert_eq!(w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn light_pdf_averages_single_light() {
        let mut w = World::new();
        w.add(light_object());
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
        w.add(light_object());
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
        w.sky = Sky::Env(env.clone());
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

#[cfg(test)]
mod material_ownership_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::DiffuseLight;
    use crate::vec3::{Point3, Vec3};
    use std::sync::Arc;

    // A horizontal quad on the `y` plane (x,z ∈ [-1,1]) as an Object that emits
    // `c` — the emission colour is a readable proxy for "which material got
    // attached at the hit".
    fn emitter(y: f32, c: Color) -> Object {
        Object {
            geometry: Arc::new(Quad::new(
                Point3::new(-1.0, y, -1.0),
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 2.0),
            )),
            material: Arc::new(DiffuseLight::from_color(c)),
            light: None,
        }
    }

    /// The core new behaviour: the World attaches the *hit object's* material to
    /// the shading record — including picking the nearer object when two overlap
    /// on the ray.
    #[test]
    fn world_attaches_the_hit_objects_material_closest_first() {
        let mut w = World::new();
        w.add(emitter(1.0, Color::new(3.0, 0.0, 0.0))); // red at y=1
        w.add(emitter(5.0, Color::new(0.0, 7.0, 0.0))); // green at y=5
        let ti = Interval::new(0.001, f32::INFINITY);

        // Fired up from the origin: the y=1 (red) object is the closest hit.
        let up = Ray::new(Point3::ZERO, Vec3::new(0.0, 1.0, 0.0));
        let hit = w.intersect(&up, &ti).expect("hits the lower object");
        assert_eq!(
            hit.material.emitted(hit.u, hit.v, hit.p),
            Color::new(3.0, 0.0, 0.0),
            "closest object's (red) material must be attached"
        );

        // Fired down from above both: the y=5 (green) object is now closest.
        let down = Ray::new(Point3::new(0.0, 6.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let hit = w.intersect(&down, &ti).expect("hits the upper object");
        assert_eq!(
            hit.material.emitted(hit.u, hit.v, hit.p),
            Color::new(0.0, 7.0, 0.0),
            "closest object's (green) material must be attached"
        );
    }

    /// The win that made `MaterialOverride` and the placeholder unnecessary: one
    /// material-agnostic geometry handle, shared by reference across objects that
    /// each resolve their own material — no per-object geometry rebuild.
    #[test]
    fn one_geometry_handle_backs_objects_with_different_materials() {
        let geometry: Arc<dyn Intersect> = Arc::new(Quad::new(
            Point3::new(-1.0, 1.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ));
        let red = Object {
            geometry: geometry.clone(),
            material: Arc::new(DiffuseLight::from_color(Color::new(3.0, 0.0, 0.0))),
            light: None,
        };
        let green = Object {
            geometry: geometry.clone(),
            material: Arc::new(DiffuseLight::from_color(Color::new(0.0, 7.0, 0.0))),
            light: None,
        };
        // Same allocation — the geometry was not rebuilt for the second object.
        assert!(Arc::ptr_eq(&red.geometry, &green.geometry));

        let up = Ray::new(Point3::ZERO, Vec3::new(0.0, 1.0, 0.0));
        let ti = Interval::new(0.001, f32::INFINITY);
        let (mut wr, mut wg) = (World::new(), World::new());
        wr.add(red);
        wg.add(green);
        let hr = wr.intersect(&up, &ti).unwrap();
        let hg = wg.intersect(&up, &ti).unwrap();
        // The shared geometry, two different resolved materials.
        assert_eq!(hr.material.emitted(hr.u, hr.v, hr.p), Color::new(3.0, 0.0, 0.0));
        assert_eq!(hg.material.emitted(hg.u, hg.v, hg.p), Color::new(0.0, 7.0, 0.0));
    }
}
