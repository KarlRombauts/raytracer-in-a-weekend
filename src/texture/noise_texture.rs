use crate::color::Color;
use crate::texture::{Perlin, Texture};
use crate::vec3::Point3;

pub struct NoiseTexture {
    noise: Perlin,
    scale: f32,
    depth: u32,
}

impl NoiseTexture {
    pub fn new(scale: f32, depth: u32) -> Self {
        NoiseTexture {
            noise: Perlin::new(),
            scale,
            depth,
        }
    }
}

impl Texture for NoiseTexture {
    fn value(&self, _u: f32, _v: f32, p: &Point3) -> Color {
        Color::new(1., 1., 1.) * self.noise.turb(&(self.scale * *p), self.depth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::Point3;

    #[test]
    fn scale_actually_changes_the_sample() {
        let p = Point3::new(1.3, 0.7, 2.1);
        let a = NoiseTexture::new(1.0, 7).value(0.0, 0.0, &p);
        let b = NoiseTexture::new(10.0, 7).value(0.0, 0.0, &p);
        assert!((a.x - b.x).abs() > 1e-4, "scale had no effect: a={a:?} b={b:?}");
    }

    #[test]
    fn depth_changes_the_sample() {
        let p = Point3::new(1.3, 0.7, 2.1);
        let a = NoiseTexture::new(2.0, 1).value(0.0, 0.0, &p);
        let b = NoiseTexture::new(2.0, 7).value(0.0, 0.0, &p);
        assert!((a.x - b.x).abs() > 1e-4, "depth had no effect: a={a:?} b={b:?}");
    }
}
