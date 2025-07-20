use std::sync::Arc;

use crate::{
    camera::{Camera, CameraConfig},
    geometry::Sphere,
    group::IntersectGroup,
    material::Lambertian,
    texture::ImageTexture,
    vec3::Point3,
};

pub fn earth() {
    let earth_texture = Arc::new(ImageTexture::new("./images/earth.jpg"));
    let earth_surface = Arc::new(Lambertian::from_texture(earth_texture));
    let globe = Arc::new(Sphere::stationary(Point3::ZERO, 2., earth_surface));

    let camera = Camera::from(
        CameraConfig::builder()
            .image_width(400)
            .aspect_ratio(1.)
            .samples(100)
            .max_depth(50)
            .fov(25.)
            .look_from(Point3::new(-5., 0., -12.))
            .look_at(Point3::ZERO)
            .build(),
    );

    camera.render(&IntersectGroup::from_object(globe));
}
