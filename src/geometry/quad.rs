use std::sync::Arc;

use crate::{
    interval::Interval,
    material::{material, Material},
    ray::{HitRecord, Intersect, Ray, AABB},
    vec3::{Point3, Vec3},
};

pub struct Quad {
    q: Point3,
    u: Vec3,
    v: Vec3,
    d: f32,
    w: Vec3,
    centroid: Vec3,
    normal: Vec3,
    material: Arc<dyn Material>,
    bbox: AABB,
}

impl Quad {
    pub fn new(q: Point3, u: Vec3, v: Vec3, material: Arc<dyn Material>) -> Self {
        let n = u.cross(&v);
        let normal = n.unit();
        let d = normal.dot(&q);

        let mut quad = Quad {
            q,
            u,
            v,
            d,
            w: n / n.dot(&n),
            centroid: q + (u + v) * 0.5,
            normal,
            material,
            bbox: AABB::EMPTY,
        };
        quad.set_bounding_box();
        quad
    }

    fn set_bounding_box(&mut self) {
        let bbox_diagonal1 = AABB::from_points(self.q, self.q + self.u + self.v);
        let bbox_diagonal2 = AABB::from_points(self.q + self.u, self.q + self.v);
        self.bbox = AABB::from_boxes(&bbox_diagonal1, &bbox_diagonal2);
    }

    fn is_interior(a: f32, b: f32) -> bool {
        let unit = Interval::new(0., 1.);
        unit.contains(a) && unit.contains(b)
    }
}

impl Intersect for Quad {
    fn center(&self) -> Vec3 {
        return self.centroid;
    }

    fn intersect(
        &self,
        ray: &Ray,
        ray_t: &crate::interval::Interval,
    ) -> Option<crate::ray::HitRecord<'_>> {
        let denom = self.normal.dot(&ray.direction);

        // No hit if the ray is parallel to the plane
        if denom.abs() < 1e-8 {
            return None;
        }

        let t = (self.d - self.normal.dot(&ray.origin)) / denom;
        // Return false if the hit point parameter t is outside the ray interval
        if !ray_t.contains(t) {
            return None;
        }

        // Check that the hit point lies inside the quad region
        let p = ray.at(t);
        let planar_hit_point = p - self.q;
        let alpha = self.w.dot(&planar_hit_point.cross(&self.v));
        let beta = self.w.dot(&self.u.cross(&planar_hit_point));

        if !Self::is_interior(alpha, beta) {
            return None;
        }

        let mut hit_record = HitRecord::new(t, p, Vec3::ZERO, self.material.as_ref());
        hit_record.set_face_normal(ray, &self.normal);
        hit_record.u = alpha;
        hit_record.v = beta;
        Some(hit_record)
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn sample_point(&self, u: f32, v: f32) -> Point3 {
        self.q + u * self.u + v * self.v
    }

    fn area(&self) -> f32 {
        self.u.cross(&self.v).length()
    }
}

#[cfg(test)]
mod area_pdf_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use std::sync::Arc;

    // Overhead light on the y = 2 plane, spanning x,z in [-1,1]; area = 4.
    fn overhead_quad() -> Quad {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        )
    }

    #[test]
    fn area_is_cross_product_magnitude() {
        assert!((overhead_quad().area() - 4.0).abs() < 1e-5);
    }

    #[test]
    fn pdf_value_matches_analytic() {
        // Origin directly below the quad centre; dir points at the centre (0,2,0).
        // dist^2 = 4, cos = 1, area = 4  =>  pdf = dist^2 / (cos * area) = 1.0
        let p = overhead_quad().pdf_value(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        assert!((p - 1.0).abs() < 1e-5, "pdf={}", p);
    }

    #[test]
    fn pdf_value_zero_on_miss() {
        // Pointing away from the overhead quad never hits it.
        let p = overhead_quad().pdf_value(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, -2.0, 0.0));
        assert_eq!(p, 0.0);
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};
    use std::sync::Arc;

    #[test]
    fn sampled_point_lies_on_quad() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let q = Quad::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            mat,
        );
        let mut rng = SmallRng::seed_from_u64(1);
        let mut xs: Vec<f32> = Vec::with_capacity(500);
        let mut ys: Vec<f32> = Vec::with_capacity(500);
        for _ in 0..500 {
            let p = q.sample_point(rng.random::<f32>(), rng.random::<f32>());
            assert!(p.z.abs() < 1e-5, "off-plane: {:?}", p);
            assert!((0.0..=2.0).contains(&p.x), "x out: {}", p.x);
            assert!((0.0..=3.0).contains(&p.y), "y out: {}", p.y);
            xs.push(p.x);
            ys.push(p.y);
        }
        // Spread check: the centroid is (1.0, 1.5, 0.0). If sample_point were replaced
        // by the default center() all 500 x-values would equal 1.0 and all y-values
        // would equal 1.5.  A real uniform sample must cover the full [0,2]×[0,3] range.
        let x_range = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let y_range = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - ys.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(x_range > 1.0, "x spread too small ({}); samples may not vary", x_range);
        assert!(y_range > 1.5, "y spread too small ({}); samples may not vary", y_range);
    }
}
