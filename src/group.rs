use crate::interval::Interval;
use crate::ray::*;
use crate::vec3::{Point3, Vec3};
use rand::rngs::SmallRng;
use rand::Rng;
use std::sync::Arc;

pub struct IntersectGroup {
    pub objects: Vec<Arc<dyn Intersect>>,
    bbox: AABB,
}

impl IntersectGroup {
    pub fn new() -> Self {
        IntersectGroup {
            objects: Vec::new(),
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
mod sample_tests {
    use super::*;
    use crate::color::Color;
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
