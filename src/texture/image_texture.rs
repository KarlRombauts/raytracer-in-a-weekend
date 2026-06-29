use crate::{
    color::Color,
    texture::{load_image_linear_buffer, load_image_linear_buffer_from_bytes, Texture},
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

    /// Decode an image from embedded bytes. Non-panicking: returns `Err` on an
    /// undecodable buffer so callers can fall back gracefully.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(ImageTexture {
            image: load_image_linear_buffer_from_bytes(bytes)?,
        })
    }
}

impl Texture for ImageTexture {
    fn value(&self, mut u: f32, mut v: f32, p: &Point3) -> Color {
        if self.image.height() == 0 {
            return Color::new(0., 1., 1.);
        }

        u = u.clamp(0., 1.);
        v = 1.0 - v.clamp(0., 1.);

        // u/v can be exactly 1.0 (clamp above, or MappedTexture's rem_euclid
        // returning 1.0 for tiny-negative inputs), which would index one past the
        // last pixel. Clamp to the last valid row/column.
        let i = ((u * self.image.width() as f32) as u32).min(self.image.width() - 1);
        let j = ((v * self.image.height() as f32) as u32).min(self.image.height() - 1);
        let pixel = self.image.get_pixel(i, j);

        return Color::new(pixel[0], pixel[1], pixel[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::Point3;
    use image::{DynamicImage, ImageFormat, RgbImage};
    use std::io::Cursor;

    fn red_png_bytes() -> Vec<u8> {
        let mut img = RgbImage::new(2, 2);
        for p in img.pixels_mut() {
            *p = image::Rgb([255, 0, 0]);
        }
        let mut bytes: Vec<u8> = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn from_bytes_decodes_a_png() {
        let tex = ImageTexture::from_bytes(&red_png_bytes()).expect("valid png decodes");
        // Uniform red image: any uv samples the same linear-red pixel.
        let c = tex.value(0.5, 0.5, &Point3::new(0.0, 0.0, 0.0));
        assert!(c.x > 0.99 && c.y < 0.01 && c.z < 0.01, "got {c:?}");
    }

    #[test]
    fn from_bytes_rejects_garbage() {
        assert!(ImageTexture::from_bytes(&[1, 2, 3, 4]).is_err());
    }

    #[test]
    fn boundary_uv_does_not_panic() {
        // u == 1.0 → i == width, and v == 0.0 → (1.0 - 0.0) → j == height: both
        // are one past the last valid index. get_pixel must not be called OOB.
        // MappedTexture's rem_euclid(1.0) can emit exactly 1.0 for tiny-negative
        // inputs, so these boundary coordinates reach ImageTexture in practice.
        let tex = ImageTexture::from_bytes(&red_png_bytes()).expect("valid png decodes");
        let c = tex.value(1.0, 0.0, &Point3::new(0.0, 0.0, 0.0));
        assert!(c.x > 0.99 && c.y < 0.01 && c.z < 0.01, "got {c:?}");
    }
}
