use rand::rngs::SmallRng;
use rand::SeedableRng;
use rayon::prelude::*;

use crate::camera::Camera;
use crate::color::Color;
use crate::group::IntersectGroup;

/// Accumulates samples one full-image pass at a time so the image can be
/// displayed as it refines. Each `add_pass` adds exactly one sample per pixel;
/// `to_rgba`/`save_png` divide the running sum by the number of passes so far.
pub struct ProgressiveRenderer {
    width: u32,
    height: u32,
    accum: Vec<Color>,
    passes: u32,
}

impl ProgressiveRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        ProgressiveRenderer {
            width,
            height,
            accum: vec![Color::ZERO; (width * height) as usize],
            passes: 0,
        }
    }

    pub fn passes(&self) -> u32 {
        self.passes
    }

    /// Add one sample per pixel to the accumulation buffer, in parallel.
    pub fn add_pass(&mut self, camera: &Camera, world: &IntersectGroup) {
        let width = self.width;
        let passes = self.passes;
        self.accum.par_iter_mut().enumerate().for_each(|(idx, c)| {
            let i = idx as u32 % width;
            let j = idx as u32 / width;
            // Seed per pixel AND per pass so each pass draws fresh samples
            // (otherwise accumulation would average identical samples).
            let mut rng = SmallRng::seed_from_u64(((passes as u64) << 40) ^ idx as u64);
            *c += camera.sample_pixel(i, j, passes, world, &mut rng);
        });
        self.passes += 1;
    }

    /// Current image as gamma-corrected, opaque RGBA bytes (row-major).
    pub fn to_rgba(&self) -> Vec<u8> {
        let scale = self.sample_scale();
        let mut out = Vec::with_capacity((self.width * self.height * 4) as usize);
        for c in &self.accum {
            let [r, g, b] = (*c * scale).to_rgb_vec();
            out.extend_from_slice(&[r, g, b, 255]);
        }
        out
    }

    pub fn save_png(&self, path: &str) {
        let scale = self.sample_scale();
        let mut img = image::ImageBuffer::new(self.width, self.height);
        for (idx, c) in self.accum.iter().enumerate() {
            let x = idx as u32 % self.width;
            let y = idx as u32 / self.width;
            img.put_pixel(x, y, image::Rgb((*c * scale).to_rgb_vec()));
        }
        img.save(path).unwrap();
    }

    fn sample_scale(&self) -> f32 {
        if self.passes == 0 {
            0.0
        } else {
            1.0 / self.passes as f32
        }
    }
}
