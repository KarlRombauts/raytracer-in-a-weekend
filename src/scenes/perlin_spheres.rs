use std::sync::Arc;

use crate::{
    camera::{Camera, CameraConfig},
    geometry::Sphere,
    group::IntersectGroup,
    material::Lambertian,
    texture::NoiseTexture,
    vec3::Point3,
};

pub fn perlin_spheres() {
    let mut world = IntersectGroup::new();
    let perlin_texture = Arc::new(NoiseTexture::new(4.));

    world.add(Arc::new(Sphere::stationary(
        Point3::new(0., -1000., 0.),
        1000.,
        Arc::new(Lambertian::from_texture(perlin_texture.clone())),
    )));

    world.add(Arc::new(Sphere::stationary(
        Point3::new(0., 2., 0.),
        2.,
        Arc::new(Lambertian::from_texture(perlin_texture.clone())),
    )));

    let camera = Camera::from(
        CameraConfig::builder()
            .image_width(400)
            .samples(400)
            .max_depth(50)
            .fov(20.)
            .look_from(Point3::new(13., 2., 3.))
            .look_at(Point3::ZERO)
            .build(),
    );

    camera.render(&world);
}
