use core::f32;
use rand::prelude::*;
use rayon::prelude::*;
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};

use image;

use crate::camera::config::CameraConfig;
use crate::color::{clamp_luminance, Color};
use crate::group::*;
use crate::interval::Interval;
use crate::ray::{Intersect, Ray};
use crate::sampling::stratified_offset;
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
    let a2 = a * a;
    let b2 = b * b;
    let denom = a2 + b2;
    if denom > 0.0 {
        a2 / denom
    } else {
        0.0
    }
}

/// Cosine-weighted hemisphere direction about `normal` (PDF = cos/PI), using the
/// `normal + random_unit` trick. Returns a unit vector.
fn cosine_direction(normal: &Vec3, rng: &mut SmallRng) -> Vec3 {
    let mut d = *normal + Vec3::random_unit(rng);
    if d.near_zero() {
        d = *normal;
    }
    d.unit()
}

impl Camera {
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
                    pixel_color +=
                        clamp_luminance(self.ray_color(&ray, world, &mut rng), self.firefly_clamp);
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
        clamp_luminance(self.ray_color(&ray, world, rng), self.firefly_clamp)
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

    fn ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color {
        let interval = Interval::new(0.001, f32::INFINITY);
        let mut color = Color::ZERO;
        let mut throughput = Color::ones();
        let mut current = ray.clone();
        // Whether emission at the next hit gets full weight: true for the camera ray
        // and after specular bounces (NEE can't sample those), false after a diffuse
        // bounce (NEE already accounted for direct light, so MIS-weight the emission).
        let mut specular_bounce = true;
        let mut prev_brdf_pdf = 0.0_f32;

        for _ in 0..self.max_depth {
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

            let Some((scattered, atten)) = hit.material.scatter(&current, &hit, rng) else {
                break; // pure light / absorber
            };

            if hit.material.is_specular() {
                throughput = throughput * atten;
                current = scattered;
                specular_bounce = true;
                continue;
            }

            // Lambertian.
            let albedo = atten;

            // (1) Next-event estimation: sample a light, weight against the BRDF pdf.
            if let Some(ld) = world.sample_light_dir(hit.p, rng) {
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
            let dir = cosine_direction(&hit.normal, rng);
            let p_brdf = hit.material.scattering_pdf(&hit, &dir);
            if p_brdf <= 0.0 {
                break;
            }
            throughput = throughput * albedo;
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
        let col = c.ray_color(&ray, &world, &mut rng);
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
            sum += c.ray_color(&ray, &world, &mut rng).x;
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
            let x = c.ray_color(&ray, &world, &mut rng).x;
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
