use crate::{
    interval::Interval,
    ray::Ray,
    vec3::{Point3, Vec3},
};

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

    pub fn extent(&self) -> Vec3 {
        self.max_vec() - self.min_vec()
    }

    pub fn area(&self) -> f32 {
        let e = self.extent();
        return e.x * e.y + e.y * e.z + e.z * e.x;
    }

    pub fn min_vec(&self) -> Vec3 {
        Vec3::new(self.x.min, self.y.min, self.z.min)
    }

    pub fn max_vec(&self) -> Vec3 {
        Vec3::new(self.x.max, self.y.max, self.z.max)
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
        let mut aabb = AABB {
            x: Interval::new(a.x.min(b.x), a.x.max(b.x)),
            z: Interval::new(a.z.min(b.z), a.z.max(b.z)),
            y: Interval::new(a.y.min(b.y), a.y.max(b.y)),
        };
        // Pad so an axis-aligned, perfectly flat primitive (e.g. a cube-face
        // triangle, or an axis-aligned quad) gets a non-zero thickness on every
        // axis. Without this, the slab test in `intersect` collapses to
        // `tmin == tmax` on the flat axis and rejects every ray through the box,
        // so a BVH whose interior nodes are all coplanar drops those primitives —
        // the missing-face-centre holes on imported cubes/cylinders.
        aabb.pad_to_minimums();
        aabb
    }

    pub fn from_boxes(a: &AABB, b: &AABB) -> Self {
        let x = Interval::enclosing(a.x, b.x);
        let y = Interval::enclosing(a.y, b.y);
        let z = Interval::enclosing(a.z, b.z);

        // Pad here too: a BVH node enclosing only coplanar children is itself
        // flat, and is the node that actually gets culled during traversal.
        let mut aabb = AABB { x, y, z };
        aabb.pad_to_minimums();
        aabb
    }

    pub fn center(&self) -> Vec3 {
        Vec3::new(self.x.center(), self.y.center(), self.z.center())
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
        // Hand-unrolled over x/y/z with direct field access. Avoids the
        // match-based `axis_interval`/`Index<u32>` lookups (each carrying a
        // panic branch) in the single hottest loop of BVH traversal.
        let mut tmin = ray_t.min;
        let mut tmax = ray_t.max;

        let inv_d = ray.inv_direction.x;
        let t0 = (self.x.min - ray.origin.x) * inv_d;
        let t1 = (self.x.max - ray.origin.x) * inv_d;
        tmin = tmin.max(t0.min(t1));
        tmax = tmax.min(t0.max(t1));
        if tmax <= tmin {
            return false;
        }

        let inv_d = ray.inv_direction.y;
        let t0 = (self.y.min - ray.origin.y) * inv_d;
        let t1 = (self.y.max - ray.origin.y) * inv_d;
        tmin = tmin.max(t0.min(t1));
        tmax = tmax.min(t0.max(t1));
        if tmax <= tmin {
            return false;
        }

        let inv_d = ray.inv_direction.z;
        let t0 = (self.z.min - ray.origin.z) * inv_d;
        let t1 = (self.z.max - ray.origin.z) * inv_d;
        tmin = tmin.max(t0.min(t1));
        tmax = tmax.min(t0.max(t1));
        if tmax <= tmin {
            return false;
        }

        true
    }

    pub fn intersect_dist(&self, ray: &Ray, ray_t: &Interval) -> f32 {
        let tx1 = (self.min_vec().x - ray.origin.x) / ray.direction.x;
        let tx2 = (self.max_vec().x - ray.origin.x) / ray.direction.x;
        let mut tmin = tx1.min(tx2);
        let mut tmax = tx1.max(tx2);

        let ty1 = (self.min_vec().y - ray.origin.y) / ray.direction.y;
        let ty2 = (self.max_vec().y - ray.origin.y) / ray.direction.y;
        tmin = tmin.max(ty1.min(ty2));
        tmax = tmax.min(ty1.max(ty2));

        let tz1 = (self.min_vec().z - ray.origin.z) / ray.direction.z;
        let tz2 = (self.max_vec().z - ray.origin.z) / ray.direction.z;
        tmin = tmin.max(tz1.min(tz2));
        tmax = tmax.min(tz1.max(tz2));

        if tmax >= tmin && tmin < ray_t.max && tmax > ray_t.min {
            tmin
        } else {
            1e30
        }
    }

    pub fn intersect_distance(&self, ray: &Ray) -> f32 {
        let box_min = Point3::new(self.x.min, self.y.min, self.z.min);
        let box_max = Point3::new(self.x.max, self.y.max, self.z.max);

        let t_min = (box_min - ray.origin) * ray.inv_direction;
        let t_max = (box_max - ray.origin) * ray.inv_direction;

        let t1 = Point3::new(
            t_min.x.min(t_max.x),
            t_min.y.min(t_max.y),
            t_min.z.min(t_max.z),
        );
        let t2 = Point3::new(
            t_min.x.max(t_max.x),
            t_min.y.max(t_max.y),
            t_min.z.max(t_max.z),
        );

        let dst_near = t1.x.max(t1.y).max(t1.z);
        let dst_far = t2.x.min(t2.y).min(t2.z);

        if dst_far >= dst_near && dst_far > 0.0 {
            dst_near.max(0.0) // Return entry distance (clamp to 0)
        } else {
            f32::INFINITY // No intersection
        }
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
