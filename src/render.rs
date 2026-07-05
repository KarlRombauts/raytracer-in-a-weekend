use rand::rngs::SmallRng;
use rand::SeedableRng;
use rayon::prelude::*;

use crate::camera::Camera;
use crate::color::{clamp_luminance, luminance, Color};
use crate::group::IntersectGroup;
use crate::integrator::Integrator;
use crate::sampling::SampleId;

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
        // Defence in depth: a non-finite sample is sticky — it would turn `mean`
        // (and the variance) into NaN forever, freezing the pixel black and
        // blocking convergence. Drop it so the pixel keeps its good samples.
        // Samples are normally sanitized upstream (`clamp_luminance`); this guards
        // any path that isn't.
        if !sample.x.is_finite() || !sample.y.is_finite() || !sample.z.is_finite() {
            return;
        }
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
    ///
    /// Dark pixels get a much longer warmup ([`DARK_WARMUP`]). Premature
    /// convergence to the wrong value is only *visible* when the wrong value is
    /// near-black (a black speck on lit geometry): a heavy-tailed pixel whose rare
    /// bright contributions all miss the warmup window measures a tiny variance at
    /// a tiny mean and freezes at the background level. Bright pixels can't hide a
    /// black speck, so they keep the short warmup and stay cheap — the extra
    /// samples are spent only where the failure actually shows.
    fn converged(&self, rel: f32, floor: f32, warmup: u32) -> bool {
        let warmup = if self.lum_mean < DARK_LEVEL {
            warmup.max(DARK_WARMUP)
        } else {
            warmup
        };
        if self.count < warmup {
            return false;
        }
        let std_err = (self.variance() / self.count as f32).sqrt();
        1.96 * std_err < rel * self.lum_mean + floor
    }
}

/// Accumulates samples one full-image pass at a time so the image can be
/// displayed as it refines. Each `add_pass` adds one sample to every pixel that
/// hasn't converged; `to_rgba`/`to_png_bytes` read each pixel's own running mean
/// (adaptive sampling gives pixels different sample counts).
pub struct ProgressiveRenderer {
    width: u32,
    height: u32,
    pixels: Vec<Pixel>,
    passes: u32,
    rel: f32,
    floor: f32,
    warmup: u32,
    /// Per-sample luminance cap applied before a sample is folded into a pixel
    /// (firefly suppression). `f32::INFINITY` disables it. The accumulator owns
    /// this — the integrator returns raw radiance.
    firefly_clamp: f32,
}

/// Convergence defaults: stop a pixel when its 95% confidence half-width is
/// within 5% of its mean luminance, after at least 64 samples.
///
/// The threshold is purely *relative* (floor = 0). An absolute floor is
/// tempting for near-black pixels, but any floor > 0 lets a pixel whose samples
/// happen to be identical (variance ≈ 0) converge immediately — and dark pixels
/// lit by rare indirect spikes routinely miss every spike in their warmup
/// window, measure ~0 variance, and freeze to black (salt-and-pepper speckle).
/// With floor = 0 such a pixel keeps sampling until it actually sees structure
/// (or the caller's global sample target caps it), so it can't falsely converge.
///
/// floor = 0 only protects *near-black* pixels (where rel·mean → 0 too). A
/// *mid-tone* pixel that misses every spike in its warmup window still measures
/// ~0 variance and freezes at the wrong (dim) value — speckle on diffuse
/// surfaces like the checker floor. The only defence is a warmup long enough to
/// usually catch the tail: a spike of probability q is missed in n samples with
/// probability (1-q)ⁿ, so n = 64 cuts a q≈0.1 miss from ~18% (at n = 16) to
/// ~0.1%, at the cost of sampling genuinely-flat regions 64× before they retire
/// (cheap in absolute terms; truly-noisy pixels run into the thousands anyway).
const DEFAULT_REL: f32 = 0.05;
const DEFAULT_FLOOR: f32 = 0.0;
const DEFAULT_WARMUP: u32 = 64;

/// A pixel dimmer than this (linear luminance) is treated as "dark": near enough
/// to black that a premature freeze would read as a black speck. Such pixels must
/// reach [`DARK_WARMUP`] samples before they may converge, so a rare bright
/// contribution has time to appear. Above it, the normal [`DEFAULT_WARMUP`]
/// applies — a bright pixel has already integrated real light and can't hide a
/// speck. The threshold is a small absolute value: the failure lives in the
/// near-black regime (dark scenes / shadowed points against a dim background).
const DARK_LEVEL: f32 = 0.02;
const DARK_WARMUP: u32 = 256;

impl ProgressiveRenderer {
    pub fn new(width: u32, height: u32, firefly_clamp: f32) -> Self {
        ProgressiveRenderer {
            width,
            height,
            pixels: vec![Pixel::new(); (width * height) as usize],
            passes: 0,
            rel: DEFAULT_REL,
            floor: DEFAULT_FLOOR,
            warmup: DEFAULT_WARMUP,
            firefly_clamp,
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
    pub fn add_pass(&mut self, camera: &Camera, integrator: &dyn Integrator, world: &IntersectGroup) {
        let width = self.width;
        let (rel, floor, warmup) = (self.rel, self.floor, self.warmup);
        let firefly = self.firefly_clamp;
        self.pixels.par_iter_mut().enumerate().for_each(|(idx, p)| {
            if p.done {
                return;
            }
            let i = idx as u32 % width;
            let j = idx as u32 / width;
            // Seed per pixel AND per local sample index, so each sample is fresh
            // and reproducible regardless of which pass it happens to land in.
            let mut rng = SmallRng::seed_from_u64(((p.count as u64) << 40) ^ idx as u64);
            // One sample identity shared by ray generation (dim 0) and the
            // integrator (dims 1+). Raw radiance is firefly-clamped before folding.
            let sample = SampleId { i, j, index: p.count };
            let ray = camera.get_ray(sample, &mut rng);
            let s = clamp_luminance(integrator.radiance(&ray, world, sample, &mut rng), firefly);
            p.add(s);
            if p.converged(rel, floor, warmup) {
                p.done = true;
            }
        });
        self.passes += 1;
    }

    /// Render headless to PNG bytes: run up to `samples` passes (stopping early
    /// once every pixel has converged), then encode. No window, no display — the
    /// offline counterpart to the interactive pump in `render_task`.
    pub fn render_to_png(
        camera: &Camera,
        integrator: &dyn Integrator,
        world: &IntersectGroup,
        firefly_clamp: f32,
        samples: u32,
    ) -> Vec<u8> {
        let mut r = ProgressiveRenderer::new(camera.image_width(), camera.image_height(), firefly_clamp);
        for _ in 0..samples {
            if r.all_converged() {
                break;
            }
            r.add_pass(camera, integrator, world);
        }
        r.to_png_bytes()
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

    /// Current image as PNG-encoded bytes (gamma-corrected, opaque RGB). Each
    /// pixel uses its own running mean (adaptive sampling gives pixels different
    /// sample counts).
    pub fn to_png_bytes(&self) -> Vec<u8> {
        let mut img = image::RgbImage::new(self.width, self.height);
        for (idx, p) in self.pixels.iter().enumerate() {
            let x = idx as u32 % self.width;
            let y = idx as u32 / self.width;
            img.put_pixel(x, y, image::Rgb(p.mean().to_rgb_vec()));
        }
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("PNG encode");
        bytes
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
    fn a_non_finite_sample_cannot_poison_the_pixel() {
        // Defence in depth: even if a NaN/Inf path slips past per-sample
        // sanitization, it must not corrupt the running mean. A poisoned mean
        // renders black and the pixel can never converge or recover.
        let mut p = Pixel::new();
        for _ in 0..8 {
            p.add(gray(0.5));
        }
        p.add(Color::new(f32::NAN, 1.0, 1.0));
        p.add(Color::new(f32::INFINITY, 0.0, 0.0));
        for _ in 0..8 {
            p.add(gray(0.5));
        }
        let m = p.mean();
        assert!(m.x.is_finite() && m.y.is_finite() && m.z.is_finite(), "mean poisoned: {m:?}");
        assert!((luminance(m) - 0.5).abs() < 1e-4, "mean drifted: {m:?}");
        assert!(p.converged(0.05, 0.0, 8), "poison should not have blocked convergence");
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
    fn does_not_freeze_dark_heavytail_pixels_at_the_background_level() {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};
        // A genuinely-bright pixel whose light arrives as a rare spike (e.g. a
        // shadowed diffuse point in a dark scene that only brightens when an
        // indirect bounce reaches a light). Its true mean is well above black, but
        // an unlucky warmup window sees only the dim background and — with too few
        // samples — measures a tiny variance and freezes at the background level.
        // That is the salt-and-pepper *black* speck. A dark pixel must take many
        // more samples before it's allowed to converge.
        let q = 0.02_f32;
        let v = 5.0_f32;
        let b = 0.002_f32; // dim background level
        let true_mean = q * v + (1.0 - q) * b; // ≈ 0.1
        let trials = 2000;
        let mut frozen_dark = 0;
        for t in 0..trials {
            let mut rng = SmallRng::seed_from_u64(t ^ 0x5151);
            let mut p = Pixel::new();
            loop {
                // The dim samples vary only slightly (a near-constant background
                // contribution), so an all-miss window measures a tiny variance —
                // exactly the condition that lets a heavy-tailed pixel converge
                // early to the wrong (dark) value.
                let base = b * (0.97 + 0.06 * rng.random::<f32>());
                let s = if rng.random::<f32>() < q { gray(v) } else { gray(base) };
                p.add(s);
                if p.converged(DEFAULT_REL, DEFAULT_FLOOR, DEFAULT_WARMUP) || p.count >= 8192 {
                    break;
                }
            }
            if luminance(p.mean()) < 0.5 * true_mean {
                frozen_dark += 1;
            }
        }
        eprintln!("frozen at background: {frozen_dark}/{trials}");
        assert!(
            frozen_dark < trials / 100,
            "too many dark heavy-tailed pixels froze at the background level (black specks): {frozen_dark}/{trials}"
        );
    }

    #[test]
    fn does_not_freeze_midtone_spiky_pixels_to_the_wrong_value() {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};
        // Model a mid-tone diffuse pixel (e.g. the checker floor): each sample is
        // a bright spike v with probability q, else a dim base b. The mean is
        // mid-tone, but the per-sample distribution is heavy-tailed. A pixel that
        // happens to miss every spike in its warmup window measures ~0 variance
        // and — unlike a near-black pixel, which floor=0 protects — would freeze
        // at the dim base, producing salt-and-pepper speckle. The warmup must be
        // long enough that this is rare.
        let q = 0.1_f32;
        let v = 2.0_f32;
        let b = 0.2_f32;
        let true_mean = q * v + (1.0 - q) * b; // 0.38
        let trials = 2000;
        let mut bad = 0;
        for t in 0..trials {
            let mut rng = SmallRng::seed_from_u64(t);
            let mut p = Pixel::new();
            loop {
                let s = if rng.random::<f32>() < q { gray(v) } else { gray(b) };
                p.add(s);
                if p.converged(DEFAULT_REL, DEFAULT_FLOOR, DEFAULT_WARMUP) || p.count >= 4096 {
                    break;
                }
            }
            if (luminance(p.mean()) - true_mean).abs() / true_mean > 0.15 {
                bad += 1;
            }
        }
        eprintln!("midtone froze badly: {bad}/{trials}");
        assert!(
            bad < trials / 100,
            "too many mid-tone spiky pixels froze far from their true value (speckle): {bad}/{trials}"
        );
    }

    #[test]
    fn flat_background_converges_early_to_its_colour() {
        use crate::camera::CameraConfig;
        // An empty world: every ray misses and returns the (constant) background,
        // so every pixel is a zero-variance flat signal and converges at warmup.
        let bg = Color::new(0.2, 0.4, 0.6);
        let config = CameraConfig::builder()
            .image_width(8)
            .aspect_ratio(1.0)
            .background(bg)
            .build();
        let integrator = crate::integrator::build_integrator(&config);
        let camera = Camera::from(config);
        let world = IntersectGroup::new();
        let mut r = ProgressiveRenderer::new(8, 8, f32::INFINITY);

        let mut passes = 0;
        while passes < 256 && !r.all_converged() {
            r.add_pass(&camera, integrator.as_ref(), &world);
            passes += 1;
        }

        assert!(r.all_converged(), "flat background should converge");
        // A zero-variance signal retires as soon as it clears the warmup window.
        assert!(
            r.passes() <= DEFAULT_WARMUP + 4,
            "should stop right after warmup, ran {} passes",
            r.passes()
        );

        // Each converged pixel's estimate equals the background (after gamma).
        let rgba = r.to_rgba();
        let [er, eg, eb] = (bg).to_rgb_vec();
        assert!((rgba[0] as i32 - er as i32).abs() <= 2, "r {} vs {}", rgba[0], er);
        assert!((rgba[1] as i32 - eg as i32).abs() <= 2, "g {} vs {}", rgba[1], eg);
        assert!((rgba[2] as i32 - eb as i32).abs() <= 2, "b {} vs {}", rgba[2], eb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_png_bytes_round_trips_dimensions() {
        let mut r = ProgressiveRenderer::new(8, 4, f32::INFINITY);
        // No passes needed; encoding the (black) buffer must still be valid PNG.
        let bytes = r.to_png_bytes();
        // PNG magic number.
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // Decodes back to the original dimensions.
        let img = image::load_from_memory(&bytes).expect("valid PNG");
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 4);
        let _ = &mut r;
    }

    #[test]
    fn render_to_png_produces_a_valid_png_at_the_requested_size() {
        use crate::camera::CameraConfig;
        let config = CameraConfig::builder().image_width(8).aspect_ratio(1.0).build();
        let integrator = crate::integrator::build_integrator(&config);
        let camera = Camera::from(config);
        let world = IntersectGroup::new();
        let bytes = ProgressiveRenderer::render_to_png(&camera, integrator.as_ref(), &world, f32::INFINITY, 4);
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let img = image::load_from_memory(&bytes).expect("valid PNG");
        assert_eq!((img.width(), img.height()), (8, 8));
    }
}
