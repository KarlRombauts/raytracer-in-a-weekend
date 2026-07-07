//! BVH benchmark harness (see `.scratch/bvh-perf/PRD.md`).
//!
//! - `traversal/<mesh>/<orientation>` — ns/ray firing a fixed grid of parallel
//!   rays through a `BVH<Triangle>` from a given direction. Per-orientation so an
//!   ordering/split-axis regression on an anisotropic mesh shows up as one
//!   orientation blowing up rather than hiding in the mean.
//! - `build/<mesh>` — time `BVH::build` on the mesh's triangles.
//! - `top_level` — traversal through a `World`'s top-level `BVH<ObjRef>` over many
//!   synthetic spheres (real scenes have too few top-level objects to time).
//! - `render/dragon` — a small, single-threaded, fixed-seed end-to-end render as
//!   the reality check that micro wins move real time.
//!
//! Timing runs with the `bvh-stats` feature OFF (zero-cost); deterministic
//! box/primitive counts come from `cargo run --example bvh_stats --features
//! bvh-stats`.

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use rand::rngs::SmallRng;
use rand::SeedableRng;

use raytracer_in_a_weekend::bench_support::{load_mesh, orientation_rays, orientations, MESHES};
use raytracer_in_a_weekend::camera::Camera;
use raytracer_in_a_weekend::color::Color;
use raytracer_in_a_weekend::geometry::{Quad, Sphere};
use raytracer_in_a_weekend::integrator::{build_integrator, Sky};
use raytracer_in_a_weekend::interval::Interval;
use raytracer_in_a_weekend::material::Lambertian;
use raytracer_in_a_weekend::ray::{Intersect, Ray, BVH};
use raytracer_in_a_weekend::sampling::SampleId;
use raytracer_in_a_weekend::vec3::{Point3, Vec3};
use raytracer_in_a_weekend::world::{Object, World};

fn traverse(bvh: &dyn Intersect, rays: &[Ray]) -> usize {
    let ti = Interval::new(0.001, f32::INFINITY);
    rays.iter().filter(|r| bvh.intersect(r, &ti).is_some()).count()
}

/// Half the bounding-sphere diagonal — the same radius `orientation_rays` uses to
/// place the ray grid, so a `t_max` expressed in these units is comparable to the
/// mesh extent.
fn bbox_radius(bb: &raytracer_in_a_weekend::ray::AABB) -> f32 {
    (bb.max_vec() - bb.min_vec()).length() * 0.5
}

fn bench_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("traversal");
    for name in MESHES {
        let (bvh, _) = load_mesh(name).build();
        for (label, dir) in orientations() {
            let rays = orientation_rays(bvh.bounding_box(), dir);
            group.throughput(criterion::Throughput::Elements(rays.len() as u64));
            group.bench_function(format!("{name}/{label}"), |b| {
                b.iter(|| traverse(bvh.as_ref(), &rays))
            });
        }
    }
    group.finish();
}

/// Shadow-ray occlusion: the any-hit `occluded` query vs the closest-hit
/// `intersect(...).is_some()` it replaced (shadow-rays Stage 2), on the SAME
/// finite-bounded rays. Both answer the identical boolean "is the segment
/// blocked?"; the ratio of the two timings is the per-shadow-ray win of the
/// occlusion path — early-exit on the *first* blocker (not the nearest) and no
/// shading record built. `t_max` is bounded just past the mesh's far face, as a
/// real shadow ray toward a light on the far side would be — that is what lets
/// `occluded` skip work `intersect` cannot.
fn bench_occlusion(c: &mut Criterion) {
    let mut group = c.benchmark_group("occlusion");
    // An oblique direction, so the rays are a realistic incoherent-ish batch
    // rather than axis-aligned; per-mesh so the size sweep is visible.
    let (_, dir) = orientations()[4];
    for name in MESHES {
        let (bvh, _) = load_mesh(name).build();
        let bb = bvh.bounding_box();
        let ti = Interval::new(0.001, bbox_radius(bb) * 4.0);
        let rays = orientation_rays(bb, dir);
        group.throughput(criterion::Throughput::Elements(rays.len() as u64));
        group.bench_function(format!("{name}/occluded"), |b| {
            b.iter(|| rays.iter().filter(|r| bvh.occluded(r, &ti)).count())
        });
        group.bench_function(format!("{name}/closest_hit"), |b| {
            b.iter(|| rays.iter().filter(|r| bvh.intersect(r, &ti).is_some()).count())
        });
    }
    group.finish();
}

/// The same occlusion A/B through a `World`'s top-level `BVH<ObjRef>` — the layer
/// a real shadow ray actually enters (`World::occluded` vs `World::intersect`).
fn bench_occlusion_top_level(c: &mut Criterion) {
    let world = sphere_world(8); // 512 objects
    let bb = world.bounding_box();
    let ti = Interval::new(0.001, bbox_radius(bb) * 4.0);
    let rays = orientation_rays(bb, Vec3::new(1.0, 0.3, 2.0).unit());
    let mut group = c.benchmark_group("occlusion_top_level");
    group.throughput(criterion::Throughput::Elements(rays.len() as u64));
    group.bench_function("spheres_512/occluded", |b| {
        b.iter(|| rays.iter().filter(|r| world.occluded(r, &ti)).count())
    });
    group.bench_function("spheres_512/closest_hit", |b| {
        b.iter(|| rays.iter().filter(|r| world.intersect(r, &ti).is_some()).count())
    });
    group.finish();
}

fn bench_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("build");
    for name in MESHES {
        let mesh = load_mesh(name);
        group.bench_function(name, |b| {
            // Fresh triangles per iteration (BVH::build consumes/reorders them);
            // triangle construction is the untimed setup.
            b.iter_batched(
                || mesh.triangles(),
                BVH::build,
                criterion::BatchSize::LargeInput,
            )
        });
    }
    group.finish();
}

/// A grid of spheres, `n³` of them, so the top-level `BVH<ObjRef>` has real work.
fn sphere_world(n: usize) -> World {
    let mut objects = Vec::with_capacity(n * n * n);
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                let c = Point3::new(i as f32 * 2.0, j as f32 * 2.0, k as f32 * 2.0);
                objects.push(Object {
                    geometry: Arc::new(Sphere::stationary(c, 0.5)),
                    material: Arc::new(Lambertian::from_color(Color::new(0.7, 0.7, 0.7))),
                    light: None,
                });
            }
        }
    }
    World::new(objects, Sky::Flat(Color::ZERO))
}

fn bench_top_level(c: &mut Criterion) {
    let world = sphere_world(8); // 512 objects
    let rays = orientation_rays(world.bounding_box(), Vec3::new(1.0, 0.3, 2.0).unit());
    let ti = Interval::new(0.001, f32::INFINITY);
    c.benchmark_group("top_level")
        .throughput(criterion::Throughput::Elements(rays.len() as u64))
        .bench_function("spheres_512", |b| {
            b.iter(|| rays.iter().filter(|r| world.intersect(r, &ti).is_some()).count())
        });
}

/// A one-object dragon `World` plus a floor, lit by a flat sky — the macro scene.
fn dragon_world() -> World {
    let (dragon, _) = load_mesh("dragon").build();
    let floor: Arc<dyn Intersect> = Arc::new(Quad::new(
        Point3::new(-500.0, 0.0, -500.0),
        Vec3::new(1000.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1000.0),
    ));
    World::new(
        vec![
            Object {
                geometry: dragon,
                material: Arc::new(Lambertian::from_color(Color::new(0.8, 0.4, 0.2))),
                light: None,
            },
            Object {
                geometry: floor,
                material: Arc::new(Lambertian::from_color(Color::new(0.8, 0.8, 0.8))),
                light: None,
            },
        ],
        Sky::Flat(Color::new(0.6, 0.7, 0.9)),
    )
}

fn bench_render(c: &mut Criterion) {
    use raytracer_in_a_weekend::camera::CameraConfig;
    let cfg = CameraConfig::builder()
        .aspect_ratio(4.0 / 3.0)
        .image_width(80)
        .samples(4)
        .max_depth(6)
        .fov(30.0)
        .look_from(Vec3::new(10.0, 8.0, 10.0))
        .look_at(Vec3::new(0.0, 2.2, 0.0))
        .build();
    let (w, h) = (80usize, 60usize);
    let world = dragon_world();
    let integrator = build_integrator(&cfg);
    let camera = Camera::from(cfg);

    c.benchmark_group("render")
        .sample_size(10)
        .bench_function("dragon_80x60x4", |b| {
            b.iter(|| {
                // Single-threaded, fixed-seed accumulation over the whole frame.
                let mut rng = SmallRng::seed_from_u64(0xBEEF);
                let mut acc = Color::ZERO;
                for j in 0..h {
                    for i in 0..w {
                        for s in 0..4u32 {
                            let sample = SampleId { i: i as u32, j: j as u32, index: s };
                            let ray = camera.get_ray(sample, &mut rng);
                            acc += integrator.radiance(&ray, &world, sample, &mut rng);
                        }
                    }
                }
                acc
            })
        });
}

criterion_group!(
    benches,
    bench_traversal,
    bench_occlusion,
    bench_occlusion_top_level,
    bench_build,
    bench_top_level,
    bench_render
);
criterion_main!(benches);
