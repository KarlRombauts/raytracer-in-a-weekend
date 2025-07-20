use crate::vec3::Point3;
use crate::vec3::Vec3;

#[derive(Debug, Clone)]
pub struct Ray {
    pub origin: Point3,
    pub direction: Vec3,
    pub time: f32,
}

impl Ray {
    pub fn new(origin: Point3, direction: Vec3) -> Self {
        Ray {
            origin,
            direction,
            time: 0.0,
        }
    }

    pub fn new_t(origin: Point3, direction: Vec3, time: f32) -> Self {
        Ray {
            origin,
            direction,
            time,
        }
    }

    pub fn at(&self, t: f32) -> Point3 {
        self.origin + self.direction * t
    }
}
