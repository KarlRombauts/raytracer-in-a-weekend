use std::f32;
use std::sync::Arc;

use crate::interval::Interval;
use crate::material::Material;
use crate::ray::*;
use crate::vec3::{Point3, Vec3};
use rand::rngs::SmallRng;
use rand::Rng;

/// Orthonormal basis `(u, v, w)` with `w` pointing along `axis`. Used to orient
/// cone samples around the direction from the shading point to a sphere's centre.
fn onb(axis: Vec3) -> (Vec3, Vec3, Vec3) {
    let w = axis.unit();
    let a = if w.x.abs() > 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let v = w.cross(&a).unit();
    let u = w.cross(&v);
    (u, v, w)
}

pub struct Sphere {
    pub center: Ray,
    pub radius: f32,
    material: Arc<dyn Material>,
    bbox: AABB,
}

impl Sphere {
    pub fn stationary(center: Point3, radius: f32, material: Arc<dyn Material>) -> Self {
        let rvec = Vec3::new(radius, radius, radius);
        Sphere {
            center: Ray::new(center, Vec3::ZERO), //: Ray::new(center, Vec3::ZERO),
            radius,
            material,
            bbox: AABB::from_points(center - rvec, center + rvec),
        }
    }

    pub fn moving(
        center1: Point3,
        center2: Point3,
        radius: f32,
        material: Arc<dyn Material>,
    ) -> Self {
        let rvec = Vec3::new(radius, radius, radius);
        let box1 = AABB::from_points(center1 - rvec, center1 + rvec);
        let box2 = AABB::from_points(center2 - rvec, center2 + rvec);
        let bbox = AABB::from_boxes(&box1, &box2);

        Sphere {
            center: Ray::new(center1, center2 - center1), //: Ray::new(center, Vec3::ZERO),
            radius,
            material,
            bbox,
        }
    }

    pub fn get_spherical_uv(&self, p: &Point3) -> (f32, f32) {
        let theta = (-p.y).acos();
        let phi = f32::atan2(-p.z, p.x) + f32::consts::PI;

        let u = phi / (2. * f32::consts::PI);
        let v = theta / f32::consts::PI;
        (u, v)
    }
}

impl Intersect for Sphere {
    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn center(&self) -> Vec3 {
        self.bbox.center()
    }

    fn sample_point(&self, rng: &mut SmallRng) -> Point3 {
        let center = self.center.at(0.0);
        center + self.radius * Vec3::random_unit(rng)
    }

    fn area(&self) -> f32 {
        4.0 * std::f32::consts::PI * self.radius * self.radius
    }

    /// A direction from `origin` toward this sphere, sampled uniformly over the
    /// solid angle (cone) the sphere subtends — the importance-sampling match for
    /// a sphere light. Falls back to a uniform direction when `origin` is inside.
    fn random_dir(&self, origin: Point3, rng: &mut SmallRng) -> Vec3 {
        let center = self.center.at(0.0);
        let to_center = center - origin;
        let d2 = to_center.length_squared();
        let r2 = self.radius * self.radius;
        if d2 <= r2 {
            return Vec3::random_unit(rng);
        }
        let cos_theta_max = (1.0 - r2 / d2).sqrt();
        let r1: f32 = rng.random();
        let r2u: f32 = rng.random();
        let cos_theta = 1.0 - r1 * (1.0 - cos_theta_max);
        let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
        let phi = 2.0 * std::f32::consts::PI * r2u;
        let (u, v, w) = onb(to_center);
        (phi.cos() * sin_theta) * u + (phi.sin() * sin_theta) * v + cos_theta * w
    }

    /// Solid-angle PDF of cone sampling toward this sphere (see `random_dir`).
    /// Uniform within the cone the sphere subtends from `origin`, so it's a
    /// constant `1 / (2π(1 - cosθ_max))` for any direction that hits the sphere,
    /// and 0 for directions that miss.
    fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32 {
        let ray = Ray::new(origin, dir);
        if self
            .intersect(&ray, &Interval::new(0.001, f32::INFINITY))
            .is_none()
        {
            return 0.0;
        }
        let center = self.center.at(0.0);
        let d2 = (center - origin).length_squared();
        let r2 = self.radius * self.radius;
        if d2 <= r2 {
            // Origin inside the sphere: every direction hits, distributed
            // uniformly over the full sphere of directions.
            return 1.0 / (4.0 * std::f32::consts::PI);
        }
        let cos_theta_max = (1.0 - r2 / d2).sqrt();
        1.0 / (2.0 * std::f32::consts::PI * (1.0 - cos_theta_max))
    }

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let current_center = self.center.at(ray.time);
        let oc = ray.origin - current_center;
        let a = ray.direction.length_squared();
        let half_b = oc.dot(&ray.direction);
        let c = oc.length_squared() - self.radius * self.radius;
        let discriminant = half_b * half_b - a * c;

        if discriminant < 0.0 {
            return None;
        }

        let sqrt_d = discriminant.sqrt();
        let mut root = (-half_b - sqrt_d) / a;

        if !ray_t.surrounds(root) {
            root = (-half_b + sqrt_d) / a;
            if !ray_t.surrounds(root) {
                return None;
            }
        }

        let p = ray.at(root);
        let outward_normal = (p - current_center).unit();
        let mut hit_record = HitRecord::new(root, p, outward_normal, self.material.as_ref());
        (hit_record.u, hit_record.v) = self.get_spherical_uv(&outward_normal);
        hit_record.set_face_normal(ray, &outward_normal);
        Some(hit_record)
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn sampled_point_is_on_sphere_surface() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let center = Point3::new(1.0, 2.0, 3.0);
        let radius = 5.0;
        let s = Sphere::stationary(center, radius, mat);
        let mut rng = SmallRng::seed_from_u64(3);
        for _ in 0..500 {
            let p = s.sample_point(&mut rng);
            let r = (p - center).length();
            assert!((r - radius).abs() < 1e-3, "off-surface: r={}", r);
        }
    }
}

#[cfg(test)]
mod cone_light_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::f32::consts::PI;
    use std::sync::Arc;

    fn sphere(center: Point3, radius: f32) -> Sphere {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        Sphere::stationary(center, radius, mat)
    }

    #[test]
    fn area_is_surface_area_of_the_sphere() {
        let s = sphere(Point3::new(0.0, 0.0, 0.0), 2.0);
        let expected = 4.0 * PI * 2.0 * 2.0;
        assert!((s.area() - expected).abs() < 1e-3, "got {}", s.area());
    }

    #[test]
    fn pdf_matches_the_uniform_cone_formula() {
        // Sphere of radius 1 at distance 5; shading point at the origin.
        let center = Point3::new(0.0, 0.0, 5.0);
        let r = 1.0;
        let s = sphere(center, r);
        let origin = Point3::new(0.0, 0.0, 0.0);
        let dir = center - origin; // aimed at the centre, so it hits the sphere

        let d2 = (center - origin).length_squared();
        let cos_max = (1.0 - r * r / d2).sqrt();
        let expected = 1.0 / (2.0 * PI * (1.0 - cos_max));

        let pdf = s.pdf_value(origin, dir);
        assert!((pdf - expected).abs() < 1e-3, "pdf={} expected={}", pdf, expected);
    }

    #[test]
    fn pdf_is_zero_when_pointing_away_from_the_sphere() {
        let s = sphere(Point3::new(0.0, 0.0, 5.0), 1.0);
        let origin = Point3::new(0.0, 0.0, 0.0);
        let away = Vec3::new(0.0, 0.0, -1.0); // points away from the sphere
        assert_eq!(s.pdf_value(origin, away), 0.0);
    }

    #[test]
    fn cone_samples_stay_in_the_cone_and_fill_it_uniformly() {
        let center = Point3::new(0.0, 0.0, 2.0);
        let r = 1.0;
        let s = sphere(center, r);
        let origin = Point3::new(0.0, 0.0, 0.0);
        let axis = (center - origin).unit();
        let d2 = (center - origin).length_squared();
        let cos_max = (1.0 - r * r / d2).sqrt();
        let mid = 0.5 * (1.0 + cos_max);

        let mut rng = SmallRng::seed_from_u64(7);
        let n = 50_000;
        let mut sum_cos = 0.0f32;
        let mut upper_half = 0usize;
        for _ in 0..n {
            let dir = s.random_dir(origin, &mut rng).unit();
            let cos = dir.dot(&axis);
            // Every sampled direction lies within the subtended cone.
            assert!(cos >= cos_max - 1e-3, "outside cone: cos={} cos_max={}", cos, cos_max);
            sum_cos += cos;
            if cos > mid {
                upper_half += 1;
            }
        }
        // Uniform in solid angle ⇒ cosθ is uniform on [cos_max, 1]:
        // mean is the midpoint and half the samples fall in the upper half.
        let mean = sum_cos / n as f32;
        assert!((mean - mid).abs() < 4e-3, "mean cos={} expected={}", mean, mid);
        let frac_upper = upper_half as f32 / n as f32;
        assert!((frac_upper - 0.5).abs() < 2e-2, "cosθ not uniform: upper frac={}", frac_upper);
    }
}
