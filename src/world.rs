use crate::color::Color;
use crate::integrator::Sky;
use crate::interval::Interval;
use crate::material::Material;
use crate::ray::{AreaLight, AreaLightSample, GeoHit, HitRecord, Intersect, Ray, BVH, AABB};
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

/// A complete next-event-estimation sample of a *chosen* light: the direction
/// `wi` from the shading point toward the sampled light point, the ray-parameter
/// `t_light` at which that point lies along `wi` (∞ for the environment light, so
/// the shadow ray is unbounded), the light's emitted `radiance` there, and the
/// per-light solid-angle `pdf` `(1/n)·p_k(wi)` of this choice. Unlike
/// [`AreaLightSample`], it carries `radiance` — the World crosses the
/// geometry/material boundary the geometry can't, reading emission from the
/// object's material. This is estimator (B): the pdf is the *chosen* light's own
/// density, not the marginal over all lights.
pub struct LightSample {
    pub wi: Vec3,
    pub t_light: f32,
    pub radiance: Color,
    pub pdf: f32,
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

    fn occluded(&self, ray: &Ray, ray_t: &Interval) -> bool {
        // Delegate to the geometry's own occlusion so a mesh object's inner BVH
        // early-exits, rather than the default (which would closest-hit it).
        self.geometry.occluded(ray, ray_t)
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
            let obj = &self.objects[proxy.object];
            HitRecord::from_geo(geo, obj.material.as_ref(), obj.light.as_deref())
        })
    }

    /// Whether any object blocks `ray` within `ray_t` — a distance-bounded
    /// occlusion query for shadow rays. Early-exits on the first blocker (through
    /// the top-level BVH and any mesh object's inner BVH) and builds no shading
    /// record, so it is cheaper than [`intersect`](Self::intersect).
    pub fn occluded(&self, ray: &Ray, ray_t: &Interval) -> bool {
        self.bvh.occluded(ray, ray_t)
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

    /// Per-light solid-angle pdf of sampling `dir` (from `origin`) toward the
    /// *specific* light identified by `light` — `(1/n)·p_k(dir)`, or 0 when the
    /// hit carries no sampleable light (a plain surface or a BSDF-only emitter).
    /// The emitter-hit MIS branch calls this with [`HitRecord::light`]: the
    /// integrator holds the dumb identity token, the World owns the pdf math (the
    /// `1/n` selection and the shape's `pdf_value`). Estimator (B) — the chosen
    /// light's own density, the same `(1/n)·p` shape [`sample_light`](Self::sample_light)
    /// and [`env_pdf`](Self::env_pdf) return, so all three MIS branches share one
    /// light pdf (partition of unity). `/n` is folded inside `map_or` so a `None`
    /// light is 0, never `0.0/0`; `n ≥ 1` whenever `light` is `Some`.
    ///
    /// [`HitRecord::light`]: crate::ray::HitRecord::light
    pub fn light_pdf(&self, light: Option<&dyn AreaLight>, origin: Point3, dir: Vec3) -> f32 {
        light.map_or(0.0, |l| l.pdf_value(origin, dir) / self.light_count() as f32)
    }

    /// Per-light solid-angle pdf of the *environment* light along `dir`:
    /// `(1/n)·direction_pdf(dir)`, or 0 when the sky is not an env map. This is the
    /// env light's own density (estimator B), for the env-escape MIS branch — the
    /// same `(1/n)·p` shape [`sample_light`](Self::sample_light) returns for it, so
    /// the sampling and evaluation branches share one light pdf.
    pub fn env_pdf(&self, dir: Vec3) -> f32 {
        match &self.sky {
            Sky::Env(env) => env.direction_pdf(&dir) / self.light_count() as f32,
            Sky::Flat(_) => 0.0,
        }
    }

    /// A full next-event sample of one *chosen* light: `rng` selects a light
    /// uniformly `(1/n)`, the canonical uniforms `(u, v)` sample a point on it,
    /// and the World reads that point's emission — so the caller gets a direction,
    /// the distance to bound its shadow ray, the radiance to add if visible, and
    /// the per-light pdf to weight by. `None` when there are no lights.
    ///
    /// This is estimator (B): the returned `pdf` is the *sampled* light's own
    /// `(1/n)·p_k(wi)`, not the marginal over all lights — pairing per-light
    /// radiance with the marginal would be biased (see the PRD's correction).
    pub fn sample_light(
        &self,
        origin: Point3,
        u: f32,
        v: f32,
        rng: &mut SmallRng,
    ) -> Option<LightSample> {
        let n = self.light_count();
        if n == 0 {
            return None;
        }
        let i = rng.random_range(0..n);
        if i < self.lights.len() {
            // An area light: the geometry kernel gives direction, distance, and
            // per-surface pdf; the World crosses to the material for radiance at
            // the sampled point (`origin + wi·t_light`).
            let obj = &self.objects[self.lights[i]];
            let light = obj
                .light
                .as_deref()
                .expect("lights indexes only sampleable emitters");
            let AreaLightSample { wi, t_light, pdf } = light.sample_toward(origin, u, v);
            let radiance = obj.material.emitted(0.0, 0.0, origin + wi * t_light);
            Some(LightSample { wi, t_light, radiance, pdf: pdf / n as f32 })
        } else {
            // The environment light: infinitely far (`t_light = ∞`, unbounded
            // shadow ray), its radiance read straight from the sky. The pdf uses
            // `direction_pdf` — the same density the emitter/env-escape MIS
            // branches evaluate — so the branches share one light pdf.
            let Sky::Env(env) = &self.sky else {
                unreachable!("i >= lights.len() only when the sky is an env light")
            };
            let (wi, _) = env.sample_direction(u, v);
            let radiance = self.sky.radiance(&wi);
            let pdf = env.direction_pdf(&wi) / n as f32;
            Some(LightSample { wi, t_light: f32::INFINITY, radiance, pdf })
        }
    }
}

#[cfg(test)]
mod light_pdf_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::DiffuseLight;
    use crate::vec3::{Point3, Vec3};
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
    fn light_pdf_is_zero_for_no_light_identity() {
        // A hit carrying no sampleable light (`None`) has light-pdf 0 — a plain
        // surface or a BSDF-only emitter, which the emitter-hit MIS branch weights
        // fully to the BSDF. Holds even when the World does have lights.
        let w = World::new(vec![light_object()], Sky::Flat(Color::ZERO));
        assert_eq!(w.light_pdf(None, Point3::ZERO, Vec3::new(0.0, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn light_pdf_is_the_per_light_density() {
        let obj = light_object();
        let light = obj.light.clone().unwrap();
        let w = World::new(vec![obj], Sky::Flat(Color::ZERO));
        // One light => (1/n)·p_k with n=1 is the light's own pdf_value; the
        // analytic value straight up at the area-4 overhead quad is 1.0.
        let p = w.light_pdf(Some(light.as_ref()), Point3::ZERO, Vec3::new(0.0, 2.0, 0.0));
        assert!((p - 1.0).abs() < 1e-5, "p={p}");
    }
}

#[cfg(test)]
mod light_sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::DiffuseLight;
    use crate::texture::env_map::EnvMap;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};
    use std::sync::Arc;

    // An overhead quad light on the y=2 plane spanning x,z ∈ [-1,1] (area 4),
    // emitting a known colour so a sample's radiance is a readable literal.
    fn colored_light(emit: Color) -> Object {
        let quad = Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ));
        Object {
            geometry: quad.clone(),
            material: Arc::new(DiffuseLight::from_color(emit)),
            light: Some(quad),
        }
    }

    #[test]
    fn sample_light_is_none_without_lights() {
        let w = World::new(vec![], Sky::Flat(Color::ZERO));
        let mut rng = SmallRng::seed_from_u64(1);
        assert!(w.sample_light(Point3::ZERO, 0.5, 0.5, &mut rng).is_none());
    }

    #[test]
    fn area_light_sample_carries_the_emitters_radiance() {
        // The World crosses the geometry/material boundary: radiance is the
        // sampled light's own emission (the literal we constructed it with).
        let emit = Color::new(3.0, 5.0, 7.0);
        let w = World::new(vec![colored_light(emit)], Sky::Flat(Color::ZERO));
        let mut rng = SmallRng::seed_from_u64(2);
        for _ in 0..50 {
            let (u, v) = (rng.random::<f32>(), rng.random::<f32>());
            let s = w.sample_light(Point3::ZERO, u, v, &mut rng).unwrap();
            assert_eq!(s.radiance, emit, "radiance must be the emitter's emission");
        }
    }

    #[test]
    fn area_light_sample_point_lands_on_the_light() {
        // origin + wi·t_light is the sampled point; it must lie on the y=2 plane,
        // inside the quad's x,z ∈ [-1,1] extent.
        let w = World::new(vec![colored_light(Color::ones())], Sky::Flat(Color::ZERO));
        let mut rng = SmallRng::seed_from_u64(3);
        for _ in 0..100 {
            let (u, v) = (rng.random::<f32>(), rng.random::<f32>());
            let s = w.sample_light(Point3::ZERO, u, v, &mut rng).unwrap();
            let p = Point3::ZERO + s.wi * s.t_light;
            assert!((p.y - 2.0).abs() < 1e-5, "point off the light plane: {p:?}");
            assert!(p.x >= -1.0 - 1e-4 && p.x <= 1.0 + 1e-4, "x off the quad: {p:?}");
            assert!(p.z >= -1.0 - 1e-4 && p.z <= 1.0 + 1e-4, "z off the quad: {p:?}");
        }
    }

    #[test]
    fn area_light_sample_pdf_is_the_per_light_solid_angle_density() {
        // One light => n=1, so pdf == p_k. Cross-check the point-based pdf that
        // sample_toward returns against the independent intersection-based
        // pdf_value (two different code paths for the same solid-angle density).
        let obj = colored_light(Color::ones());
        let light = obj.light.clone().unwrap();
        let w = World::new(vec![obj], Sky::Flat(Color::ZERO));
        let mut rng = SmallRng::seed_from_u64(4);
        for _ in 0..100 {
            let (u, v) = (rng.random::<f32>(), rng.random::<f32>());
            let s = w.sample_light(Point3::ZERO, u, v, &mut rng).unwrap();
            let reference = light.pdf_value(Point3::ZERO, s.wi);
            assert!(
                (s.pdf - reference).abs() < 1e-4,
                "per-light pdf {} disagreed with pdf_value {reference}",
                s.pdf
            );
        }
    }

    #[test]
    fn area_light_sample_pdf_is_divided_by_light_count() {
        // With two lights the selected light's pdf is halved (uniform 1/n choice).
        // The chosen light is whichever the sample points at; check against that
        // light's own pdf_value.
        let a = colored_light(Color::ones());
        let la = a.light.clone().unwrap();
        // A second light on the y=-2 plane (below the origin), area 4.
        let below = Arc::new(Quad::new(
            Point3::new(-1.0, -2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ));
        let b = Object {
            geometry: below.clone(),
            material: Arc::new(DiffuseLight::from_color(Color::ones())),
            light: Some(below.clone()),
        };
        let lb: Arc<dyn AreaLight> = below;
        let w = World::new(vec![a, b], Sky::Flat(Color::ZERO));
        let mut rng = SmallRng::seed_from_u64(5);
        for _ in 0..200 {
            let (u, v) = (rng.random::<f32>(), rng.random::<f32>());
            let s = w.sample_light(Point3::ZERO, u, v, &mut rng).unwrap();
            // Above vs below picks which light was chosen.
            let chosen = if s.wi.y > 0.0 { &la } else { &lb };
            let reference = chosen.pdf_value(Point3::ZERO, s.wi) / 2.0;
            assert!(
                (s.pdf - reference).abs() < 1e-4,
                "two-light pdf {} should be pdf_value/2 = {reference}",
                s.pdf
            );
        }
    }

    // A World whose only light is an environment map with one bright texel.
    fn env_world() -> (World, Arc<EnvMap>) {
        let (ew, eh) = (8usize, 4usize);
        let mut data = vec![[0.05f32; 3]; ew * eh];
        data[ew + 6] = [80.0, 80.0, 80.0];
        let env = Arc::new(EnvMap::from_pixels(ew, eh, data));
        (World::new(vec![], Sky::Env(env.clone())), env)
    }

    #[test]
    fn env_light_sample_is_infinitely_far_and_bright() {
        // The env light is unbounded (t_light = ∞) and, because it is importance
        // sampled toward the bright texel, most samples carry bright radiance.
        let (w, _env) = env_world();
        let mut rng = SmallRng::seed_from_u64(6);
        let mut bright = 0;
        for _ in 0..1000 {
            let s = w.sample_light(Point3::ZERO, rng.random(), rng.random(), &mut rng).unwrap();
            assert!(s.t_light.is_infinite(), "env light must be infinitely far");
            if s.radiance.x > 1.0 {
                bright += 1;
            }
        }
        assert!(bright as f32 / 1000.0 > 0.9, "env sampling should carry the bright sky, {bright}/1000");
    }

    #[test]
    fn env_light_sample_pdf_matches_direction_pdf() {
        // Single env light => n=1, so the sample's pdf is the env's own
        // direction_pdf along wi (the density the MIS eval branches also use).
        let (w, env) = env_world();
        let mut rng = SmallRng::seed_from_u64(7);
        for _ in 0..100 {
            let s = w.sample_light(Point3::ZERO, rng.random(), rng.random(), &mut rng).unwrap();
            let reference = env.direction_pdf(&s.wi);
            assert!(
                (s.pdf - reference).abs() < 1e-4,
                "env pdf {} disagreed with direction_pdf {reference}",
                s.pdf
            );
        }
    }
}

#[cfg(test)]
mod hit_light_identity_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::interval::Interval;
    use crate::material::{DiffuseLight, Lambertian};
    use crate::vec3::{Point3, Vec3};
    use std::sync::Arc;

    // A horizontal quad on the y-plane spanning x,z ∈ [-1,1]. `registered` decides
    // whether it carries its area-light handle (sampleable) or is `light: None`.
    fn quad_object(y: f32, material: Arc<dyn crate::material::Material>, registered: bool) -> Object {
        let quad = Arc::new(Quad::new(
            Point3::new(-1.0, y, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ));
        Object {
            geometry: quad.clone(),
            material,
            light: registered.then(|| quad as Arc<dyn AreaLight>),
        }
    }

    #[test]
    fn intersect_tags_a_registered_area_light() {
        // A registered emissive quad overhead; the hit carries its light identity.
        let light_mat = Arc::new(DiffuseLight::from_color(Color::ones()));
        let w = World::new(vec![quad_object(2.0, light_mat, true)], Sky::Flat(Color::ZERO));
        let ray = Ray::new(Point3::ZERO, Vec3::new(0.0, 1.0, 0.0));
        let hit = w.intersect(&ray, &Interval::new(0.001, f32::INFINITY)).unwrap();
        assert!(hit.light.is_some(), "a registered area light must tag its hit");
    }

    #[test]
    fn intersect_leaves_a_plain_surface_untagged() {
        // An ordinary (non-emissive, unregistered) surface: no light identity.
        let mat = Arc::new(Lambertian::from_color(Color::new(0.7, 0.7, 0.7)));
        let w = World::new(vec![quad_object(2.0, mat, false)], Sky::Flat(Color::ZERO));
        let ray = Ray::new(Point3::ZERO, Vec3::new(0.0, 1.0, 0.0));
        let hit = w.intersect(&ray, &Interval::new(0.001, f32::INFINITY)).unwrap();
        assert!(hit.light.is_none(), "a plain surface must not be tagged");
    }

    #[test]
    fn intersect_leaves_a_bsdf_only_emitter_untagged() {
        // Emits (its material glows) but isn't registered for sampling — so the
        // emitter-hit MIS branch gets light-pdf 0 and full BSDF weight, as today.
        let light_mat = Arc::new(DiffuseLight::from_color(Color::ones()));
        let w = World::new(vec![quad_object(2.0, light_mat, false)], Sky::Flat(Color::ZERO));
        let ray = Ray::new(Point3::ZERO, Vec3::new(0.0, 1.0, 0.0));
        let hit = w.intersect(&ray, &Interval::new(0.001, f32::INFINITY)).unwrap();
        assert!(hit.light.is_none(), "an unregistered emitter must not be tagged");
    }
}

#[cfg(test)]
mod env_pdf_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::DiffuseLight;
    use crate::texture::env_map::EnvMap;
    use crate::vec3::{Point3, Vec3};
    use std::sync::Arc;

    fn env(bright_texel: usize) -> Arc<EnvMap> {
        let (ew, eh) = (8usize, 4usize);
        let mut data = vec![[0.05f32; 3]; ew * eh];
        data[bright_texel] = [80.0, 80.0, 80.0];
        Arc::new(EnvMap::from_pixels(ew, eh, data))
    }

    fn overhead_light() -> Object {
        let quad = Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ));
        Object {
            geometry: quad.clone(),
            material: Arc::new(DiffuseLight::from_color(Color::ones())),
            light: Some(quad),
        }
    }

    #[test]
    fn env_pdf_is_zero_without_an_env_light() {
        let w = World::new(vec![], Sky::Flat(Color::new(0.2, 0.4, 0.6)));
        assert_eq!(w.env_pdf(Vec3::new(0.0, 1.0, 0.0)), 0.0);
    }

    #[test]
    fn env_pdf_is_the_env_density_for_a_lone_env_light() {
        // Only light is the env => n=1, so env_pdf == the env's own direction_pdf.
        let e = env(6);
        let w = World::new(vec![], Sky::Env(e.clone()));
        let dir = Vec3::new(0.3, 0.8, 0.4);
        let reference = e.direction_pdf(&dir);
        assert!((w.env_pdf(dir) - reference).abs() < 1e-5, "env_pdf must equal direction_pdf for a lone env light");
    }

    #[test]
    fn env_pdf_is_divided_by_the_light_count() {
        // One area light + the env => n=2, so env_pdf == direction_pdf/2.
        let e = env(6);
        let w = World::new(vec![overhead_light()], Sky::Env(e.clone()));
        let dir = Vec3::new(0.3, 0.8, 0.4);
        let reference = e.direction_pdf(&dir) / 2.0;
        assert!((w.env_pdf(dir) - reference).abs() < 1e-5, "env_pdf must be direction_pdf / n");
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

#[cfg(test)]
mod occlusion_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::{Sphere, Triangle};
    use crate::material::Lambertian;
    use crate::ray::BVH;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    fn lambertian() -> Arc<dyn crate::material::Material> {
        Arc::new(Lambertian::from_color(Color::new(0.7, 0.7, 0.7)))
    }

    // A world of two sphere primitives plus one *mesh* object — a `BVH<Triangle>`
    // wrapped as geometry — so occlusion has to recurse through the top-level BVH
    // into an object's inner BVH.
    fn world_with_mesh() -> World {
        // A single triangle standing in the x = 5 plane, spanning y,z ∈ [-1,1].
        let tri = Arc::new(BVH::build(vec![Triangle::from_points(
            &Point3::new(5.0, -1.0, -1.0),
            &Point3::new(5.0, 1.5, 0.0),
            &Point3::new(5.0, -1.0, 1.5),
        )])) as Arc<dyn Intersect>;
        World::new(
            vec![
                // Off the +x axis, so a ray fired down it can only meet the mesh.
                Object { geometry: Arc::new(Sphere::stationary(Point3::new(0.0, 3.0, 0.0), 0.8)), material: lambertian(), light: None },
                Object { geometry: Arc::new(Sphere::stationary(Point3::new(-3.0, -2.0, 2.0), 0.6)), material: lambertian(), light: None },
                Object { geometry: tri, material: lambertian(), light: None },
            ],
            Sky::Flat(Color::ZERO),
        )
    }

    #[test]
    fn occluded_matches_intersect_over_random_rays() {
        let world = world_with_mesh();
        let mut rng = SmallRng::seed_from_u64(0x0CC17);
        for _ in 0..800 {
            let origin = Point3::new(
                rng.random_range(-8.0..8.0),
                rng.random_range(-8.0..8.0),
                rng.random_range(-8.0..8.0),
            );
            let target = Point3::new(
                rng.random_range(-1.0..6.0),
                rng.random_range(-3.0..3.0),
                rng.random_range(-3.0..3.0),
            );
            let ray = Ray::new(origin, target - origin);
            let ti = Interval::new(0.001, rng.random_range(0.3..1.2));
            assert_eq!(
                world.occluded(&ray, &ti),
                world.intersect(&ray, &ti).is_some(),
                "World::occluded disagreed with closest-hit"
            );
        }
    }

    #[test]
    fn occlusion_reaches_into_a_mesh_object() {
        let world = world_with_mesh();
        // A ray from the origin toward the triangle's interior (hit at t = 1). It
        // clears both spheres, so only the mesh object can occlude it — proving the
        // inner BVH's any-hit path is reached.
        let to_tri = Point3::new(5.0, -0.2, 0.2) - Point3::ZERO;
        let at_mesh = Ray::new(Point3::ZERO, to_tri);
        assert!(world.occluded(&at_mesh, &Interval::new(0.001, f32::INFINITY)), "mesh triangle should occlude");
        // Bounded short of the triangle (t=1): no longer occluded (distance works).
        assert!(!world.occluded(&at_mesh, &Interval::new(0.001, 0.5)), "triangle beyond t_max must not occlude");
    }
}
