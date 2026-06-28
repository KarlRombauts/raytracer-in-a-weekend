use std::sync::Arc;

use crate::{
    camera::{camera, Camera, CameraConfig},
    color::Color,
    geometry::{ObjData, Quad},
    group::IntersectGroup,
    material::{Lambertian, Metal},
    ray::BVHNode,
    vec3::{Point3, Vec3},
};

pub fn obj() {
    let gray = Arc::new(Metal::new(Color::new(1.0, 0.4, 0.2), 0.2));
    let mut obj = ObjData::load("./objs/dragon.obj").into_mesh(gray);
    let floor = Arc::new(Lambertian::from_color(Color::new(0.8, 0.8, 0.8)));
    obj.add(Arc::new(Quad::new(
        Point3::new(-500., 0.1, 20.),
        Vec3::new(3000., 0.1, 0.),
        Vec3::new(0., 0.1, -3000.),
        floor,
    )));

    let world = IntersectGroup::from_object(BVHNode::from_group(obj));

    let camera = Camera::from(
        CameraConfig::builder()
            .aspect_ratio(4. / 3.)
            .image_width(1200)
            .samples(400)
            .max_depth(10)
            .fov(30.)
            .look_from(Vec3::new(10., 8., 10.))
            .look_at(Vec3::new(0., 2.2, 0.))
            .build(),
    );

    camera.render(&world);
}
