//! Procedural texture presets for the texture library swatch grid.
//!
//! Textures are generated once (via `OnceLock`) as PNG-encoded bytes so they
//! are cheap to clone into `Asset::bytes`. A per-thread egui texture cache
//! turns those bytes into `egui::TextureHandle`s for rendering thumbnails.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

use eframe::egui;
use image::{ImageFormat, RgbImage};

// ---------------------------------------------------------------------------
// Preset definition
// ---------------------------------------------------------------------------

/// A named procedural texture preset, stored as PNG-encoded bytes.
pub struct TexPreset {
    pub name: &'static str,
    pub bytes: Arc<[u8]>,
}

/// All built-in presets, generated once on first access.
pub fn presets() -> &'static [TexPreset] {
    static PRESETS: OnceLock<Vec<TexPreset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        vec![
            TexPreset {
                name: "checker",
                bytes: encode_png(gen_checker(128, 128)),
            },
            TexPreset {
                name: "uv_grid",
                bytes: encode_png(gen_uv_grid(128, 128)),
            },
            TexPreset {
                name: "gradient",
                bytes: encode_png(gen_gradient(128, 128)),
            },
            TexPreset {
                name: "dots",
                bytes: encode_png(gen_dots(128, 128)),
            },
            TexPreset {
                name: "bricks",
                bytes: encode_png(gen_bricks(128, 128)),
            },
            TexPreset {
                name: "noise",
                bytes: encode_png(gen_noise(128, 128)),
            },
        ]
    })
}

// ---------------------------------------------------------------------------
// Thumbnail / texture cache
// ---------------------------------------------------------------------------

thread_local! {
    static TEXTURE_CACHE: RefCell<HashMap<String, egui::TextureHandle>> =
        RefCell::new(HashMap::new());
}

/// Decode `bytes` into an egui texture, cached by `key`. Returns `None` if
/// decoding fails (no panic). Cached handles are returned on subsequent calls.
pub fn texture_for(
    ctx: &egui::Context,
    key: &str,
    bytes: &[u8],
) -> Option<egui::TextureHandle> {
    // Fast path: already cached.
    let cached = TEXTURE_CACHE.with(|c| c.borrow().get(key).cloned());
    if let Some(h) = cached {
        return Some(h);
    }

    // Decode & upload.
    let dyn_img = image::load_from_memory(bytes).ok()?;
    let rgba = dyn_img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_raw(),
    );
    let handle = ctx.load_texture(key, color_image, egui::TextureOptions::LINEAR);

    TEXTURE_CACHE.with(|c| {
        c.borrow_mut().insert(key.to_string(), handle.clone());
    });

    Some(handle)
}

// ---------------------------------------------------------------------------
// PNG encoder helper
// ---------------------------------------------------------------------------

fn encode_png(img: RgbImage) -> Arc<[u8]> {
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .expect("PNG encode");
    Arc::from(bytes)
}

// ---------------------------------------------------------------------------
// Procedural generators
// ---------------------------------------------------------------------------

/// 2-color checkerboard (dark grey / light grey).
fn gen_checker(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    let cells = 8u32;
    for y in 0..h {
        for x in 0..w {
            let cx = x * cells / w;
            let cy = y * cells / h;
            let bright = (cx + cy) % 2 == 0;
            let v = if bright { 220u8 } else { 60u8 };
            img.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    img
}

/// Colored grid lines on a UV gradient background.
fn gen_uv_grid(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    let cells = 8u32;
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / w as f32;
            let v = y as f32 / h as f32;

            // Grid line?
            let gx = (x * cells) % w < 2;
            let gy = (y * cells) % h < 2;

            if gx || gy {
                img.put_pixel(x, y, image::Rgb([255, 255, 255]));
            } else {
                let r = (u * 220.0) as u8;
                let g = (v * 200.0) as u8;
                let b = 80u8;
                img.put_pixel(x, y, image::Rgb([r, g, b]));
            }
        }
    }
    img
}

/// Smooth 2-axis gradient (red → yellow axis, blue channel on y).
fn gen_gradient(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / (w - 1) as f32;
            let v = y as f32 / (h - 1) as f32;
            let r = (u * 255.0) as u8;
            let g = (v * 200.0) as u8;
            let b = ((1.0 - u) * 180.0) as u8;
            img.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    }
    img
}

/// Polka dots: light circles on a dark background.
fn gen_dots(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    let cols = 6u32;
    let rows = 6u32;
    let cell_w = w as f32 / cols as f32;
    let cell_h = h as f32 / rows as f32;
    let radius_frac = 0.35f32;

    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let cx = ((fx / cell_w) as u32).min(cols - 1);
            let cy = ((fy / cell_h) as u32).min(rows - 1);
            let center_x = (cx as f32 + 0.5) * cell_w;
            let center_y = (cy as f32 + 0.5) * cell_h;
            let dx = (fx - center_x) / (cell_w * 0.5);
            let dy = (fy - center_y) / (cell_h * 0.5);
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < radius_frac * 2.0 {
                img.put_pixel(x, y, image::Rgb([220, 200, 160]));
            } else {
                img.put_pixel(x, y, image::Rgb([30, 32, 40]));
            }
        }
    }
    img
}

/// Offset brick pattern.
fn gen_bricks(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    let brick_w = w / 4;
    let brick_h = h / 8;
    let mortar = 2u32;

    for y in 0..h {
        for x in 0..w {
            let row = y / brick_h;
            // Offset every other row by half a brick.
            let offset = if row % 2 == 1 { brick_w / 2 } else { 0 };
            let bx = (x + offset) % brick_w;
            let by = y % brick_h;
            // Mortar gap?
            if bx < mortar || by < mortar {
                img.put_pixel(x, y, image::Rgb([60, 58, 55]));
            } else {
                // Brick color with mild per-brick variation using hash.
                let brick_col = (x + offset) / brick_w;
                let hv = hash2(brick_col, row);
                let r = 170u8.saturating_add((hv & 0x1f) as u8);
                let g = 100u8.saturating_add(((hv >> 5) & 0x0f) as u8);
                let b = 80u8.saturating_add(((hv >> 9) & 0x0f) as u8);
                img.put_pixel(x, y, image::Rgb([r, g, b]));
            }
        }
    }
    img
}

/// Value noise (random grayscale) using a deterministic integer hash.
fn gen_noise(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    let cells = 8u32;
    let cell_w = w / cells;
    let cell_h = h / cells;

    for y in 0..h {
        for x in 0..w {
            let cx = x / cell_w;
            let cy = y / cell_h;
            // Local position within cell [0,1].
            let lx = (x - cx * cell_w) as f32 / cell_w as f32;
            let ly = (y - cy * cell_h) as f32 / cell_h as f32;
            // Bilinearly interpolate corner values.
            let v00 = hash2(cx, cy) as f32 / 255.0;
            let v10 = hash2(cx + 1, cy) as f32 / 255.0;
            let v01 = hash2(cx, cy + 1) as f32 / 255.0;
            let v11 = hash2(cx + 1, cy + 1) as f32 / 255.0;
            let top = lerp(v00, v10, lx);
            let bot = lerp(v01, v11, lx);
            let v = lerp(top, bot, ly);
            let byte = (v * 255.0) as u8;
            img.put_pixel(x, y, image::Rgb([byte, byte, byte]));
        }
    }
    img
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deterministic integer hash (Wang hash variant) returning a value 0..=255.
fn hash2(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(2654435761).wrapping_add(y.wrapping_mul(2246822519));
    h ^= h >> 16;
    h = h.wrapping_mul(0x45d9f3b7);
    h ^= h >> 16;
    h & 0xff
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
