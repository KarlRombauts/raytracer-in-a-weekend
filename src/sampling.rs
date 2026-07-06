//! Low-discrepancy sub-pixel sampling.
//!
//! Replaces white-noise pixel jitter with the R2 sequence (Martin Roberts,
//! 2018), decorrelated per pixel via a Cranley-Patterson rotation so the same
//! offset pattern is not reused across pixels.

/// R2 low-discrepancy point for `index`, each component in [0, 1).
pub fn r2(index: u32) -> (f32, f32) {
    // Plastic-constant multipliers (the 2D golden-ratio analogue). The product
    // is formed in f64 so the fractional part survives for large indices: an
    // f32 `index` loses integer precision past 2^24, which would silently
    // collapse every sample onto the pixel center.
    let x = (index as f64 * 0.7548776662).fract() as f32;
    let y = (index as f64 * 0.5698402909).fract() as f32;
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

/// Per-dimension irrational multipliers for the Kronecker (additive recurrence)
/// low-discrepancy sequences. dim 0 keeps the 2D plastic constants (so it equals
/// `r2` and sub-pixel AA is unchanged); dims 1+ use successive components of the
/// generalized-golden (6D plastic) sequence. Distinct multipliers per dimension
/// mean the dimensions are *genuinely* independent, not rigid shifts of one
/// another — so the joint (sub-pixel, light, bounce) samples fill their space
/// instead of collapsing onto a degenerate subspace.
const DIM_MULT: [(f64, f64); 3] = [
    (0.7548776662, 0.5698402909), // dim 0 — 2D plastic (exactly r2), AA unchanged
    (0.8986537126, 0.8075784952), // dim 1 — NEE light sample
    (0.7257349836, 0.6519350658), // dim 2 — first BSDF bounce
];

/// 2D Kronecker point for `index` along sampling `dim` (each component in
/// [0, 1)), using that dimension's own multipliers. Products are formed in f64
/// so the fractional part survives large indices.
fn kronecker_2d(index: u32, dim: u32) -> (f32, f32) {
    let (ax, ay) = DIM_MULT[(dim as usize).min(DIM_MULT.len() - 1)];
    (
        ((index as f64) * ax).fract() as f32,
        ((index as f64) * ay).fract() as f32,
    )
}

/// A stratified low-discrepancy point in [0, 1)² for sample `sample_index` of
/// pixel `(i, j)`, along an independent sampling `dim`ension (0 = sub-pixel AA,
/// 1 = NEE light sample, 2 = first BSDF bounce, ...). Each dimension uses its
/// own Kronecker sequence (distinct multipliers) plus its own per-pixel
/// Cranley-Patterson rotation, so the dimensions are decorrelated both within a
/// pixel and across pixels.
pub fn stratified_unit(i: u32, j: u32, sample_index: u32, dim: u32) -> (f32, f32) {
    let (rx, ry) = kronecker_2d(sample_index, dim);
    let (ox, oy) = hash01(
        i.wrapping_add(dim.wrapping_mul(0x9E3779B1)),
        j.wrapping_add(dim.wrapping_mul(0x85EBCA77)),
    );
    ((rx + ox).fract(), (ry + oy).fract())
}

/// Sub-pixel offset for sample `sample_index` of pixel `(i, j)`, each component
/// in [-0.5, 0.5). This is the camera's per-sample anti-aliasing jitter (the
/// `dim = 0` stratified point, recentred on the pixel).
pub fn stratified_offset(i: u32, j: u32, sample_index: u32) -> (f32, f32) {
    let (x, y) = stratified_unit(i, j, sample_index, 0);
    (x - 0.5, y - 0.5)
}

/// The identity of one Monte-Carlo sample: its pixel `(i, j)` and its per-pixel
/// sample `index`. Carried across the render seam so the camera (dim 0, AA) and
/// the integrator (dims 1+, light/bounce) draw from one consistent coordinate.
/// It is an identity, not a screen position — nothing spatial is derived from it.
#[derive(Clone, Copy, Debug)]
pub struct SampleId {
    pub i: u32,
    pub j: u32,
    pub index: u32,
}

impl SampleId {
    /// Stratified low-discrepancy point in [0, 1)² for sampling dimension `dim`
    /// (0 = sub-pixel AA, 1 = NEE light sample, 2 = first BSDF bounce).
    pub fn stratified(&self, dim: u32) -> (f32, f32) {
        stratified_unit(self.i, self.j, self.index, dim)
    }
}

/// A piecewise-constant 1D distribution over `n` non-negative weights. Samples a
/// continuous position in [0, 1) with probability proportional to weight — the
/// inversion method (see PBRT §13.3): build the cumulative array, then binary-
/// search a uniform draw. Also reports the pdf at any position, so a sampled
/// value can be MIS-weighted.
pub struct Distribution1D {
    /// Normalized cumulative distribution, length `n + 1`, from 0.0 to 1.0.
    cdf: Vec<f32>,
    /// The piecewise-constant weights (the unnormalized "function").
    func: Vec<f32>,
    /// Average weight = Σfunc / n. The normalizing constant for the pdf.
    func_int: f32,
}

impl Distribution1D {
    pub fn new(weights: &[f32]) -> Self {
        let n = weights.len();
        let func = weights.to_vec();
        // Unnormalized CDF: cdf[i+1] = cdf[i] + func[i]/n.
        let mut cdf = vec![0.0f32; n + 1];
        for i in 0..n {
            cdf[i + 1] = cdf[i] + func[i] / n as f32;
        }
        let func_int = cdf[n]; // = Σfunc / n
        // Normalize to [0, 1]. If every weight is zero, fall back to a uniform CDF
        // so sampling still returns a valid position instead of dividing by zero.
        if func_int == 0.0 {
            for (i, c) in cdf.iter_mut().enumerate() {
                *c = i as f32 / n as f32;
            }
        } else {
            for c in cdf.iter_mut() {
                *c /= func_int;
            }
        }
        Distribution1D { cdf, func, func_int }
    }

    /// Sample a continuous position `x` in [0, 1) from a uniform `u` in [0, 1).
    /// Returns `(x, pdf, index)`: the position, its pdf (per unit length), and the
    /// bin it fell in.
    pub fn sample_continuous(&self, u: f32) -> (f32, f32, usize) {
        let n = self.func.len();
        // Largest `offset` with cdf[offset] <= u (binary search over the sorted CDF).
        let offset = self.find_interval(u);
        // Fractional position within the bin.
        let mut du = u - self.cdf[offset];
        let span = self.cdf[offset + 1] - self.cdf[offset];
        if span > 0.0 {
            du /= span;
        }
        let pdf = if self.func_int > 0.0 {
            self.func[offset] / self.func_int
        } else {
            1.0 // uniform fallback
        };
        let x = (offset as f32 + du) / n as f32;
        (x, pdf, offset)
    }

    /// The pdf (per unit length in [0, 1)) at continuous position `x`.
    pub fn pdf(&self, x: f32) -> f32 {
        if self.func_int <= 0.0 {
            return 1.0;
        }
        let n = self.func.len();
        let i = ((x * n as f32) as usize).min(n - 1);
        self.func[i] / self.func_int
    }

    /// Average weight (Σfunc / n) — the distribution's integral, used to combine
    /// marginal and conditional distributions in 2D.
    pub fn integral(&self) -> f32 {
        self.func_int
    }

    fn find_interval(&self, u: f32) -> usize {
        // Binary search for the last index whose cdf value is <= u, clamped so the
        // returned bin is always a valid [offset, offset+1) pair.
        let n = self.func.len();
        let (mut lo, mut hi) = (0usize, self.cdf.len());
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.cdf[mid] <= u {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo.min(n - 1)
    }
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
    fn stratified_unit_stays_in_unit_square() {
        for index in 0..256 {
            for dim in 0..4 {
                let (u, v) = stratified_unit(2, 9, index, dim);
                assert!((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v), "dim {dim}: ({u},{v})");
            }
        }
    }

    #[test]
    fn stratified_unit_dimensions_are_decorrelated() {
        // Same pixel & sample index, different dimensions => different points,
        // so the light sample and bounce sample don't track each other.
        let light = stratified_unit(4, 5, 7, 1);
        let bounce = stratified_unit(4, 5, 7, 2);
        assert!(light != bounce, "dims 1 and 2 collide: {light:?}");
    }

    #[test]
    fn dimensions_are_not_rigid_shifts_of_each_other() {
        // A per-pixel *rotation* alone leaves dim d a fixed toroidal shift of
        // dim 0 (dim_d(s) - dim_0(s) constant for all s), collapsing the joint
        // (sub-pixel, light, bounce) samples onto a degenerate subspace. The
        // offset between dim 0 and dim 2 must instead VARY across sample index.
        let off = |s: u32| {
            let (a, _) = stratified_unit(3, 4, s, 0);
            let (b, _) = stratified_unit(3, 4, s, 2);
            (b - a).rem_euclid(1.0)
        };
        let (o1, o2, o3) = (off(1), off(2), off(37));
        assert!(
            (o1 - o2).abs() > 1e-4 || (o1 - o3).abs() > 1e-4,
            "dim 2 is a rigid shift of dim 0 (degenerate joint sampling): {o1} {o2} {o3}"
        );
    }

    #[test]
    fn dim_zero_still_equals_r2_so_aa_is_unchanged() {
        for s in [0u32, 1, 2, 100, 5000, 60000] {
            let (kx, ky) = kronecker_2d(s, 0);
            let (rx, ry) = r2(s);
            assert!((kx - rx).abs() < 1e-6 && (ky - ry).abs() < 1e-6, "dim0 != r2 at {s}");
        }
    }

    #[test]
    fn stratified_offset_matches_dim_zero() {
        // AA (dim 0) is unchanged by the refactor: offset == unit(dim 0) - 0.5.
        let (ux, uy) = stratified_unit(3, 7, 11, 0);
        let (dx, dy) = stratified_offset(3, 7, 11);
        assert!((dx - (ux - 0.5)).abs() < 1e-6 && (dy - (uy - 0.5)).abs() < 1e-6);
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

    #[test]
    fn distribution1d_samples_proportional_to_weights() {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};
        // Weights [1, 3]: bin 1 (x >= 0.5) should be chosen ~3× as often as bin 0.
        let d = Distribution1D::new(&[1.0, 3.0]);
        let mut rng = SmallRng::seed_from_u64(1);
        let (mut n0, mut n1) = (0u32, 0u32);
        for _ in 0..40_000 {
            let (x, _pdf, _i) = d.sample_continuous(rng.random());
            if x < 0.5 {
                n0 += 1;
            } else {
                n1 += 1;
            }
        }
        let ratio = n1 as f32 / n0 as f32;
        assert!((ratio - 3.0).abs() < 0.15, "expected ~3:1, got {ratio} ({n0} vs {n1})");
    }

    #[test]
    fn distribution1d_pdf_is_piecewise_constant_and_integrates_to_one() {
        // Worked from the definition: funcInt = mean(func) = (1+3)/2 = 2, so the
        // pdf is func[i]/funcInt: bin0 = 0.5, bin1 = 1.5. Over [0,1) each bin has
        // width 0.5, so the integral is 0.5·0.5 + 1.5·0.5 = 1.
        let d = Distribution1D::new(&[1.0, 3.0]);
        assert!((d.pdf(0.25) - 0.5).abs() < 1e-5, "pdf(0.25)={}", d.pdf(0.25));
        assert!((d.pdf(0.75) - 1.5).abs() < 1e-5, "pdf(0.75)={}", d.pdf(0.75));
        let integral = 0.5 * d.pdf(0.25) + 0.5 * d.pdf(0.75);
        assert!((integral - 1.0).abs() < 1e-5, "integral={integral}");
    }

    #[test]
    fn distribution1d_sample_reports_its_own_pdf() {
        // The pdf returned alongside a sample must equal pdf(x) at that x.
        let d = Distribution1D::new(&[2.0, 1.0, 5.0, 1.0]);
        for &u in &[0.05f32, 0.3, 0.55, 0.8, 0.99] {
            let (x, pdf, _i) = d.sample_continuous(u);
            assert!((pdf - d.pdf(x)).abs() < 1e-4, "u={u}: sample pdf {pdf} vs pdf(x) {}", d.pdf(x));
        }
    }

    #[test]
    fn sample_id_stratified_maps_its_fields_to_the_sampler() {
        // The bundle must forward (i, j, index) in the right order for each dim,
        // so a field-order slip (e.g. i/j swapped) is caught.
        let id = SampleId { i: 4, j: 5, index: 7 };
        assert_eq!(id.stratified(0), stratified_unit(4, 5, 7, 0));
        assert_eq!(id.stratified(1), stratified_unit(4, 5, 7, 1));
        assert_eq!(id.stratified(2), stratified_unit(4, 5, 7, 2));
    }
}
