use std::sync::Arc;

use crate::{
    camera::{camera, Camera, CameraConfig},
    color::Color,
    geometry::Triangle,
    group::IntersectGroup,
    material::Lambertian,
    vec3::{Point3, Vec3},
};

pub fn tris() {
    let mut world = IntersectGroup::new();

    let left_red = Arc::new(Lambertian::from_color(Color::new(1.0, 0.2, 0.2)));
    let back_green = Arc::new(Lambertian::from_color(Color::new(0.2, 1.0, 0.2)));
    let right_blue = Arc::new(Lambertian::from_color(Color::new(0.2, 0.2, 1.0)));
    let upper_orange = Arc::new(Lambertian::from_color(Color::new(1.0, 0.5, 0.0)));
    let lower_teal = Arc::new(Lambertian::from_color(Color::new(0.2, 0.8, 0.8)));

    world.add(Arc::new(Triangle::new(
        Point3::new(-3., -2., 5.),
        Vec3::new(0., 0., -4.),
        Vec3::new(0., 4., 0.),
        left_red,
    )));
    world.add(Arc::new(Triangle::new(
        Point3::new(-2., -2., 0.),
        Vec3::new(4., 0., 0.),
        Vec3::new(0., 4., 0.),
        back_green,
    )));
    world.add(Arc::new(Triangle::new(
        Point3::new(3., -2., 1.),
        Vec3::new(0., 0., 4.),
        Vec3::new(0., 4., 0.),
        right_blue,
    )));
    world.add(Arc::new(Triangle::new(
        Point3::new(-2., 3., 1.),
        Vec3::new(4., 0., 0.),
        Vec3::new(0., 0., 4.),
        upper_orange,
    )));
    world.add(Arc::new(Triangle::new(
        Point3::new(-2., -3., 5.),
        Vec3::new(4., 0., 0.),
        Vec3::new(0., 0., -4.),
        lower_teal,
    )));

    let camera = Camera::from(
        CameraConfig::builder()
            .aspect_ratio(1.)
            .image_width(400)
            .samples(100)
            .max_depth(50)
            .fov(80.)
            .look_from(Vec3::new(0., 0., 9.))
            .build(),
    );

    camera.render(&world);
}
