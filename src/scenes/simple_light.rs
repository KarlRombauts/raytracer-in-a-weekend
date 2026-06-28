use std::sync::Arc;

use crate::{
    camera::{
        camera, config::CameraConfigBuilder_Error_Repeated_field_background, Camera, CameraConfig,
    },
    color::Color,
    geometry::{Quad, Sphere},
    group::IntersectGroup,
    material::{diffuse_light, DiffuseLight, Lambertian},
    texture::{noise_texture, NoiseTexture},
    vec3::{Point3, Vec3},
};

pub fn simple_light() {
    let noise_texture = Arc::new(NoiseTexture::new(4.));
    let noise_material = Arc::new(Lambertian::from_texture(noise_texture));
    let mut world = IntersectGroup::new();
    //ground
    world.add(Arc::new(Sphere::stationary(
        Point3::new(0., -1000., 0.),
        1000.,
        noise_material.clone(),
    )));
    // ball
    world.add(Arc::new(Sphere::stationary(
        Point3::new(0., 2., 0.),
        2.,
        noise_material.clone(),
    )));

    let diffuse_light = Arc::new(DiffuseLight::from_color(Color::new(4., 4., 4.)));
    world.add(Arc::new(Quad::new(
        Point3::new(3., 1., -2.),
        Point3::new(2., 0., 0.),
        Point3::new(0., 2., 0.),
        diffuse_light.clone(),
    )));

    world.add(Arc::new(Sphere::stationary(
        Point3::new(0., 7., 0.),
        2.,
        diffuse_light.clone(),
    )));

    let camera = Camera::from(
        CameraConfig::builder()
            .aspect_ratio(16.0 / 9.0)
            .image_width(800)
            .samples(400)
            .max_depth(50)
            .background(Color::zeros())
            .fov(20.)
            .look_from(Vec3::new(26., 3., 6.))
            .look_at(Vec3::new(0., 2., 0.))
            .dof_angle(0.)
            .build(),
    );

    camera.render(&world);
}
