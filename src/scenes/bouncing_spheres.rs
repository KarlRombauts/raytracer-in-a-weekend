use std::sync::Arc;

use rand::Rng;

use crate::{
    camera::{Camera, CameraConfig},
    color::Color,
    geometry::Sphere,
    group::IntersectGroup,
    material::{Dielectric, Lambertian, Metal},
    ray::BVHNode,
    texture::CheckerTexture,
    vec3::{Point3, Vec3},
};

pub fn bouncing_spheres() {
    let mut world = IntersectGroup::new();

    let checker = Arc::new(CheckerTexture::from_colors(
        0.32,
        Color::new(0.2, 0.3, 0.1),
        Color::new(0.9, 0.9, 0.9),
    ));

    let ground_material = Arc::new(Lambertian::from_texture(checker));

    world.add(Arc::new(Sphere::stationary(
        Point3::new(0., -1000., 0.),
        1000.,
        ground_material.clone(),
    )));

    let mut rng = rand::rng();

    for a in -11..11 {
        for b in -11..11 {
            let center = Vec3::new(
                a as f32 + 0.9 * rng.random::<f32>(),
                0.2,
                b as f32 + 0.9 * rng.random::<f32>(),
            );
            let choose_mat: f32 = rng.random();

            if choose_mat < 0.8 {
                // diffuse
                let albedo = Color::random() * Color::random();
                let material = Arc::new(Lambertian::from_color(albedo));
                let center2 = center + Vec3::new(0.0, rng.random_range(0.0..0.5), 0.0);
                world.add(Arc::new(Sphere::moving(center, center2, 0.2, material)));
            } else if choose_mat < 0.95 {
                // Metal
                let albedo = Color::random_range(0.5, 1.);
                let roughness = rng.random_range(0.0..0.5);
                let material = Arc::new(Metal::new(albedo, roughness));
                world.add(Arc::new(Sphere::stationary(center, 0.2, material)));
            } else {
                // Glass
                let material = Arc::new(Dielectric::new(1.5));
                world.add(Arc::new(Sphere::stationary(center, 0.2, material)));
            }
        }
    }

    let material1 = Arc::new(Dielectric::new(1.5));
    world.add(Arc::new(Sphere::stationary(
        Point3::new(0., 1., 0.),
        1.,
        material1,
    )));

    let material2 = Arc::new(Lambertian::from_color(Color::new(0.4, 0.2, 0.1)));
    world.add(Arc::new(Sphere::stationary(
        Point3::new(-4., 1., 0.),
        1.,
        material2,
    )));

    let material3 = Arc::new(Metal::new(Color::new(0.7, 0.6, 0.5), 0.0));
    world.add(Arc::new(Sphere::stationary(
        Point3::new(4., 1., 0.),
        1.,
        material3,
    )));

    world = IntersectGroup::from_object(BVHNode::from_group(world));

    let camera = Camera::from(
        CameraConfig::builder()
            .image_width(800)
            .fov(20.)
            .samples(300)
            .focus_dist(10.6)
            .dof_angle(0.6)
            .max_depth(50)
            .look_from(Vec3::new(13., 2., 3.))
            .look_at(Vec3::new(0., 0., 0.))
            .build(),
    );

    camera.render(&world)
}
