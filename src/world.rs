use crate::color::Color;
use crate::integrator::Sky;
use crate::interval::Interval;
use crate::material::Material;
use crate::ray::{AreaLight, GeoHit, HitRecord, Intersect, Ray, BVH, AABB};
use crate::texture::env_map::EnvMap;
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

/// A borrowed view of one directly-sampleable light, derived on the fly from the
/// World's objects and sky rather than stored in a parallel list: either an
/// emissive surface (an [`AreaLight`]) or the environment map. Both answer "a
/// direction toward me and its solid-angle pdf" — the env light's pdf is
/// position-independent (it is infinitely far away), so it ignores the origin.
enum LightRef<'a> {
    Area(&'a dyn AreaLight),
    Env(&'a EnvMap),
}

impl LightRef<'_> {
    /// Solid-angle pdf of sampling `dir` (from `origin`) toward this light.
    fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32 {
        match self {
            LightRef::Area(l) => l.pdf_value(origin, dir),
            LightRef::Env(env) => env.direction_pdf(&dir),
        }
    }

    /// A (unnormalized) direction from `origin` toward this light, using the two
    /// canonical uniforms `(u, v)`. The env sampler's own pdf is recomputed by
    /// [`World::light_pdf`] (the marginal over all lights), so it is dropped here.
    fn sample_dir(&self, origin: Point3, u: f32, v: f32) -> Vec3 {
        match self {
            LightRef::Area(l) => l.sample_dir(origin, u, v),
            LightRef::Env(env) => env.sample_direction(u, v).0,
        }
    }
}

/// A lightweight proxy the top-level BVH is built over: an object's geometry
/// handle paired with the *stable* index of that object in [`World::objects`].
/// The BVH reorders proxies freely for spatial locality; the object array never
/// moves — it stays in scene order, the source of truth for materials and
/// light-sampling order. On a hit, the proxy's `object` index resolves the
/// object whose material is attached. Intersection and bounds delegate to the
/// shared geometry handle, so no geometry is copied into the proxy.
struct ObjRef {
    geometry: Arc<dyn Intersect>,
    object: usize,
}

impl Intersect for ObjRef {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<GeoHit> {
        self.geometry.intersect(ray, ray_t)
    }

    fn bounding_box(&self) -> &AABB {
        self.geometry.bounding_box()
    }

    fn center(&self) -> Vec3 {
        self.geometry.center()
    }
}

/// The built, acceleration-backed runtime structure the path tracer walks: the
/// scene's geometry plus the scene-global state the integrator needs — the
/// directly-sampleable lights and the [`Sky`] seen on a miss. Produced from a
/// `Scene` by `build_world`.
///
/// Lights have one source of truth: an emissive [`Object`]'s own `light` handle
/// (tracked by index in `lights`), plus the environment map when `sky` is one.
/// Intersection is accelerated by a top-level BVH over object proxies (the outer
/// half of a two-level TLAS/BLAS structure — each mesh keeps its own inner
/// triangle BVH, reused by reference). Distinct from
/// [`IntersectGroup`](crate::group::IntersectGroup), a plain list of hittables
/// used *inside* objects; the World is the top level and is not itself an
/// `Intersect`.
pub struct World {
    pub objects: Vec<Object>,
    /// Indices into `objects` of the sampleable emitters, in scene order — the
    /// discrete set next-event estimation chooses among (alongside the env light,
    /// when `sky` is an environment map). Derived from the objects at build time.
    lights: Vec<usize>,
    /// The radiance a ray sees when it hits nothing. Owned by the World so its
    /// miss-shading and its light-sampling (when it's an environment map) share
    /// one source of truth — the env map lives here, not in a second light list.
    pub sky: Sky,
    /// Top-level BVH over [`ObjRef`] proxies of `objects`. Each proxy carries its
    /// object's stable scene-order index, so the BVH may reorder proxies for
    /// locality without disturbing `objects`. Its root bounds the whole World.
    bvh: BVH<ObjRef>,
}

impl World {
    /// Build the runtime World from a complete set of objects (in scene order)
    /// plus the sky. Everything derived from the objects is computed once here —
    /// the light index set and the top-level BVH over object proxies — because a
    /// BVH cannot be cheaply appended to, which also makes a built World
    /// immutable. `objects` keeps scene order (the source of truth for materials
    /// and light-sampling order); only the BVH's proxies are reordered.
    pub fn new(objects: Vec<Object>, sky: Sky) -> Self {
        // A sampleable emitter is a light — derived from the objects, one place,
        // no parallel list to keep in sync.
        let lights = objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.light.is_some())
            .map(|(i, _)| i)
            .collect();
        let proxies = objects
            .iter()
            .enumerate()
            .map(|(i, o)| ObjRef {
                geometry: o.geometry.clone(),
                object: i,
            })
            .collect();
        World {
            objects,
            lights,
            sky,
            bvh: BVH::build(proxies),
        }
    }

    /// Bounds of the whole World — the top-level BVH's root AABB.
    pub fn bounding_box(&self) -> &AABB {
        self.bvh.bounding_box()
    }

    /// Closest hit along `ray` within `ray_t`, or `None` on a miss. The top-level
    /// BVH returns the winning object proxy; the proxy's index resolves that
    /// object, whose material is attached here to produce the shading
    /// [`HitRecord`] — the single material-attach seam.
    pub fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        self.bvh.closest_hit(ray, ray_t).map(|(geo, proxy)| {
            HitRecord::from_geo(geo, self.objects[proxy.object].material.as_ref())
        })
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

    /// Every directly-sampleable light as a borrowed [`LightRef`], in the order
    /// `light_count` counts them: the registered emissive objects first, then the
    /// environment map when `sky` is one. Derived on the fly — no stored list.
    fn light_refs(&self) -> impl Iterator<Item = LightRef<'_>> {
        self.lights
            .iter()
            .map(|&i| {
                let l = self.objects[i]
                    .light
                    .as_deref()
                    .expect("lights indexes only sampleable emitters");
                LightRef::Area(l)
            })
            .chain(match &self.sky {
                Sky::Env(env) => Some(LightRef::Env(env)),
                Sky::Flat(_) => None,
            })
    }

    /// Average solid-angle PDF of sampling `dir` (from `origin`) toward any
    /// registered light. 0 when there are no lights.
    pub fn light_pdf(&self, origin: Point3, dir: Vec3) -> f32 {
        let n = self.light_count();
        if n == 0 {
            return 0.0;
        }
        let sum: f32 = self.light_refs().map(|l| l.pdf_value(origin, dir)).sum();
        sum / n as f32
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
        let n = self.light_count();
        if n == 0 {
            return None;
        }
        let i = rng.random_range(0..n);
        let light = self.light_refs().nth(i).expect("i < light_count");
        Some(light.sample_dir(origin, u, v))
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
        let w = World::new(vec![], Sky::Flat(Color::ZERO));
        assert_eq!(w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn light_pdf_averages_single_light() {
        let w = World::new(vec![light_object()], Sky::Flat(Color::ZERO));
        // One light => average == that light's pdf_value; analytic value is 1.0.
        let p = w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        assert!((p - 1.0).abs() < 1e-5, "p={p}");
    }

    #[test]
    fn sample_light_dir_is_none_without_lights() {
        let w = World::new(vec![], Sky::Flat(Color::ZERO));
        let mut rng = SmallRng::seed_from_u64(1);
        assert!(w.sample_light_dir(Point3::new(0.0, 0.0, 0.0), 0.5, 0.5, &mut rng).is_none());
    }

    #[test]
    fn sample_light_dir_points_toward_the_light() {
        let w = World::new(vec![light_object()], Sky::Flat(Color::ZERO));
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
        let w = World::new(vec![], Sky::Env(env.clone()));
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
        let w = World::new(
            vec![
                emitter(1.0, Color::new(3.0, 0.0, 0.0)), // red at y=1
                emitter(5.0, Color::new(0.0, 7.0, 0.0)), // green at y=5
            ],
            Sky::Flat(Color::ZERO),
        );
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
        let wr = World::new(vec![red], Sky::Flat(Color::ZERO));
        let wg = World::new(vec![green], Sky::Flat(Color::ZERO));
        let hr = wr.intersect(&up, &ti).unwrap();
        let hg = wg.intersect(&up, &ti).unwrap();
        // The shared geometry, two different resolved materials.
        assert_eq!(hr.material.emitted(hr.u, hr.v, hr.p), Color::new(3.0, 0.0, 0.0));
        assert_eq!(hg.material.emitted(hg.u, hg.v, hg.p), Color::new(0.0, 7.0, 0.0));
    }
}

#[cfg(test)]
mod top_bvh_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Sphere;
    use crate::material::Lambertian;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    /// A brute-force linear scan over the public object list — the closest hit's
    /// distance and the index of the object that produced it. This is the
    /// independent source of truth the BVH-accelerated `intersect` must match; no
    /// such linear path is kept in production.
    fn linear_closest(world: &World, ray: &Ray, ray_t: &Interval) -> Option<(f32, usize)> {
        let mut best: Option<(f32, usize)> = None;
        let mut closest_t = ray_t.max;
        for (i, obj) in world.objects.iter().enumerate() {
            if let Some(geo) = obj
                .geometry
                .intersect(ray, &Interval::new(ray_t.min, closest_t))
            {
                closest_t = geo.t;
                best = Some((geo.t, i));
            }
        }
        best
    }

    #[test]
    fn intersect_matches_a_brute_force_scan() {
        // Several spheres spread through space, each with its own material so the
        // resolved material pins down *which* object a ray hit.
        let centers = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 1.0, -2.0),
            Point3::new(-2.5, -1.0, 1.5),
            Point3::new(1.0, -2.0, 3.0),
            Point3::new(-1.5, 2.0, -3.0),
            Point3::new(2.0, 2.5, 2.0),
        ];
        let objects: Vec<Object> = centers
            .iter()
            .enumerate()
            .map(|(i, c)| Object {
                geometry: Arc::new(Sphere::stationary(*c, 0.8)),
                material: Arc::new(Lambertian::from_color(Color::new(0.1 * i as f32, 0.2, 0.3))),
                light: None,
            })
            .collect();
        let world = World::new(objects, Sky::Flat(Color::ZERO));

        let ti = Interval::new(0.001, f32::INFINITY);
        let mut rng = SmallRng::seed_from_u64(0xB57);
        let mut hits = 0;
        for _ in 0..500 {
            // Origin on a shell around the cluster, aimed at a jittered point
            // inside it — a healthy mix of hits and misses.
            let origin = Point3::new(
                rng.random_range(-8.0..8.0),
                rng.random_range(-8.0..8.0),
                rng.random_range(-8.0..8.0),
            );
            let target = Point3::new(
                rng.random_range(-3.0..3.0),
                rng.random_range(-3.0..3.0),
                rng.random_range(-3.0..3.0),
            );
            let ray = Ray::new(origin, target - origin);

            let bvh = world.intersect(&ray, &ti);
            let lin = linear_closest(&world, &ray, &ti);
            assert_eq!(bvh.is_some(), lin.is_some(), "hit/miss disagree");
            if let (Some(hr), Some((lt, li))) = (bvh, lin) {
                hits += 1;
                // Both paths call the same geometry, so the distance is exact.
                assert_eq!(hr.t, lt, "closest distance disagrees");
                assert!(
                    std::ptr::eq(hr.material, world.objects[li].material.as_ref()),
                    "resolved material disagrees at object {li}"
                );
            }
        }
        assert!(hits > 50, "expected a healthy mix of hits, got {hits}");
    }
}
