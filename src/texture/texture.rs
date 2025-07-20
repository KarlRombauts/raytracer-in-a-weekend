use crate::{color::Color, vec3::Point3};

pub trait Texture: Send + Sync {
    fn value(&self, u: f32, v: f32, p: &Point3) -> Color;
}
