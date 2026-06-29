use rand::rngs::SmallRng;
use rand::SeedableRng;
use rayon::prelude::*;

use crate::camera::Camera;
use crate::color::{luminance, Color};
use crate::group::IntersectGroup;

/// Per-pixel running statistics for adaptive sampling: a numerically stable
/// (Welford) running mean of the colour and of its luminance, plus the
/// luminance variance used to decide when the pixel has converged.
#[derive(Clone)]
struct Pixel {
    mean: Color,   // running per-channel mean = the pixel's current estimate
    lum_mean: f32, // running mean of sample luminance
    lum_m2: f32,   // Welford sum of squared luminance deviations
    count: u32,
    done: bool, // converged — stop drawing samples here
}

impl Pixel {
    fn new() -> Self {
        Pixel {
            mean: Color::ZERO,
            lum_mean: 0.0,
            lum_m2: 0.0,
            count: 0,
            done: false,
        }
    }

    /// Fold one more sample in (Welford update for colour mean and luminance
    /// mean/variance).
    fn add(&mut self, sample: Color) {
        self.count += 1;
        let n = self.count as f32;
        self.mean += (sample - self.mean) / n;
        let l = luminance(sample);
        let delta = l - self.lum_mean;
        self.lum_mean += delta / n;
        self.lum_m2 += delta * (l - self.lum_mean);
    }

    fn mean(&self) -> Color {
        self.mean
    }

    /// Sample variance of luminance (0 with fewer than two samples).
    fn variance(&self) -> f32 {
        if self.count < 2 {
            0.0
        } else {
            self.lum_m2 / (self.count as f32 - 1.0)
        }
    }

    /// Converged once the 95% confidence half-width of the mean luminance drops
    /// below `rel * mean_luminance + floor`. Needs at least `warmup` samples so
    /// the variance estimate is trustworthy.
    fn converged(&self, rel: f32, floor: f32, warmup: u32) -> bool {
        if self.count < warmup {
            return false;
        }
        let std_err = (self.variance() / self.count as f32).sqrt();
        1.96 * std_err < rel * self.lum_mean + floor
    }
}

/// Accumulates samples one full-image pass at a time so the image can be
/// displayed as it refines. Each `add_pass` adds exactly one sample per pixel;
/// `to_rgba`/`save_png` read each pixel's own running mean (adaptive sampling
/// gives pixels different sample counts).
pub struct ProgressiveRenderer {
    width: u32,
    height: u32,
    pixels: Vec<Pixel>,
    passes: u32,
    rel: f32,
    floor: f32,
    warmup: u32,
}

/// Convergence defaults: stop a pixel when its 95% confidence half-width is
/// within 5% of its mean luminance, after at least 16 samples.
///
/// The threshold is purely *relative* (floor = 0). An absolute floor is
/// tempting for near-black pixels, but any floor > 0 lets a pixel whose samples
/// happen to be identical (variance ≈ 0) converge immediately — and dark pixels
/// lit by rare indirect spikes routinely miss every spike in their warmup
/// window, measure ~0 variance, and freeze to black (salt-and-pepper speckle).
/// With floor = 0 such a pixel keeps sampling until it actually sees structure
/// (or the caller's global sample target caps it), so it can't falsely converge.
const DEFAULT_REL: f32 = 0.05;
const DEFAULT_FLOOR: f32 = 0.0;
const DEFAULT_WARMUP: u32 = 16;

impl ProgressiveRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        ProgressiveRenderer {
            width,
            height,
            pixels: vec![Pixel::new(); (width * height) as usize],
            passes: 0,
            rel: DEFAULT_REL,
            floor: DEFAULT_FLOOR,
            warmup: DEFAULT_WARMUP,
        }
    }

    pub fn passes(&self) -> u32 {
        self.passes
    }

    /// True once every pixel has reached its convergence threshold — the caller
    /// can stop early instead of running to a fixed sample target.
    pub fn all_converged(&self) -> bool {
        self.pixels.iter().all(|p| p.done)
    }

    /// Draw one more sample for every pixel that hasn't yet converged, in
    /// parallel. Converged pixels are skipped, so passes get cheaper over time.
    pub fn add_pass(&mut self, camera: &Camera, world: &IntersectGroup) {
        let width = self.width;
        let (rel, floor, warmup) = (self.rel, self.floor, self.warmup);
        self.pixels.par_iter_mut().enumerate().for_each(|(idx, p)| {
            if p.done {
                return;
            }
            let i = idx as u32 % width;
            let j = idx as u32 / width;
            // Seed per pixel AND per local sample index, so each sample is fresh
            // and reproducible regardless of which pass it happens to land in.
            let mut rng = SmallRng::seed_from_u64(((p.count as u64) << 40) ^ idx as u64);
            let s = camera.sample_pixel(i, j, p.count, world, &mut rng);
            p.add(s);
            if p.converged(rel, floor, warmup) {
                p.done = true;
            }
        });
        self.passes += 1;
    }

    /// Current image as gamma-corrected, opaque RGBA bytes (row-major).
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity((self.width * self.height * 4) as usize);
        for p in &self.pixels {
            let [r, g, b] = p.mean().to_rgb_vec();
            out.extend_from_slice(&[r, g, b, 255]);
        }
        out
    }

    pub fn save_png(&self, path: &str) {
        let mut img = image::ImageBuffer::new(self.width, self.height);
        for (idx, p) in self.pixels.iter().enumerate() {
            let x = idx as u32 % self.width;
            let y = idx as u32 / self.width;
            img.put_pixel(x, y, image::Rgb(p.mean().to_rgb_vec()));
        }
        img.save(path).unwrap();
    }
}

#[cfg(test)]
mod adaptive_tests {
    use super::*;
    use crate::color::Color;

    // grey(v) has luminance exactly v (Rec.709 weights sum to 1).
    fn gray(v: f32) -> Color {
        Color::new(v, v, v)
    }

    #[test]
    fn mean_is_the_running_average() {
        let mut p = Pixel::new();
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            p.add(gray(v));
        }
        let m = p.mean();
        assert!((m.x - 3.0).abs() < 1e-5 && (m.y - 3.0).abs() < 1e-5 && (m.z - 3.0).abs() < 1e-5, "mean={m:?}");
        assert_eq!(p.count, 5);
    }

    #[test]
    fn variance_matches_sample_variance_of_luminance() {
        let mut p = Pixel::new();
        // luminances 1..5 → sample variance = 2.5.
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            p.add(gray(v));
        }
        assert!((p.variance() - 2.5).abs() < 1e-4, "var={}", p.variance());
    }

    #[test]
    fn not_converged_before_warmup() {
        let mut p = Pixel::new();
        for _ in 0..7 {
            p.add(gray(0.5)); // perfectly flat, but below the warmup count
        }
        assert!(!p.converged(0.05, 0.001, 8));
    }

    #[test]
    fn converges_on_a_flat_signal_after_warmup() {
        let mut p = Pixel::new();
        for _ in 0..8 {
            p.add(gray(0.5)); // zero variance
        }
        assert!(p.converged(0.05, 0.001, 8));
    }

    #[test]
    fn stays_unconverged_on_a_noisy_signal() {
        let mut p = Pixel::new();
        for k in 0..64 {
            p.add(gray(if k % 2 == 0 { 0.1 } else { 0.9 })); // mean 0.5, high variance
        }
        assert!(!p.converged(0.05, 0.001, 8), "var={}", p.variance());
    }

    #[test]
    fn repro_does_not_freeze_dark_spiky_pixels_to_black() {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};
        // Model a dark, indirectly-lit pixel: each sample is a bright spike with
        // probability q, else black. True mean ≈ q*v. A robust convergence test
        // must not let many such pixels converge to ~black after missing the
        // spikes in their warmup window.
        let q = 0.05_f32;
        let v = 1.8_f32;
        let true_mean = q * v;
        let trials = 1000;
        let mut frozen_black = 0;
        for t in 0..trials {
            let mut rng = SmallRng::seed_from_u64(t);
            let mut p = Pixel::new();
            loop {
                let s = if rng.random::<f32>() < q { gray(v) } else { gray(0.0) };
                p.add(s);
                if p.converged(DEFAULT_REL, DEFAULT_FLOOR, DEFAULT_WARMUP) {
                    break;
                }
                if p.count >= 1024 {
                    break;
                }
            }
            if luminance(p.mean()) < 0.5 * true_mean {
                frozen_black += 1;
            }
        }
        eprintln!("frozen_black = {frozen_black}/{trials}");
        assert!(
            frozen_black < trials / 20,
            "too many dark pixels froze to black: {frozen_black}/{trials}"
        );
    }

    #[test]
    fn flat_background_converges_early_to_its_colour() {
        use crate::camera::CameraConfig;
        // An empty world: every ray misses and returns the (constant) background,
        // so every pixel is a zero-variance flat signal and converges at warmup.
        let bg = Color::new(0.2, 0.4, 0.6);
        let camera = Camera::from(
            CameraConfig::builder()
                .image_width(8)
                .aspect_ratio(1.0)
                .background(bg)
                .build(),
        );
        let world = IntersectGroup::new();
        let mut r = ProgressiveRenderer::new(8, 8);

        let mut passes = 0;
        while passes < 64 && !r.all_converged() {
            r.add_pass(&camera, &world);
            passes += 1;
        }

        assert!(r.all_converged(), "flat background should converge");
        assert!(r.passes() < 32, "should stop early, ran {} passes", r.passes());

        // Each converged pixel's estimate equals the background (after gamma).
        let rgba = r.to_rgba();
        let [er, eg, eb] = (bg).to_rgb_vec();
        assert!((rgba[0] as i32 - er as i32).abs() <= 2, "r {} vs {}", rgba[0], er);
        assert!((rgba[1] as i32 - eg as i32).abs() <= 2, "g {} vs {}", rgba[1], eg);
        assert!((rgba[2] as i32 - eb as i32).abs() <= 2, "b {} vs {}", rgba[2], eb);
    }
}
