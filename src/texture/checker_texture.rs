use std::sync::Arc;

use crate::{
    color::Color,
    texture::{SolidColor, Texture},
    vec3::Point3,
};

pub struct CheckerTexture {
    inv_scale: f32,
    even: Arc<dyn Texture>,
    odd: Arc<dyn Texture>,
}

impl CheckerTexture {
    pub fn from_textures(scale: f32, even: Arc<dyn Texture>, odd: Arc<dyn Texture>) -> Self {
        CheckerTexture {
            inv_scale: 1.0 / scale,
            even,
            odd,
        }
    }

    pub fn from_colors(scale: f32, even: Color, odd: Color) -> Self {
        CheckerTexture {
            inv_scale: 1.0 / scale,
            even: Arc::new(SolidColor::from_color(even)),
            odd: Arc::new(SolidColor::from_color(odd)),
        }
    }
}

impl Texture for CheckerTexture {
    fn value(&self, u: f32, v: f32, p: &Point3) -> Color {
        let x_int = (self.inv_scale * p.x).floor() as i32;
        let y_int = (self.inv_scale * p.y).floor() as i32;
        let z_int = (self.inv_scale * p.z).floor() as i32;

        let is_even = (x_int + y_int + z_int) % 2 == 0;

        match is_even {
            true => self.even.value(u, v, p),
            false => self.odd.value(u, v, p),
        }
    }
}
