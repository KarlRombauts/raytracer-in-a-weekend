use crate::{color::Color, vec3::Vec3};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

#[derive(TypedBuilder, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraConfig {
    #[builder(default = 16.0 / 9.0)]
    pub aspect_ratio: f64,

    #[builder(default = 400)]
    pub image_width: u32,

    #[builder(default = 100)]
    pub samples: u32,

    #[builder(default = 10)]
    pub max_depth: u32,

    #[builder(default = 60.0)]
    pub fov: f32,

    #[builder(default = 0.0)]
    pub roll: f32,

    #[builder(default = 0.)]
    pub dof_angle: f32,

    #[builder(default = 10.)]
    pub focus_dist: f32,

    #[builder(default = Vec3::new(0., 0., 0.))]
    pub look_from: Vec3,

    #[builder(default = Vec3::new(0., 0., -1.))]
    pub look_at: Vec3,

    #[builder(default = Vec3::new(0., 1., 0.))]
    pub v_up: Vec3,

    #[builder(default = Color::new(0.7, 0.8, 1.))]
    pub background: Color,

    /// Firefly suppression: per-sample luminance cap. `f32::INFINITY` disables it.
    #[builder(default = f32::INFINITY)]
    pub firefly_clamp: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_config_postcard_round_trip() {
        let original = CameraConfig::builder()
            .aspect_ratio(4.0 / 3.0)
            .image_width(800)
            .samples(50)
            .max_depth(20)
            .fov(45.0)
            .roll(0.1)
            .dof_angle(0.5)
            .focus_dist(5.0)
            .look_from(Vec3::new(1.0, 2.0, 3.0))
            .look_at(Vec3::new(0.0, 0.5, -1.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .background(Color::new(0.1, 0.2, 0.3))
            .firefly_clamp(10.0)
            .build();

        let bytes = postcard::to_allocvec(&original).expect("postcard serialise");
        let decoded: CameraConfig = postcard::from_bytes(&bytes).expect("postcard deserialise");
        assert_eq!(original, decoded);
    }
}
