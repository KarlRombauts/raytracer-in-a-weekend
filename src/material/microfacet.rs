//! GGX (Trowbridge–Reitz) microfacet reflection.
//!
//! Replaces the ad-hoc "perfect reflection + random fuzz" with a physically
//! based rough-reflection model: a microfacet normal is importance-sampled from
//! the GGX visible-normal distribution (Heitz 2018), the ray reflects about it,
//! and the throughput is weighted by the Smith masking-shadowing term. This
//! gives the characteristic bright highlight core with an energy-conserving soft
//! falloff, instead of the shapeless blur of the fuzz model.
//!
//! Fresnel is left to the caller — `F0` differs per material (a metal's colour,
//! a clear coat's 0.04) — so these helpers return the half-angle cosine for it.

use crate::vec3::Vec3;
use rand::rngs::SmallRng;
use rand::Rng;

/// Below this roughness GGX is treated as a perfect mirror: the distribution
/// collapses toward a delta and the VNDF sampling would just churn numerically.
pub const MIRROR_ALPHA: f32 = 1e-3;

/// Smith Λ for GGX, from a direction's cosine-to-normal and roughness `alpha`.
fn smith_lambda(cos_theta: f32, alpha: f32) -> f32 {
    let c2 = (cos_theta * cos_theta).clamp(1e-8, 1.0);
    let tan2 = (1.0 - c2) / c2;
    0.5 * (-1.0 + (1.0 + alpha * alpha * tan2).sqrt())
}

/// Sample a GGX microfacet normal from the distribution of *visible* normals for
/// view direction `ve` (local space, surface normal = +z, `ve.z > 0`). Heitz,
/// "Sampling the GGX Distribution of Visible Normals" (2018). Returns a unit
/// half-vector in the upper hemisphere.
fn sample_vndf(ve: Vec3, alpha: f32, u1: f32, u2: f32) -> Vec3 {
    use std::f32::consts::PI;
    // Stretch the view direction into the hemisphere configuration.
    let vh = Vec3::new(alpha * ve.x, alpha * ve.y, ve.z).unit();
    // Orthonormal basis around vh.
    let lensq = vh.x * vh.x + vh.y * vh.y;
    let t1 = if lensq > 1e-8 {
        Vec3::new(-vh.y, vh.x, 0.0) * (1.0 / lensq.sqrt())
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let t2 = vh.cross(&t1);
    // Sample a point on the projected (squashed) disk.
    let r = u1.sqrt();
    let phi = 2.0 * PI * u2;
    let p1 = r * phi.cos();
    let mut p2 = r * phi.sin();
    let s = 0.5 * (1.0 + vh.z);
    p2 = (1.0 - s) * (1.0 - p1 * p1).max(0.0).sqrt() + s * p2;
    // Reproject onto the hemisphere and un-stretch.
    let nh = p1 * t1 + p2 * t2 + (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt() * vh;
    Vec3::new(alpha * nh.x, alpha * nh.y, nh.z.max(0.0)).unit()
}

/// Orthonormal basis `(t, b, n)` with `n` along the given (unit) axis.
fn basis(n: &Vec3) -> (Vec3, Vec3, Vec3) {
    let a = if n.x.abs() > 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let t = n.cross(&a).unit();
    let b = n.cross(&t);
    (t, b, *n)
}

/// Importance-sample a GGX reflection. `ray_dir` is the incoming direction (into
/// the surface); `n` is the (unit) shading normal facing the ray; `alpha` is the
/// GGX roughness. Returns `(outgoing_world_dir, half_angle_cosine, g2_over_g1)`,
/// or `None` if the reflection falls below the surface. The caller multiplies
/// throughput by `fresnel(half_angle_cosine) * g2_over_g1`.
pub fn ggx_reflect(
    ray_dir: &Vec3,
    n: &Vec3,
    alpha: f32,
    rng: &mut SmallRng,
) -> Option<(Vec3, f32, f32)> {
    let (t, b, n) = basis(n);
    let v = -ray_dir.unit(); // toward the viewer
    let v_local = Vec3::new(v.dot(&t), v.dot(&b), v.dot(&n));
    if v_local.z <= 0.0 {
        return None;
    }
    let m = sample_vndf(v_local, alpha, rng.random::<f32>(), rng.random::<f32>());
    // Reflect the view about the microfacet normal to get the light direction.
    let wo_local = 2.0 * v_local.dot(&m) * m - v_local;
    if wo_local.z <= 0.0 {
        return None; // reflected below the surface
    }
    let cos_d = v_local.dot(&m).clamp(0.0, 1.0); // = wo·m, the half-angle cosine
    let lambda_v = smith_lambda(v_local.z, alpha);
    let lambda_l = smith_lambda(wo_local.z, alpha);
    let g2_over_g1 = (1.0 + lambda_v) / (1.0 + lambda_v + lambda_l);
    let wo_world = wo_local.x * t + wo_local.y * b + wo_local.z * n;
    Some((wo_world, cos_d, g2_over_g1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn lambda_is_zero_head_on() {
        // cosθ = 1 → no tilt → no masking.
        assert!(smith_lambda(1.0, 0.3).abs() < 1e-6);
        // Rougher and more grazing → more masking.
        assert!(smith_lambda(0.3, 0.6) > smith_lambda(0.3, 0.2));
    }

    #[test]
    fn sampled_half_vector_is_unit_and_upper_hemisphere() {
        let mut rng = SmallRng::seed_from_u64(1);
        let ve = Vec3::new(0.3, 0.1, 0.95).unit();
        for _ in 0..2000 {
            let m = sample_vndf(ve, 0.4, rng.random(), rng.random());
            assert!((m.length() - 1.0).abs() < 1e-4, "not unit: {m:?}");
            assert!(m.z >= 0.0, "below hemisphere: {m:?}");
        }
    }

    #[test]
    fn reflection_stays_above_surface_and_conserves_energy() {
        let mut rng = SmallRng::seed_from_u64(7);
        let n = Vec3::new(0.0, 1.0, 0.0);
        let ray_dir = Vec3::new(0.4, -1.0, 0.2).unit(); // hitting the surface from above
        let mut hits = 0;
        for _ in 0..3000 {
            if let Some((wo, cos_d, g)) = ggx_reflect(&ray_dir, &n, 0.35, &mut rng) {
                hits += 1;
                assert!(wo.dot(&n) > -1e-4, "below surface: {wo:?}");
                assert!((0.0..=1.0).contains(&cos_d), "bad cos_d {cos_d}");
                // Smith G2/G1 is a (0,1] attenuation — never amplifies energy.
                assert!(g > 0.0 && g <= 1.0 + 1e-4, "g2/g1 out of range: {g}");
            }
        }
        assert!(hits > 2500, "too many reflections rejected: {hits}/3000");
    }

    #[test]
    fn near_zero_roughness_approaches_the_mirror_direction() {
        let mut rng = SmallRng::seed_from_u64(3);
        let n = Vec3::new(0.0, 1.0, 0.0);
        let ray_dir = Vec3::new(0.5, -1.0, 0.0).unit();
        let mirror = Vec3::reflect(&ray_dir, &n).unit();
        let (wo, _, _) = ggx_reflect(&ray_dir, &n, MIRROR_ALPHA, &mut rng).unwrap();
        assert!(wo.unit().dot(&mirror) > 0.999, "not near-mirror: {wo:?} vs {mirror:?}");
    }
}
