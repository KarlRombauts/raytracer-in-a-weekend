use std::sync::Arc;

use crate::geometry::Sphere;
use crate::integrator::Sky;
use crate::ray::{AreaLight, Intersect};
use crate::texture::env_map::load_cached;
use crate::world::{Object, World};

use super::{placed_quad, MaterialSpec, Placement, Scene, Shape, Transform};

/// One baked emissive primitive, viewed both as world geometry (for
/// intersection) and as a sampleable `AreaLight` (for next-event estimation).
/// Both handles share a single allocation.
struct BakedLight {
    intersect: Arc<dyn Intersect>,
    light: Arc<dyn AreaLight>,
}

/// Wrap a concrete primitive once and expose it as both an `Intersect` and an
/// `AreaLight`. The two `Arc`s coerce from the same `Arc<T>`, so there is one
/// underlying surface, not two.
fn bake<T: AreaLight + 'static>(prim: T) -> BakedLight {
    let arc = Arc::new(prim);
    BakedLight {
        intersect: arc.clone(),
        light: arc,
    }
}

/// Bake an emissive object's transform directly into a concrete, exactly
/// sampleable primitive, so a *transformed* light still gets next-event
/// estimation. Returns `None` for geometry we can't sample exactly — a
/// non-uniformly scaled sphere (an ellipsoid), a Box, or a Mesh — which then
/// deliberately falls back to BSDF-only illumination.
fn bake_area_light(shape: &Shape, transform: &Transform) -> Option<BakedLight> {
    match shape {
        Shape::Quad { q, u, v } => Some(bake(placed_quad(*q, *u, *v, transform))),
        Shape::Sphere { center, radius } => {
            // A sphere stays a sphere only under uniform scale; a non-uniform
            // scale makes an ellipsoid we can't sample as a Sphere. Rotation
            // leaves a sphere fixed about its own centre, so it maps to
            // `center + translate` with the radius scaled uniformly.
            let s = transform.scale;
            let uniform = (s.x - s.y).abs() < 1e-6 && (s.y - s.z).abs() < 1e-6;
            if !uniform {
                return None;
            }
            let world_center = Placement::new(transform, *center).point(*center);
            Some(bake(Sphere::stationary(world_center, *radius * s.x)))
        }
        // Box (6 quads) and Mesh (a BVH of triangles) need group/area-weighted
        // sampling — out of scope for now, they stay BSDF-only.
        Shape::Box { .. } | Shape::Mesh { .. } => None,
    }
}

/// Assemble the renderable world from the scene description. Cheap enough to
/// call on every edit (Mesh handles are shared, not rebuilt). Emissive objects
/// that can be sampled exactly also carry their `light` handle, so the World
/// registers them once for both roles.
pub fn build_world(scene: &Scene) -> World {
    let mut objects = Vec::new();
    for obj in &scene.objects {
        if obj.hidden {
            continue;
        }
        if let MaterialSpec::DiffuseLight { .. } = &obj.material {
            // Bake the transform into an exactly-sampleable primitive. On success
            // the one baked surface is registered *once*, as an Object that is
            // both the world geometry and — via its `light` handle — the
            // next-event-estimation light.
            if let Some(baked) = bake_area_light(&obj.shape, &obj.transform) {
                objects.push(Object {
                    geometry: baked.intersect,
                    material: obj.material.build(),
                    light: Some(baked.light),
                });
                continue;
            }
            // Deliberate, narrow fallback (was a silent blanket drop before the
            // AreaLight split): ellipsoid / Box / Mesh lights still glow when hit
            // directly, they're just not shadow-ray sampled.
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!(
                "light '{}' can't be area-sampled (non-uniform sphere, box, or mesh); BSDF-only",
                obj.name
            );
        }
        objects.push(Object {
            geometry: obj.build(),
            material: obj.material.build(),
            light: None,
        });
    }

    // The sky: an HDR environment map if the camera names one and it loads, else
    // the flat background. When it's an env map it is *also* a directionally
    // sampled light — the World derives that from `sky`, so it lives here only.
    let cfg = &scene.camera;
    let sky = match cfg.sky.as_deref().and_then(load_cached) {
        Some(env) => Sky::Env(env),
        None => Sky::Flat(cfg.background),
    };

    // One-pass immutable construction: the top-level BVH and light set are built
    // from the complete object list here (a BVH can't be cheaply appended to).
    World::new(objects, sky)
}

#[cfg(test)]
mod visibility_tests {
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::scene::{build_world, MaterialSpec, ObjectSpec, Scene, Shape, TextureSpec, Transform};
    use crate::vec3::{Point3, Vec3};

    fn emissive(name: &str) -> ObjectSpec {
        ObjectSpec {
            name: name.into(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 0.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 1.0, 0.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        }
    }

    #[test]
    fn hidden_object_is_excluded_from_world_and_lights() {
        let mut scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![emissive("a"), emissive("b")],
        };
        let full = build_world(&scene);
        scene.objects[1].hidden = true;
        let partial = build_world(&scene);
        // One fewer light registered when an emitter is hidden.
        assert_eq!(full.light_count(), 2);
        assert_eq!(partial.light_count(), 1);
    }
}

#[cfg(test)]
mod light_tests {
    use crate::color::Color;
    use crate::scene::build_world;
    use crate::scenes::cornell_box;
    use crate::vec3::Point3;

    #[test]
    fn cornell_box_collects_one_light() {
        let scene = cornell_box();
        let world = build_world(&scene);
        assert_eq!(world.light_count(), 1, "expected exactly one light");
        // The one registered light is an object whose material emits 15.
        let light = world.area_light_objects().next().expect("one area light");
        assert_eq!(
            light.material.emitted(0.0, 0.0, Point3::ZERO),
            Color::new(15.0, 15.0, 15.0)
        );
    }
}

#[cfg(test)]
mod registration_tests {
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::scene::{build_world, MaterialSpec, ObjectSpec, Scene, Shape, TextureSpec, Transform};
    use crate::vec3::{Point3, Vec3};

    #[test]
    fn quad_and_sphere_emitters_both_register() {
        let quad_light = ObjectSpec {
            name: "quad".to_string(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 5.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 1.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        };
        let sphere_light = ObjectSpec {
            name: "sphere".to_string(),
            shape: Shape::Sphere {
                center: Point3::new(0.0, 0.0, 0.0),
                radius: 1.0,
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        };
        let scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![quad_light, sphere_light],
        };
        let world = build_world(&scene);
        // Both the quad and the sphere have area()>0, so both register as
        // importance-sampled lights — the sphere via cone (solid-angle) sampling.
        assert_eq!(world.light_count(), 2, "quad and sphere both register");
        // Both objects still live in the world geometry too.
        assert_eq!(world.objects.len(), 2, "both objects remain in the world");
    }

    #[test]
    fn an_emitter_is_a_single_object_that_owns_its_light() {
        // One source of truth: the emitter is one Object in the world — not
        // duplicated into a parallel light list — and its next-event-estimation
        // role is carried by that same object's `light` handle.
        let quad_light = ObjectSpec {
            name: "quad".to_string(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 5.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 1.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        };
        let scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![quad_light],
        };
        let world = build_world(&scene);
        assert_eq!(world.objects.len(), 1, "one object, not duplicated across lists");
        assert_eq!(world.light_count(), 1, "and it is registered as a light");
        assert!(
            world.objects[0].light.is_some(),
            "the object itself owns its area-light handle"
        );
    }

    #[test]
    fn transformed_quad_emitter_registers() {
        // A quad light that has been rotated, non-uniformly scaled, and moved.
        // Before the AreaLight split this silently failed: build() wraps the quad
        // in Translate/Scale/Rotate decorators whose area() defaults to 0, so the
        // `area()>0` gate dropped it from next-event estimation entirely.
        let quad_light = ObjectSpec {
            name: "quad".to_string(),
            shape: Shape::Quad {
                q: Point3::new(-1.0, 0.0, -1.0),
                u: Vec3::new(2.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 2.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform {
                rotate: Vec3::new(0.0, 0.0, 45.0),
                scale: Vec3::new(1.5, 1.0, 1.0),
                translate: Vec3::new(0.0, 5.0, 0.0),
            },
            hidden: false,
        };
        let scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![quad_light],
        };
        let world = build_world(&scene);
        assert_eq!(
            world.light_count(),
            1,
            "a transformed quad emitter must still register for NEE"
        );
        assert_eq!(world.objects.len(), 1, "the emitter remains in the world");
    }

    #[test]
    fn baked_transformed_quad_light_has_analytic_pdf() {
        // Base quad on y=0 centred at the origin (area 4), scaled ×2 in x and
        // lifted to y=2. Baking must place it at y=2, spanning x∈[-2,2], z∈[-1,1]
        // with area 8. From the origin, aiming at its centre (0,2,0):
        //   dist² = 4, cos = 1, area = 8  ⇒  solid-angle pdf = 4 / (1·8) = 0.5.
        let quad_light = ObjectSpec {
            name: "quad".to_string(),
            shape: Shape::Quad {
                q: Point3::new(-1.0, 0.0, -1.0),
                u: Vec3::new(2.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 2.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform {
                rotate: Vec3::ZERO,
                scale: Vec3::new(2.0, 1.0, 1.0),
                translate: Vec3::new(0.0, 2.0, 0.0),
            },
            hidden: false,
        };
        let scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![quad_light],
        };
        let world = build_world(&scene);
        assert_eq!(world.light_count(), 1);
        let light = world.area_light_objects().next().expect("one area light");
        let geom = light.light.as_ref().expect("sampleable emitter");
        let pdf = geom.pdf_value(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        assert!(
            (pdf - 0.5).abs() < 1e-5,
            "baked non-uniform-scaled quad pdf={pdf}, expected 0.5"
        );
    }

    #[test]
    fn uniform_scaled_sphere_registers_but_ellipsoid_falls_back() {
        let emit = MaterialSpec::DiffuseLight {
            emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
        };
        let sphere = |scale: Vec3| ObjectSpec {
            name: "sphere".to_string(),
            shape: Shape::Sphere {
                center: Point3::new(0.0, 0.0, 0.0),
                radius: 1.0,
            },
            material: emit.clone(),
            transform: Transform {
                rotate: Vec3::ZERO,
                scale,
                translate: Vec3::new(0.0, 3.0, 0.0),
            },
            hidden: false,
        };
        // Uniform scale stays a sphere ⇒ bakes and registers.
        let uniform = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![sphere(Vec3::new(2.0, 2.0, 2.0))],
        };
        let w = build_world(&uniform);
        assert_eq!(w.light_count(), 1, "uniform-scaled sphere registers");
        assert_eq!(w.objects.len(), 1);

        // Non-uniform scale is an ellipsoid ⇒ deliberate BSDF-only fallback.
        let ellipsoid = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![sphere(Vec3::new(2.0, 1.0, 1.0))],
        };
        let w = build_world(&ellipsoid);
        assert_eq!(w.light_count(), 0, "non-uniform sphere falls back to BSDF-only");
        assert_eq!(w.objects.len(), 1, "the ellipsoid still lives in the world");
    }
}
