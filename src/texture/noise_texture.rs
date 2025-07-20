use crate::color::Color;
use crate::texture::{Perlin, Texture};
use crate::vec3::Point3;

pub struct NoiseTexture {
    noise: Perlin,
    scale: f32,
}

impl NoiseTexture {
    pub fn new(scale: f32) -> Self {
        NoiseTexture {
            noise: Perlin::new(),
            scale,
        }
    }
}

impl Texture for NoiseTexture {
    fn value(&self, u: f32, v: f32, p: &Point3) -> Color {
        let point = self.scale * (*p);
        // return Color::new(1., 1., 1.) * 0.5 * (1.0 + self.noise.noise(&point));
        return Color::new(1., 1., 1.) * self.noise.turb(p, 7);
    }
}
