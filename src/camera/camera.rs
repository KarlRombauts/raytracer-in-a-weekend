use core::f32;
use rand::prelude::*;
use rayon::prelude::*;
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
use indicatif::{ProgressBar, ProgressStyle};

use image;

use crate::camera::config::CameraConfig;
use crate::color::{clamp_luminance, Color};
use crate::group::*;
use crate::interval::Interval;
use crate::ray::{Intersect, Ray};
use crate::sampling::{stratified_offset, stratified_unit};
use crate::vec3::{Point3, Vec3};

pub struct Camera {
    // specified by config
    image_width: u32,
    samples: u32,
    max_depth: u32,
    dof_angle: f32,
    background: Color,
    firefly_clamp: f32,

    // derived:
    pixel_samples_scale: f32,
    image_height: u32,
    center: Point3,
    pixel00_loc: Point3,
    pixel_delta_u: Vec3,
    pixel_delta_v: Vec3,
    u: Vec3,
    v: Vec3,
    w: Vec3,

    dof_disk_u: Vec3,
    dof_disk_v: Vec3,
    #[allow(dead_code)]
    basis_u: Vec3,
}

impl From<CameraConfig> for Camera {
    fn from(config: CameraConfig) -> Self {
        // derived
        let pixel_samples_scale = 1. / config.samples as f32;
        let image_height = ((config.image_width as f64 / config.aspect_ratio) as u32).max(1);
        let center = config.look_from;

        let theta = config.fov.to_radians();
        let h = (theta / 2.0).tan();
        let viewport_height = 2.0 * h * config.focus_dist;
        let viewport_width = viewport_height * (config.image_width as f32) / (image_height as f32);

        let w = (config.look_from - config.look_at).unit();
        // Roll spins the up reference about the view axis before deriving right.
        let up = config.v_up.rotate_about_axis(&w, config.roll.to_radians());
        let u = up.cross(&w).unit();
        let v = w.cross(&u);

        let viewport_u = viewport_width * u;
        let viewport_v = viewport_height * -v;

        let pixel_delta_u = viewport_u / config.image_width as f32;
        let pixel_delta_v = viewport_v / image_height as f32;

        let viewport_upper_left =
            center - (config.focus_dist * w) - (viewport_u / 2.0) - (viewport_v / 2.0);

        let dof_radius = config.focus_dist * (config.dof_angle / 2.).to_radians().tan();
        let pixel00_loc = viewport_upper_left + 0.5 * (pixel_delta_v + pixel_delta_u);
        let dof_disk_u = u * dof_radius;
        let dof_disk_v = v * dof_radius;

        Camera {
            image_width: config.image_width,
            samples: config.samples,
            max_depth: config.max_depth,
            dof_angle: config.dof_angle,
            background: config.background,
            firefly_clamp: config.firefly_clamp,

            pixel_samples_scale,
            image_height,
            center,
            pixel00_loc,
            pixel_delta_u,
            pixel_delta_v,
            u,
            v,
            w,
            dof_disk_u,
            dof_disk_v,
            basis_u: u,
        }
    }
}

/// Power heuristic (β = 2) MIS weight for a technique with PDF `a` competing
/// against a technique with PDF `b`: `a² / (a² + b²)`. Returns 0 (not NaN) when
/// both PDFs are zero.
fn power_heuristic(a: f32, b: f32) -> f32 {
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
const MIN_RR_DEPTH: u32 = 3;

/// Russian roulette: probabilistically terminate a path so its dim tail doesn't
/// run the full `max_depth`, while keeping the estimator unbiased. Below
/// `MIN_RR_DEPTH` the path always continues unchanged. Otherwise it survives
/// with probability `p` — the brightest throughput channel, capped at 0.95 so
/// the path can still die — and survivors are scaled by `1/p` so the expected
/// contribution is unchanged. Returns `None` to terminate the path.
fn russian_roulette(throughput: Color, depth: u32, rng: &mut SmallRng) -> Option<Color> {
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
fn cosine_direction_from_uv(normal: &Vec3, u: f32, v: f32) -> Vec3 {
    let r = u.sqrt();
    let phi = 2.0 * std::f32::consts::PI * v;
    let x = r * phi.cos();
    let y = r * phi.sin();
    let z = (1.0 - u).max(0.0).sqrt();
    let (t, b, n) = onb(normal);
    x * t + y * b + z * n
}

impl Camera {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render(&self, world: &IntersectGroup) {
        let start = Instant::now();
        let bar = ProgressBar::new(self.image_height as u64 * self.image_width as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] [{bar:40.cyan/blue}] {percent}% ({eta_precise})",
            )
            .unwrap()
            .progress_chars("#>-"),
        );
        let mut img_buf = image::ImageBuffer::new(self.image_width, self.image_height);
        img_buf
            .par_enumerate_pixels_mut()
            .for_each(|(i, j, pixel)| {
                // Per-pixel PRNG, seeded deterministically from the pixel coords so
                // renders are reproducible and threads never share RNG state.
                let mut rng = SmallRng::seed_from_u64(((j as u64) << 32) | i as u64);
                let mut pixel_color = Color::ZERO;
                for s in 0..self.samples {
                    let ray = self.get_ray(i, j, s, &mut rng);
                    let light_uv = stratified_unit(i, j, s, 1);
                    let bounce_uv = stratified_unit(i, j, s, 2);
                    pixel_color += clamp_luminance(
                        self.ray_color(&ray, world, light_uv, bounce_uv, &mut rng),
                        self.firefly_clamp,
                    );
                }

                pixel_color *= self.pixel_samples_scale;

                *pixel = image::Rgb(pixel_color.to_rgb_vec());
                bar.inc(1);
            });

        bar.finish();
        let duration = start.elapsed();

        eprintln!(
            "Render complete: {:.3} secs ({}×{} pixels, {} samples)",
            duration.as_secs_f64(),
            self.image_width,
            self.image_height,
            self.samples
        );
        img_buf.save("test.png").unwrap();
    }

    pub fn image_width(&self) -> u32 {
        self.image_width
    }

    pub fn image_height(&self) -> u32 {
        self.image_height
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Trace a single sample for pixel (i, j). The progressive renderer calls
    /// this once per pass and averages the results itself.
    pub fn sample_pixel(
        &self,
        i: u32,
        j: u32,
        sample_index: u32,
        world: &IntersectGroup,
        rng: &mut SmallRng,
    ) -> Color {
        let ray = self.get_ray(i, j, sample_index, rng);
        // Stratify the light sample (dim 1) and first bounce (dim 2) alongside
        // the sub-pixel AA (dim 0), decorrelated per pixel.
        let light_uv = stratified_unit(i, j, sample_index, 1);
        let bounce_uv = stratified_unit(i, j, sample_index, 2);
        clamp_luminance(
            self.ray_color(&ray, world, light_uv, bounce_uv, rng),
            self.firefly_clamp,
        )
    }

    fn dof_disk_sample(&self, rng: &mut SmallRng) -> Vec3 {
        let p = Vec3::random_in_unit_disk(rng);
        return self.center + (p.x * self.dof_disk_u) + (p.y * self.dof_disk_v);
    }

    fn get_ray(&self, i: u32, j: u32, sample_index: u32, rng: &mut SmallRng) -> Ray {
        let (dx, dy) = stratified_offset(i, j, sample_index);
        let pixel_sample = self.pixel00_loc
            + ((i as f32 + dx) * self.pixel_delta_u)
            + ((j as f32 + dy) * self.pixel_delta_v);

        let ray_origin = if self.dof_angle <= 0. {
            self.center
        } else {
            self.dof_disk_sample(rng)
        };
        let ray_direction = pixel_sample - ray_origin;

        let ray_time = rng.random::<f32>();
        Ray::new_t(ray_origin, ray_direction, ray_time)
    }

    #[cfg(test)]
    pub(crate) fn basis_u(&self) -> crate::vec3::Vec3 {
        self.basis_u
    }

    fn ray_color(
        &self,
        ray: &Ray,
        world: &IntersectGroup,
        light_uv: (f32, f32),
        bounce_uv: (f32, f32),
        rng: &mut SmallRng,
    ) -> Color {
        let interval = Interval::new(0.001, f32::INFINITY);
        let mut color = Color::ZERO;
        let mut throughput = Color::ones();
        let mut current = ray.clone();
        // Whether emission at the next hit gets full weight: true for the camera ray
        // and after specular bounces (NEE can't sample those), false after a diffuse
        // bounce (NEE already accounted for direct light, so MIS-weight the emission).
        let mut specular_bounce = true;
        let mut prev_brdf_pdf = 0.0_f32;

        for depth in 0..self.max_depth {
            let Some(hit) = world.intersect(&current, &interval) else {
                color += throughput * self.background;
                break;
            };

            let emitted = hit.material.emitted(hit.u, hit.v, hit.p);
            if emitted != Color::ZERO {
                if specular_bounce {
                    color += throughput * emitted;
                } else {
                    let p_light = world.light_pdf(current.origin, current.direction);
                    let w = power_heuristic(prev_brdf_pdf, p_light);
                    color += throughput * emitted * w;
                }
            }

            let Some((scattered, atten, specular)) = hit.material.scatter_lobe(&current, &hit, rng)
            else {
                break; // pure light / absorber
            };

            // `specular` is the *sampled lobe*, not the material: a Glossy hit is
            // specular when it took its coat reflection and diffuse when it took
            // its base, so the base gets next-event estimation like a Lambertian.
            if specular {
                throughput = throughput * atten;
                current = scattered;
                specular_bounce = true;
                continue;
            }

            // Lambertian.
            let albedo = atten;

            // Stratify the NEE light sample and the BSDF bounce on the camera
            // ray's first (diffuse) hit; deeper bounces fall back to plain rng.
            let (lu, lv) = if depth == 0 {
                light_uv
            } else {
                (rng.random(), rng.random())
            };
            let (bu, bv) = if depth == 0 {
                bounce_uv
            } else {
                (rng.random(), rng.random())
            };

            // (1) Next-event estimation: sample a light, weight against the BRDF pdf.
            if let Some(ld) = world.sample_light_dir(hit.p, lu, lv, rng) {
                let p_light = world.light_pdf(hit.p, ld);
                let p_brdf = hit.material.scattering_pdf(&hit, &ld);
                if p_light > 0.0 && p_brdf > 0.0 {
                    let shadow = Ray::new_t(hit.p, ld, current.time);
                    if let Some(lh) = world.intersect(&shadow, &interval) {
                        // Radiance from whatever the shadow ray actually reaches:
                        // an occluder (or non-emitter) gives 0 = shadowed; a light
                        // gives its emission. `p_light` is the marginal over ALL
                        // lights, so even if the ray reaches a different light than
                        // the one sampled, the estimator stays unbiased — don't
                        // "fix" this to use the sampled light's own pdf.
                        let le = lh.material.emitted(lh.u, lh.v, lh.p);
                        if le != Color::ZERO {
                            let w = power_heuristic(p_light, p_brdf);
                            color += throughput * w * albedo * (p_brdf / p_light) * le;
                        }
                    }
                }
            }

            // (2) BRDF bounce (cosine), weighted against the light pdf at the next hit.
            let dir = cosine_direction_from_uv(&hit.normal, bu, bv);
            let p_brdf = hit.material.scattering_pdf(&hit, &dir);
            if p_brdf <= 0.0 {
                break;
            }
            throughput = throughput * albedo;
            // Russian roulette: terminate the dim tail early (unbiased) instead
            // of always running to `max_depth`.
            match russian_roulette(throughput, depth, rng) {
                Some(t) => throughput = t,
                None => break,
            }
            prev_brdf_pdf = p_brdf;
            specular_bounce = false;
            current = Ray::new_t(hit.p, dir, current.time);
        }

        color
    }
}

#[cfg(test)]
mod mixture_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::group::{IntersectGroup, Light};
    use crate::material::{DiffuseLight, Lambertian};
    use crate::ray::{Intersect, Ray};
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn cam() -> Camera {
        Camera::from(
            CameraConfig::builder()
                .image_width(1)
                .aspect_ratio(1.0)
                .background(Color::ZERO)
                .build(),
        )
    }

    fn floor() -> Arc<dyn Intersect> {
        let mat = Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 0.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    // Large overhead emitter (covers a big solid angle so pure-GI sampling
    // converges with feasible sample counts).
    fn ceiling_light() -> Arc<dyn Intersect> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 2.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    #[test]
    fn camera_sees_emitter_emission() {
        // Ray straight up into the emitter; no floor. The first hit is the
        // emitter: emission is added, scatter() is None, path ends.
        let c = cam();
        let mut world = IntersectGroup::new();
        world.add(ceiling_light());
        let ray = Ray::new(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(1);
        let col = c.ray_color(&ray, &world, (0.5, 0.5), (0.5, 0.5), &mut rng);
        assert!((col.x - 5.0).abs() < 1e-4, "expected emitter color, got {:?}", col);
    }

    fn avg_floor_color(register_light: bool) -> f32 {
        let c = cam();
        let mut world = IntersectGroup::new();
        world.add(floor());
        let light = ceiling_light();
        world.add(light.clone());
        if register_light {
            world.lights.push(Light { geom: light, emit: Color::new(5.0, 5.0, 5.0) });
        }
        // Look straight down at the floor centre.
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let n = 8000;
        let mut sum = 0.0;
        for _ in 0..n {
            let luv = (rng.random::<f32>(), rng.random::<f32>());
            let buv = (rng.random::<f32>(), rng.random::<f32>());
            sum += c.ray_color(&ray, &world, luv, buv, &mut rng).x;
        }
        sum / n as f32
    }

    #[test]
    fn mixture_matches_pure_gi_mean() {
        // With the light registered, the diffuse bounce is mixture-sampled
        // (light + cosine); unregistered, it is pure cosine GI. Both estimators
        // are unbiased, so their means must agree (mixture only cuts variance).
        let with_nee = avg_floor_color(true);
        let pure_gi = avg_floor_color(false);
        assert!(with_nee > 0.0 && pure_gi > 0.0, "both lit: nee={with_nee} gi={pure_gi}");
        let rel = (with_nee - pure_gi).abs() / pure_gi;
        // Tight tolerance (both well-converged on the big light) so a subtle MIS
        // weight bias would actually trip this guard, not just gross errors.
        assert!(rel < 0.05, "means should agree (unbiased): nee={with_nee} gi={pure_gi} rel={rel}");
    }
}

#[cfg(test)]
mod mis_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::group::{IntersectGroup, Light};
    use crate::material::{DiffuseLight, Lambertian};
    use crate::ray::{Intersect, Ray};
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn cam() -> Camera {
        Camera::from(
            CameraConfig::builder()
                .image_width(1)
                .aspect_ratio(1.0)
                .background(Color::ZERO)
                .build(),
        )
    }

    fn floor() -> Arc<dyn Intersect> {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.7, 0.7, 0.7)));
        Arc::new(Quad::new(
            Point3::new(-5.0, 0.0, -5.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            mat,
        ))
    }

    // Small, bright overhead light: pure GI rarely hits it (high variance);
    // NEE samples it every bounce (low variance).
    fn small_light() -> Arc<dyn Intersect> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(40.0, 40.0, 40.0)));
        Arc::new(Quad::new(
            Point3::new(-0.5, 4.0, -0.5),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            mat,
        ))
    }

    // Returns (mean, variance) of the .x channel over `n` samples.
    fn stats(register_light: bool, n: usize) -> (f32, f32) {
        let c = cam();
        let mut world = IntersectGroup::new();
        world.add(floor());
        let l = small_light();
        world.add(l.clone());
        if register_light {
            world.lights.push(Light { geom: l, emit: Color::new(40.0, 40.0, 40.0) });
        }
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let mut rng = SmallRng::seed_from_u64(7);
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        for _ in 0..n {
            let luv = (rng.random::<f32>(), rng.random::<f32>());
            let buv = (rng.random::<f32>(), rng.random::<f32>());
            let x = c.ray_color(&ray, &world, luv, buv, &mut rng).x;
            sum += x;
            sum2 += x * x;
        }
        let mean = sum / n as f32;
        let var = (sum2 / n as f32) - mean * mean;
        (mean, var)
    }

    #[test]
    fn mis_cuts_variance_versus_pure_gi() {
        let n = 4000;
        let (mis_mean, mis_var) = stats(true, n);
        let (gi_mean, gi_var) = stats(false, n);
        // Both lit and (being unbiased) roughly equal in mean.
        assert!(mis_mean > 0.0 && gi_mean > 0.0, "both lit: mis={mis_mean} gi={gi_mean}");
        // The whole point: NEE+MIS has markedly lower per-sample variance.
        assert!(
            mis_var < 0.5 * gi_var,
            "expected MIS variance well below pure-GI: mis_var={mis_var} gi_var={gi_var}"
        );
    }
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
mod roll_tests {
    use super::Camera;
    use crate::camera::CameraConfig;
    use crate::vec3::Vec3;

    fn cfg(roll: f32) -> CameraConfig {
        CameraConfig::builder()
            .look_from(Vec3::new(0.0, 0.0, 0.0))
            .look_at(Vec3::new(0.0, 0.0, -1.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .roll(roll)
            .build()
    }

    #[test]
    fn zero_roll_keeps_upright_basis() {
        // With no roll, the right axis u should be world +x (within sign tol).
        let cam = Camera::from(cfg(0.0));
        assert!((cam.basis_u().x.abs() - 1.0).abs() < 1e-5, "u={:?}", cam.basis_u());
        assert!(cam.basis_u().y.abs() < 1e-5);
    }

    #[test]
    fn ninety_roll_tilts_right_axis_to_vertical() {
        // Rolling 90° should swing the right axis onto the world vertical.
        let cam = Camera::from(cfg(90.0));
        assert!(cam.basis_u().y.abs() > 0.99, "u={:?}", cam.basis_u());
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
