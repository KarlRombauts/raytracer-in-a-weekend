use crate::{interval::Interval, ray::Ray, vec3::Point3};

pub struct AABB {
    x: Interval,
    y: Interval,
    z: Interval,
}

impl AABB {
    pub fn new(x: Interval, y: Interval, z: Interval) -> Self {
        let mut aabb = AABB { x, y, z };
        aabb.pad_to_minimums();
        aabb
    }

    fn pad_to_minimums(&mut self) {
        let delta = 0.0001;
        if self.x.size() < delta {
            self.x = self.x.expand(delta);
        }
        if self.y.size() < delta {
            self.y = self.y.expand(delta);
        }
        if self.z.size() < delta {
            self.z = self.z.expand(delta);
        }
    }

    pub fn from_points(a: Point3, b: Point3) -> Self {
        AABB {
            x: Interval::new(a.x.min(b.x), a.x.max(b.x)),
            z: Interval::new(a.z.min(b.z), a.z.max(b.z)),
            y: Interval::new(a.y.min(b.y), a.y.max(b.y)),
        }
    }

    pub fn from_boxes(a: &AABB, b: &AABB) -> Self {
        let x = Interval::enclosing(a.x, b.x);
        let y = Interval::enclosing(a.y, b.y);
        let z = Interval::enclosing(a.z, b.z);

        AABB { x, y, z }
    }

    pub fn axis_interval(&self, n: u32) -> &Interval {
        match n {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Axis interval index out of range: {}", n),
        }
    }

    pub fn intersect(&self, ray: &Ray, ray_t: &Interval) -> bool {
        let mut tmin = ray_t.min;
        let mut tmax = ray_t.max;

        for axis in 0..3 {
            let ax = self.axis_interval(axis);
            let inv_d = 1.0 / ray.direction[axis];
            let t0 = (ax.min - ray.origin[axis]) * inv_d;
            let t1 = (ax.max - ray.origin[axis]) * inv_d;
            let (t_low, t_high) = (t0.min(t1), t0.max(t1));

            tmin = tmin.max(t_low);
            tmax = tmax.min(t_high);
            if tmax <= tmin {
                return false;
            }
        }
        true
    }

    pub const EMPTY: AABB = AABB {
        x: Interval::EMPTY,
        y: Interval::EMPTY,
        z: Interval::EMPTY,
    };
}

impl Default for AABB {
    fn default() -> Self {
        AABB {
            x: Interval::EMPTY,
            y: Interval::EMPTY,
            z: Interval::EMPTY,
        }
    }
}
