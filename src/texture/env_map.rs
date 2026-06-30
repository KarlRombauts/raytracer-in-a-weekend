//! Equirectangular HDR environment map: a high-dynamic-range "sky" sampled by
//! ray direction, used in place of a flat background colour. Rays that escape the
//! scene read their radiance from the map, so it both shows behind the scene and
//! lights/reflects onto it (via BSDF bounces — Tier 1; importance-sampling the
//! map as a light for clean diffuse illumination is a later upgrade).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::color::Color;
use crate::vec3::Vec3;

pub struct EnvMap {
    width: usize,
    height: usize,
    /// Linear-light radiance, row-major, top row = +Y (up).
    data: Vec<[f32; 3]>,
    /// Yaw applied to the lookup direction (radians), to spin the sky.
    rotation: f32,
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
        Ok(EnvMap {
            width: w as usize,
            height: h as usize,
            data,
            rotation: 0.0,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: usize, height: usize, fill: [f32; 3]) -> EnvMap {
        EnvMap { width, height, data: vec![fill; width * height], rotation: 0.0 }
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
        let m = EnvMap { width: w, height: h, data, rotation: 0.0 };
        assert!(m.sample(&Vec3::new(0.0, 1.0, 0.0)).y > 0.9, "straight up should be bright");
        assert!(m.sample(&Vec3::new(0.0, -1.0, 0.0)).y < 0.1, "straight down should be dark");
    }
}
