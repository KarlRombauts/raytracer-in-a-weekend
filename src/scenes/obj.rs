use std::sync::Arc;

use crate::{
    camera::{camera, Camera, CameraConfig},
    color::Color,
    geometry::{ObjData, Quad},
    group::IntersectGroup,
    material::Lambertian,
    ray::BVHNode,
    vec3::{Point3, Vec3},
};

pub fn obj() {
    let red = Arc::new(Lambertian::from_color(Color::new(1.0, 0.2, 0.2)));
    let gray = Arc::new(Lambertian::from_color(Color::new(0.8, 0.8, 0.8)));
    let mut obj = ObjData::load("./objs/dragon.obj").into_mesh(gray);
    obj.add(Arc::new(Quad::new(
        Point3::new(-500., 0., 20.),
        Vec3::new(3000., 0., 0.),
        Vec3::new(0., 0., -3000.),
        red,
    )));

    let world = IntersectGroup::from_object(BVHNode::from_group(obj));

    let camera = Camera::from(
        CameraConfig::builder()
            .image_width(1200)
            .samples(400)
            .max_depth(50)
            .fov(40.)
            .look_from(Vec3::new(10., 8., 10.))
            .look_at(Vec3::new(0.3, 1.6, 0.))
            .build(),
    );

    camera.render(&world);
}
