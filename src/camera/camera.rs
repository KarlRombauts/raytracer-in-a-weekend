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

    /// Mixture-PDF path tracer. One ray per bounce. At a diffuse (non-specular)
    /// hit the outgoing ray is drawn from a 50/50 mixture of a cosine-weighted
    /// direction and a direction toward a registered light, weighted by
    /// `albedo * scattering_pdf / p_mixture`. Specular materials use their own
    /// scattered ray. Emission is accumulated at every hit (one ray per bounce, so
    /// no double counting).
    fn ray_color(&self, ray: &Ray, world: &IntersectGroup, rng: &mut SmallRng) -> Color {
        let interval = Interval::new(0.001, f32::INFINITY);
        let mut color = Color::ZERO;
        let mut throughput = Color::ones();
        let mut current = ray.clone();

        for _ in 0..self.max_depth {
            let Some(hit) = world.intersect(&current, &interval) else {
                color += throughput * self.background;
                break;
            };

            color += throughput * hit.material.emitted(hit.u, hit.v, hit.p);

            let Some((scattered, atten)) = hit.material.scatter(&current, &hit, rng) else {
                break; // pure light / absorber
            };

            if hit.material.is_specular() {
                throughput = throughput * atten;
                current = scattered;
                continue;
            }

            // Lambertian: sample one outgoing ray from the cosine/light mixture.
            let albedo = atten;
            let dir = if !world.lights.is_empty() && rng.random::<f32>() < 0.5 {
                match world.sample_light_dir(hit.p, rng) {
                    Some(d) => d,
                    None => cosine_direction(&hit.normal, rng),
                }
            } else {
                cosine_direction(&hit.normal, rng)
            };

            let s = hit.material.scattering_pdf(&hit, &dir);
            if s <= 0.0 {
                break; // direction below the surface contributes nothing
            }
            let p = if world.lights.is_empty() {
                s
            } else {
                0.5 * s + 0.5 * world.light_pdf(hit.p, dir)
            };
            if p <= 0.0 {
                break;
            }
            throughput = throughput * albedo * (s / p);
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
        assert!(rel < 0.15, "means should agree (unbiased): nee={with_nee} gi={pure_gi} rel={rel}");
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
