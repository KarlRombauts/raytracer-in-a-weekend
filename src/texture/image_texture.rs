use crate::{
    color::Color,
    texture::{load_image_linear_buffer, Texture},
    vec3::Point3,
};

use image::{ImageBuffer, Rgb};

pub struct ImageTexture {
    image: ImageBuffer<Rgb<f32>, Vec<f32>>,
}

impl ImageTexture {
    pub fn new(file_path: &str) -> Self {
        ImageTexture {
            image: load_image_linear_buffer(&file_path).unwrap(),
        }
    }
}

impl Texture for ImageTexture {
    fn value(&self, mut u: f32, mut v: f32, p: &Point3) -> Color {
        if self.image.height() == 0 {
            return Color::new(0., 1., 1.);
        }

        u = u.clamp(0., 1.);
        v = 1.0 - v.clamp(0., 1.);

        let i = (u * self.image.width() as f32) as u32;
        let j = (v * self.image.height() as f32) as u32;
        let pixel = self.image.get_pixel(i, j);

        return Color::new(pixel[0], pixel[1], pixel[2]);
    }
}
