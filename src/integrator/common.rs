//! Shared path-tracing machinery used by every integrator: MIS weighting,
//! Russian roulette, and cosine-weighted hemisphere sampling.

use rand::prelude::*;

use crate::color::Color;
use crate::vec3::Vec3;

/// Power heuristic (β = 2) MIS weight for a technique with PDF `a` competing
/// against a technique with PDF `b`: `a² / (a² + b²)`. Returns 0 (not NaN) when
/// both PDFs are zero.
pub(crate) fn power_heuristic(a: f32, b: f32) -> f32 {
    // Normalize by the larger input before squaring. Squaring raw pdfs overflows
    // f32 above ~1.8e19 (and Inf² = Inf), so `a²/(a²+b²)` collapses to Inf/Inf =
    // NaN for the large/infinite pdfs that light sampling legitimately produces
    // (grazing angles, a tiny distant sphere light). A NaN weight propagates into
    // the pixel's running mean and freezes it black. Dividing through by the max
    // keeps both terms in [0, 1] so the ratio is always finite, while giving the
    // identical result for ordinary magnitudes.
    let m = a.abs().max(b.abs());
    if m == 0.0 || !m.is_finite() {
        // Both zero -> no info (0). Exactly one infinite -> it dominates fully.
        return if a.is_infinite() && !b.is_infinite() {
            1.0
        } else if b.is_infinite() && !a.is_infinite() {
            0.0
        } else {
            0.0 // both zero, or both infinite (indeterminate) -> no weight
        };
    }
    let a2 = (a / m) * (a / m);
    let b2 = (b / m) * (b / m);
    a2 / (a2 + b2)
}

/// Path depth at which Russian roulette begins. Early bounces carry most of the
/// energy, so they always survive (lower variance); only the long, dim tail is
/// stochastically terminated.
pub(crate) const MIN_RR_DEPTH: u32 = 3;

/// Russian roulette: probabilistically terminate a path so its dim tail doesn't
/// run the full `max_depth`, while keeping the estimator unbiased. Below
/// `MIN_RR_DEPTH` the path always continues unchanged. Otherwise it survives
/// with probability `p` — the brightest throughput channel, capped at 0.95 so
/// the path can still die — and survivors are scaled by `1/p` so the expected
/// contribution is unchanged. Returns `None` to terminate the path.
pub(crate) fn russian_roulette(throughput: Color, depth: u32, rng: &mut SmallRng) -> Option<Color> {
    if depth < MIN_RR_DEPTH {
        return Some(throughput);
    }
    let p = throughput.x.max(throughput.y).max(throughput.z).min(0.95);
    if p <= 0.0 || rng.random::<f32>() >= p {
        return None;
    }
    Some(throughput / p)
}

/// Orthonormal basis `(t, b, n)` with `n` along the given axis.
fn onb(axis: &Vec3) -> (Vec3, Vec3, Vec3) {
    let n = axis.unit();
    let a = if n.x.abs() > 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let t = n.cross(&a).unit();
    let b = n.cross(&t);
    (t, b, n)
}

/// Cosine-weighted hemisphere direction about `normal` from a 2D uniform sample
/// `(u, v)` in [0, 1)² (PDF = cos/PI). Malley's polar mapping — the explicit
/// form lets the first-bounce sample be stratified via the R2 sequence. Returns
/// a unit vector.
pub(crate) fn cosine_direction_from_uv(normal: &Vec3, u: f32, v: f32) -> Vec3 {
    let r = u.sqrt();
    let phi = 2.0 * std::f32::consts::PI * v;
    let x = r * phi.cos();
    let y = r * phi.sin();
    let z = (1.0 - u).max(0.0).sqrt();
    let (t, b, n) = onb(normal);
    x * t + y * b + z * n
}

#[cfg(test)]
mod power_heuristic_tests {
    use super::power_heuristic;

    #[test]
    fn beta2_weights() {
        // 3^2 / (3^2 + 4^2) = 9/25 = 0.36
        assert!((power_heuristic(3.0, 4.0) - 0.36).abs() < 1e-6);
    }

    #[test]
    fn dominant_pdf_gets_full_weight() {
        assert!((power_heuristic(5.0, 0.0) - 1.0).abs() < 1e-6);
        assert!(power_heuristic(0.0, 5.0).abs() < 1e-6);
    }

    #[test]
    fn both_zero_is_zero_not_nan() {
        let w = power_heuristic(0.0, 0.0);
        assert_eq!(w, 0.0);
        assert!(!w.is_nan());
    }

    #[test]
    fn huge_or_infinite_pdf_does_not_produce_nan() {
        // Squaring overflows f32 above ~1.8e19, and a light's solid-angle pdf can
        // legitimately blow up (grazing angles, a tiny distant sphere light ->
        // literal Inf). A NaN weight here poisons the pixel's running mean and
        // freezes it black (salt-and-pepper black specks). The dominant pdf must
        // still win cleanly.
        for &(a, b) in &[(1e30_f32, 1.0), (f32::INFINITY, 1.0), (1e25, 1e24)] {
            let w = power_heuristic(a, b);
            assert!(!w.is_nan(), "power_heuristic({a}, {b}) = NaN");
            assert!((0.0..=1.0).contains(&w), "out of range: {w}");
        }
        // a hugely dominant over b -> weight ~1.
        assert!(power_heuristic(f32::INFINITY, 1.0) > 0.99);
        // b hugely dominant over a -> weight ~0.
        assert!(power_heuristic(1.0, f32::INFINITY) < 0.01);
    }
}

#[cfg(test)]
mod russian_roulette_tests {
    use super::*;
    use crate::color::Color;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    #[test]
    fn always_continues_unchanged_before_min_depth() {
        let mut rng = SmallRng::seed_from_u64(1);
        let t = Color::new(0.1, 0.2, 0.05);
        for depth in 0..MIN_RR_DEPTH {
            assert_eq!(
                russian_roulette(t, depth, &mut rng),
                Some(t),
                "should not roulette at depth {depth}"
            );
        }
    }

    #[test]
    fn preserves_expected_throughput() {
        // The estimator must stay unbiased: averaging the (scaled) survivors,
        // counting terminations as zero, recovers the original throughput.
        let mut rng = SmallRng::seed_from_u64(7);
        let t = Color::new(0.6, 0.3, 0.45);
        let n = 400_000;
        let mut sum = Color::ZERO;
        for _ in 0..n {
            if let Some(scaled) = russian_roulette(t, MIN_RR_DEPTH, &mut rng) {
                sum += scaled;
            }
        }
        let mean = sum / n as f32;
        assert!((mean.x - t.x).abs() < 0.01, "x mean {} vs {}", mean.x, t.x);
        assert!((mean.y - t.y).abs() < 0.01, "y mean {} vs {}", mean.y, t.y);
        assert!((mean.z - t.z).abs() < 0.01, "z mean {} vs {}", mean.z, t.z);
    }

    #[test]
    fn terminates_a_dead_path() {
        // Zero throughput can contribute nothing, so it should always terminate.
        let mut rng = SmallRng::seed_from_u64(3);
        assert_eq!(russian_roulette(Color::ZERO, MIN_RR_DEPTH, &mut rng), None);
    }
}

#[cfg(test)]
mod cosine_uv_tests {
    use super::*;
    use crate::vec3::Vec3;

    #[test]
    fn samples_are_unit_and_in_the_hemisphere() {
        let n = Vec3::new(0.0, 1.0, 0.0);
        for a in 0..16 {
            for b in 0..16 {
                let (u, v) = (a as f32 / 16.0, b as f32 / 16.0);
                let d = cosine_direction_from_uv(&n, u, v);
                assert!((d.length() - 1.0).abs() < 1e-4, "not unit: {d:?}");
                assert!(d.dot(&n) >= -1e-4, "below hemisphere: {d:?}");
            }
        }
    }

    #[test]
    fn u_zero_points_along_the_normal() {
        let n = Vec3::new(0.0, 0.0, 1.0);
        let d = cosine_direction_from_uv(&n, 0.0, 0.37);
        assert!(d.dot(&n) > 0.999, "expected ~normal, got {d:?}");
    }

    #[test]
    fn distribution_is_cosine_weighted() {
        // E[cos theta] for a cosine-weighted hemisphere is 2/3.
        let n = Vec3::new(0.0, 1.0, 0.0);
        let m = 200;
        let mut sum = 0.0;
        for a in 0..m {
            for b in 0..m {
                let (u, v) = ((a as f32 + 0.5) / m as f32, (b as f32 + 0.5) / m as f32);
                sum += cosine_direction_from_uv(&n, u, v).dot(&n);
            }
        }
        let mean = sum / (m * m) as f32;
        assert!((mean - 2.0 / 3.0).abs() < 5e-3, "mean cos={mean}");
    }
}
