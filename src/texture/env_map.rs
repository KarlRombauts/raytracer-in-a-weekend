//! Equirectangular HDR environment map: a high-dynamic-range "sky" sampled by
//! ray direction, used in place of a flat background colour. Rays that escape the
//! scene read their radiance from the map, so it both shows behind the scene and
//! lights/reflects onto it (via BSDF bounces — Tier 1; importance-sampling the
//! map as a light for clean diffuse illumination is a later upgrade).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::color::{luminance, Color};
use crate::sampling::Distribution2D;
use crate::vec3::Vec3;

pub struct EnvMap {
    width: usize,
    height: usize,
    /// Linear-light radiance, row-major, top row = +Y (up).
    data: Vec<[f32; 3]>,
    /// Yaw applied to the lookup direction (radians), to spin the sky.
    rotation: f32,
    /// Importance-sampling distribution over the texels, weighted by
    /// luminance × sin θ so directions are sampled in proportion to sky radiance
    /// (the sin θ cancels the equirectangular Jacobian). Built once at load.
    distribution: Distribution2D,
}

impl EnvMap {
    /// Set the sky's yaw (radians) — rotates the map about the vertical axis.
    pub fn with_rotation(mut self, yaw_radians: f32) -> Self {
        self.rotation = yaw_radians;
        self
    }

    /// Load a Radiance `.hdr` equirectangular map, keeping full f32 range (the
    /// HDR codec already decodes to linear light, so no sRGB conversion).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_hdr_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let img = image::open(path)?.into_rgb32f();
        let (w, h) = img.dimensions();
        let data = img.pixels().map(|p| [p[0], p[1], p[2]]).collect();
        Ok(EnvMap::from_pixels(w as usize, h as usize, data))
    }

    /// Build an env map from raw linear-light pixels (row-major, top row = +Y),
    /// computing the importance-sampling distribution. Each texel's sampling
    /// weight is its luminance × sin θ, where θ is its latitude — the sin θ both
    /// corrects the equirectangular pole stretch and (after the pdf's ÷ sin θ)
    /// leaves the sky sampled in proportion to radiance.
    pub fn from_pixels(width: usize, height: usize, data: Vec<[f32; 3]>) -> Self {
        let mut func = vec![0.0f32; width * height];
        for y in 0..height {
            let sin_theta = (std::f32::consts::PI * (y as f32 + 0.5) / height as f32).sin();
            for x in 0..width {
                let p = data[y * width + x];
                func[y * width + x] = luminance(Color::new(p[0], p[1], p[2])) * sin_theta;
            }
        }
        let distribution = Distribution2D::new(&func, width, height);
        EnvMap { width, height, data, rotation: 0.0, distribution }
    }

    fn texel(&self, x: usize, y: usize) -> Color {
        let p = self.data[y * self.width + x];
        Color::new(p[0], p[1], p[2])
    }

    /// Radiance arriving along `dir` (pointing away from the surface, toward the
    /// sky). Equirectangular lookup with bilinear filtering: wrap in longitude,
    /// clamp in latitude.
    pub fn sample(&self, dir: &Vec3) -> Color {
        use std::f32::consts::PI;
        let d = dir.unit();
        // Spin the sky about +Y.
        let (s, c) = self.rotation.sin_cos();
        let dx = c * d.x + s * d.z;
        let dz = -s * d.x + c * d.z;

        let u = 0.5 + dz.atan2(dx) / (2.0 * PI); // longitude -> [0, 1)
        let v = d.y.clamp(-1.0, 1.0).acos() / PI; // latitude: 0 = up (+Y), 1 = down

        let fx = u * self.width as f32 - 0.5;
        let fy = v * self.height as f32 - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let (tx, ty) = (fx - x0, fy - y0);

        let w = self.width as i64;
        let h = self.height as i64;
        let wrap = |i: i64| (((i % w) + w) % w) as usize;
        let clampy = |j: i64| j.clamp(0, h - 1) as usize;
        let (x0i, x1i) = (wrap(x0 as i64), wrap(x0 as i64 + 1));
        let (y0i, y1i) = (clampy(y0 as i64), clampy(y0 as i64 + 1));

        let top = self.texel(x0i, y0i) * (1.0 - tx) + self.texel(x1i, y0i) * tx;
        let bot = self.texel(x0i, y1i) * (1.0 - tx) + self.texel(x1i, y1i) * tx;
        top * (1.0 - ty) + bot * ty
    }

    /// (u, v) image coordinates of a world direction — the same equirectangular
    /// mapping `sample` uses (yaw-rotated longitude, latitude), factored out so
    /// importance sampling and radiance lookup share one convention.
    fn dir_to_uv(&self, dir: &Vec3) -> (f32, f32) {
        use std::f32::consts::PI;
        let d = dir.unit();
        let (s, c) = self.rotation.sin_cos();
        let dx = c * d.x + s * d.z;
        let dz = -s * d.x + c * d.z;
        let u = 0.5 + dz.atan2(dx) / (2.0 * PI);
        let v = d.y.clamp(-1.0, 1.0).acos() / PI;
        (u, v)
    }

    /// Importance-sample a direction toward the sky from two uniforms in [0, 1),
    /// returning the (unit) world direction and its **solid-angle** pdf. Directions
    /// are drawn in proportion to sky radiance; the pdf is the image-space density
    /// divided by the equirectangular Jacobian `2π²·sin θ`.
    pub fn sample_direction(&self, u0: f32, u1: f32) -> (Vec3, f32) {
        use std::f32::consts::PI;
        let ((u, v), map_pdf) = self.distribution.sample_continuous(u0, u1);
        let theta = PI * v;
        let sin_theta = theta.sin();
        if sin_theta <= 0.0 {
            return (Vec3::new(0.0, 1.0, 0.0), 0.0); // degenerate pole
        }
        // Direction in the yaw-rotated frame, then un-rotate to world.
        let a = 2.0 * PI * (u - 0.5); // atan2(dz, dx) in the rotated frame
        let (dx, dz) = (sin_theta * a.cos(), sin_theta * a.sin());
        let dy = theta.cos();
        let (s, c) = self.rotation.sin_cos();
        let wx = c * dx - s * dz;
        let wz = s * dx + c * dz;
        let dir = Vec3::new(wx, dy, wz);
        let pdf = map_pdf / (2.0 * PI * PI * sin_theta);
        (dir, pdf)
    }

    /// Solid-angle pdf of importance-sampling `dir` toward the sky — the inverse
    /// of [`sample_direction`], for MIS weighting.
    pub fn direction_pdf(&self, dir: &Vec3) -> f32 {
        use std::f32::consts::PI;
        let sin_theta = (1.0 - dir.unit().y.clamp(-1.0, 1.0).powi(2)).max(0.0).sqrt();
        if sin_theta <= 0.0 {
            return 0.0;
        }
        let (u, v) = self.dir_to_uv(dir);
        self.distribution.pdf(u, v) / (2.0 * PI * PI * sin_theta)
    }
}

/// Load (and cache) the bundled sky named `name`, from `assets/hdrs/<name>.hdr`.
/// Returns `None` (caller falls back to the solid background) if it can't be
/// loaded. Cached by name so rebuilding the camera on every edit is cheap.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_cached(name: &str) -> Option<Arc<EnvMap>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<EnvMap>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = cache.lock().unwrap();
    if let Some(entry) = map.get(name) {
        return entry.clone();
    }
    let path = format!("assets/hdrs/{name}.hdr");
    let loaded = EnvMap::from_hdr_file(&path).ok().map(Arc::new);
    if loaded.is_none() {
        eprintln!("warning: could not load sky '{name}' (looked in {path})");
    }
    map.insert(name.to_string(), loaded.clone());
    loaded
}

/// On wasm there's no filesystem; skies would need to be embedded/fetched.
#[cfg(target_arch = "wasm32")]
pub fn load_cached(_name: &str) -> Option<Arc<EnvMap>> {
    None
}

/// Names (file stems) of the bundled HDR skies in `assets/hdrs/`, sorted — for
/// populating the sky picker. Empty on wasm (no filesystem).
#[cfg(not(target_arch = "wasm32"))]
pub fn available_skies() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("assets/hdrs")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().and_then(|s| s.to_str()) == Some("hdr"))
                .then(|| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
                .flatten()
        })
        .collect();
    names.sort();
    names
}

#[cfg(target_arch = "wasm32")]
pub fn available_skies() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: usize, height: usize, fill: [f32; 3]) -> EnvMap {
        EnvMap::from_pixels(width, height, vec![fill; width * height])
    }

    #[test]
    fn flat_map_returns_its_constant_radiance_in_every_direction() {
        let m = flat(8, 4, [0.2, 0.5, 1.5]); // note: HDR value > 1
        for d in [
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-0.3, 0.2, 0.9),
        ] {
            let c = m.sample(&d);
            assert!((c.x - 0.2).abs() < 1e-4 && (c.y - 0.5).abs() < 1e-4 && (c.z - 1.5).abs() < 1e-4,
                "dir {d:?} -> {c:?}");
        }
    }

    #[test]
    fn up_and_down_hit_the_top_and_bottom_rows() {
        // A vertical gradient: top row bright, bottom row dark.
        let (w, h) = (4usize, 2usize);
        let mut data = vec![[0.0_f32; 3]; w * h];
        for x in 0..w {
            data[x] = [1.0, 1.0, 1.0]; // top row (v≈0)
            data[w + x] = [0.0, 0.0, 0.0]; // bottom row (v≈1)
        }
        let m = EnvMap::from_pixels(w, h, data);
        assert!(m.sample(&Vec3::new(0.0, 1.0, 0.0)).y > 0.9, "straight up should be bright");
        assert!(m.sample(&Vec3::new(0.0, -1.0, 0.0)).y < 0.1, "straight down should be dark");
    }

    // A uniform direction on the sphere (for Monte-Carlo integration checks).
    fn random_unit(rng: &mut rand::rngs::SmallRng) -> Vec3 {
        use rand::Rng;
        let z = rng.random::<f32>() * 2.0 - 1.0;
        let phi = rng.random::<f32>() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * phi.cos(), z, r * phi.sin())
    }

    #[test]
    fn importance_sampling_concentrates_on_the_bright_texel() {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};
        // Dim everywhere except one bright texel. Sampled directions should point
        // at it — verified by looking the direction back up: it reads bright.
        let (w, h) = (8usize, 4usize);
        let mut data = vec![[0.01f32; 3]; w * h];
        data[w + 6] = [100.0, 100.0, 100.0]; // row 1, col 6
        let env = EnvMap::from_pixels(w, h, data);
        let mut rng = SmallRng::seed_from_u64(7);
        let mut bright = 0;
        for _ in 0..2000 {
            let (dir, pdf) = env.sample_direction(rng.random(), rng.random());
            assert!(pdf > 0.0, "sampled pdf must be positive");
            if env.sample(&dir).x > 1.0 {
                bright += 1;
            }
        }
        assert!(bright as f32 / 2000.0 > 0.9, "most samples should hit the bright texel, got {bright}/2000");
    }

    #[test]
    fn direction_pdf_integrates_to_one_over_the_sphere() {
        use rand::rngs::SmallRng;
        use rand::SeedableRng;
        // Estimate ∫ pdf dω by uniform sphere sampling: mean(pdf)·4π ≈ 1. Catches a
        // wrong Jacobian constant (the 2π²·sinθ factor).
        let (w, h) = (8usize, 4usize);
        let mut data = vec![[0.2f32; 3]; w * h];
        data[w + 6] = [50.0, 50.0, 50.0];
        let env = EnvMap::from_pixels(w, h, data);
        let mut rng = SmallRng::seed_from_u64(9);
        let n = 200_000;
        let mut sum = 0.0f64;
        for _ in 0..n {
            let d = random_unit(&mut rng);
            sum += env.direction_pdf(&d) as f64;
        }
        let integral = (sum / n as f64) * 4.0 * std::f64::consts::PI;
        assert!((integral - 1.0).abs() < 0.05, "∫pdf dω = {integral}");
    }

    #[test]
    fn sample_direction_reports_the_same_pdf_as_direction_pdf() {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};
        let (w, h) = (6usize, 3usize);
        let mut data = vec![[0.1f32; 3]; w * h];
        data[w + 3] = [20.0, 20.0, 20.0];
        let env = EnvMap::from_pixels(w, h, data);
        let mut rng = SmallRng::seed_from_u64(11);
        for _ in 0..200 {
            let (dir, pdf) = env.sample_direction(rng.random(), rng.random());
            let p2 = env.direction_pdf(&dir);
            assert!((pdf - p2).abs() <= 0.02 * pdf.max(1e-3), "sample pdf {pdf} vs direction_pdf {p2}");
        }
    }
}
