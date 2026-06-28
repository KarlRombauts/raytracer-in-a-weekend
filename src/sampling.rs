//! Low-discrepancy sub-pixel sampling.
//!
//! Replaces white-noise pixel jitter with the R2 sequence (Martin Roberts,
//! 2018), decorrelated per pixel via a Cranley-Patterson rotation so the same
//! offset pattern is not reused across pixels.

/// R2 low-discrepancy point for `index`, each component in [0, 1).
pub fn r2(index: u32) -> (f32, f32) {
    // Plastic-constant multipliers (the 2D golden-ratio analogue).
    let x = (index as f32 * 0.7548776662).fract();
    let y = (index as f32 * 0.5698402909).fract();
    (x, y)
}

/// Deterministic per-pixel offset in [0, 1), used to rotate the R2 sequence so
/// neighbouring pixels do not share the same sample positions. Stable across
/// passes (depends only on the pixel coordinates).
pub fn hash01(i: u32, j: u32) -> (f32, f32) {
    let hx = wang_hash(i.wrapping_mul(0x9E3779B1) ^ j);
    let hy = wang_hash(j.wrapping_mul(0x85EBCA77) ^ i.wrapping_add(0x165667B1));
    (
        hx as f32 / u32::MAX as f32,
        hy as f32 / u32::MAX as f32,
    )
}

fn wang_hash(mut x: u32) -> u32 {
    x = (x ^ 61) ^ (x >> 16);
    x = x.wrapping_mul(9);
    x = x ^ (x >> 4);
    x = x.wrapping_mul(0x27D4EB2D);
    x = x ^ (x >> 15);
    x
}

/// Sub-pixel offset for sample `sample_index` of pixel `(i, j)`, each component
/// in [-0.5, 0.5). Drop-in replacement for the old random `sample_square`.
pub fn stratified_offset(i: u32, j: u32, sample_index: u32) -> (f32, f32) {
    let (rx, ry) = r2(sample_index);
    let (ox, oy) = hash01(i, j);
    let dx = (rx + ox).fract() - 0.5;
    let dy = (ry + oy).fract() - 0.5;
    (dx, dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r2_stays_in_unit_square() {
        for index in 0..1000 {
            let (x, y) = r2(index);
            assert!((0.0..1.0).contains(&x), "x out of range at {index}: {x}");
            assert!((0.0..1.0).contains(&y), "y out of range at {index}: {y}");
        }
    }

    #[test]
    fn r2_first_values_match_known_sequence() {
        // index 0 -> (0,0); index 1 -> the two multipliers themselves.
        let (x0, y0) = r2(0);
        assert!(x0.abs() < 1e-6 && y0.abs() < 1e-6);
        let (x1, y1) = r2(1);
        assert!((x1 - 0.7548776662).abs() < 1e-5, "x1={x1}");
        assert!((y1 - 0.5698402909).abs() < 1e-5, "y1={y1}");
    }

    #[test]
    fn stratified_offset_stays_within_pixel() {
        for index in 0..256 {
            let (dx, dy) = stratified_offset(3, 7, index);
            assert!((-0.5..0.5).contains(&dx), "dx out of range: {dx}");
            assert!((-0.5..0.5).contains(&dy), "dy out of range: {dy}");
        }
    }

    #[test]
    fn different_pixels_get_different_rotations() {
        // Same sample index, different pixels => different offsets (decorrelated).
        let a = stratified_offset(0, 0, 5);
        let b = stratified_offset(1, 0, 5);
        let c = stratified_offset(0, 1, 5);
        assert!(a != b, "pixels (0,0) and (1,0) collide: {a:?}");
        assert!(a != c, "pixels (0,0) and (0,1) collide: {a:?}");
    }
}
