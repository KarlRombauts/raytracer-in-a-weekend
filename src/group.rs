use crate::color::Color;
use crate::interval::Interval;
use crate::ray::*;
use crate::vec3::{Point3, Vec3};
use rand::rngs::SmallRng;
use rand::Rng;
use std::sync::Arc;

/// An emissive object the integrator can sample directly (next event
/// estimation). Pairs a sampleable geometry handle with a constant emission.
pub struct Light {
    pub geom: Arc<dyn Intersect>,
    pub emit: Color,
}

pub struct IntersectGroup {
    pub objects: Vec<Arc<dyn Intersect>>,
    pub lights: Vec<Light>,
    bbox: AABB,
}

impl IntersectGroup {
    pub fn new() -> Self {
        IntersectGroup {
            objects: Vec::new(),
            lights: Vec::new(),
            bbox: AABB::EMPTY,
        }
    }

    pub fn add(&mut self, object: Arc<dyn Intersect>) -> &mut Self {
        self.bbox = AABB::from_boxes(&self.bbox, object.bounding_box());
        self.objects.push(object);
        self
    }

    pub fn from_object(object: Arc<dyn Intersect>) -> Self {
        let mut group = IntersectGroup::new();
        group.add(object);
        group
    }

    pub fn clear(&mut self) {
        self.objects.clear();
    }

    /// Average solid-angle PDF of sampling `dir` (from `origin`) toward any
    /// registered light. 0 when there are no lights.
    pub fn light_pdf(&self, origin: Point3, dir: Vec3) -> f32 {
        if self.lights.is_empty() {
            return 0.0;
        }
        let sum: f32 = self
            .lights
            .iter()
            .map(|l| l.geom.pdf_value(origin, dir))
            .sum();
        sum / self.lights.len() as f32
    }

    /// A random (unnormalized) direction from `origin` toward a uniformly chosen
    /// registered light. `None` when there are no lights.
    pub fn sample_light_dir(&self, origin: Point3, rng: &mut SmallRng) -> Option<Vec3> {
        if self.lights.is_empty() {
            return None;
        }
        let i = rng.random_range(0..self.lights.len());
        Some(self.lights[i].geom.random_dir(origin, rng))
    }
}

impl Intersect for IntersectGroup {
    fn center(&self) -> Vec3 {
        self.bbox.center()
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let mut closest_hit: Option<HitRecord> = None;
        let mut closest_t = ray_t.max;

        for object in &self.objects {
            if let Some(hit_record) = object.intersect(ray, &Interval::new(ray_t.min, closest_t)) {
                closest_t = hit_record.t as f32;
                closest_hit = Some(hit_record);
            }
        }

        closest_hit
    }

    fn sample_point(&self, rng: &mut SmallRng) -> Point3 {
        if self.objects.is_empty() {
            return self.center();
        }
        let i = rng.random_range(0..self.objects.len());
        self.objects[i].sample_point(rng)
    }
}

#[cfg(test)]
mod light_mixture_tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    // Overhead quad light on the y=2 plane, area 4 (same as the analytic
    // pdf_value setup: pdf_value((0,0,0),(0,2,0)) == 1.0).
    fn overhead_light() -> Arc<dyn Intersect> {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        ))
    }

    #[test]
    fn light_pdf_is_zero_without_lights() {
        let w = IntersectGroup::new();
        assert_eq!(w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn light_pdf_averages_single_light() {
        let mut w = IntersectGroup::new();
        w.lights.push(Light { geom: overhead_light(), emit: Color::new(1.0, 1.0, 1.0) });
        // One light => average == that light's pdf_value; analytic value is 1.0.
        let p = w.light_pdf(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        assert!((p - 1.0).abs() < 1e-5, "p={p}");
    }

    #[test]
    fn sample_light_dir_is_none_without_lights() {
        let w = IntersectGroup::new();
        let mut rng = SmallRng::seed_from_u64(1);
        assert!(w.sample_light_dir(Point3::new(0.0, 0.0, 0.0), &mut rng).is_none());
    }

    #[test]
    fn sample_light_dir_points_toward_the_light() {
        let mut w = IntersectGroup::new();
        w.lights.push(Light { geom: overhead_light(), emit: Color::new(1.0, 1.0, 1.0) });
        let mut rng = SmallRng::seed_from_u64(2);
        for _ in 0..100 {
            let d = w.sample_light_dir(Point3::new(0.0, 0.0, 0.0), &mut rng).unwrap();
            assert!(d.y > 0.0, "expected upward dir toward overhead light, got {:?}", d);
        }
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::geometry::Quad;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn group_samples_one_of_its_children() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let q_a = Arc::new(Quad::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat.clone(),
        ));
        let q_b = Arc::new(Quad::new(
            Point3::new(10.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat,
        ));
        let mut g = IntersectGroup::new();
        g.add(q_a);
        g.add(q_b);

        let mut rng = SmallRng::seed_from_u64(4);
        let (mut hit_a, mut hit_b) = (false, false);
        for _ in 0..500 {
            let p = g.sample_point(&mut rng);
            let in_a = (0.0..=1.0).contains(&p.x);
            let in_b = (10.0..=11.0).contains(&p.x);
            assert!(in_a || in_b, "point on neither child: {:?}", p);
            hit_a |= in_a;
            hit_b |= in_b;
        }
        assert!(hit_a && hit_b, "expected to sample both children");
    }
}
