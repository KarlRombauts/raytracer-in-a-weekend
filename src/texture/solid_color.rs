use crate::{color::Color, texture::Texture, vec3::Point3};

pub struct SolidColor {
    albedo: Color,
}

impl SolidColor {
    pub fn from_color(albedo: Color) -> Self {
        SolidColor { albedo }
    }

    pub fn from_rgb(red: f32, green: f32, blue: f32) -> Self {
        SolidColor {
            albedo: Color::new(red, green, blue),
        }
    }
}

impl Texture for SolidColor {
    fn value(&self, _: f32, _: f32, _: &Point3) -> Color {
        return self.albedo;
    }
}
