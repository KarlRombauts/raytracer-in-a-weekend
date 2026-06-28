use std::ops::Index;

use rand::prelude::*;
use rand_distr::{UnitDisc, UnitSphere};

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub type Point3 = Vec3;

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub fn zeros() -> Self {
        Vec3::new(0., 0., 0.)
    }

    pub fn ones() -> Self {
        Vec3::new(1., 1., 1.)
    }

    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    pub fn inverse(&self) -> Self {
        Vec3 {
            x: 1. / self.x,
            y: 1. / self.y,
            z: 1. / self.z,
        }
    }

    pub fn random() -> Self {
        let mut rng = rand::rng();
        Vec3::new(
            rng.random::<f32>(),
            rng.random::<f32>(),
            rng.random::<f32>(),
        )
    }

    pub fn min(a: &Self, b: &Self) -> Self {
        Vec3 {
            x: f32::min(a.x, b.x),
            y: f32::min(a.y, b.y),
            z: f32::min(a.z, b.z),
        }
    }

    pub fn max(a: &Self, b: &Self) -> Self {
        Vec3 {
            x: f32::max(a.x, b.x),
            y: f32::max(a.y, b.y),
            z: f32::max(a.z, b.z),
        }
    }

    pub fn random_range(min: f32, max: f32) -> Self {
        let mut rng = rand::rng();
        Vec3::new(
            rng.random_range(min..max),
            rng.random_range(min..max),
            rng.random_range(min..max),
        )
    }

    pub fn random_unit(rng: &mut impl Rng) -> Self {
        let [x, y, z]: [f32; 3] = UnitSphere.sample(rng);
        return Vec3 { x, y, z };
    }

    pub fn random_in_unit_disk(rng: &mut impl Rng) -> Self {
        let [x, y]: [f32; 2] = UnitDisc.sample(rng);
        Vec3::new(x, y, 0.0)
    }

    pub fn random_on_hemisphere(normal: &Vec3, rng: &mut impl Rng) -> Self {
        let on_unit_sphere = Vec3::random_unit(rng);
        if on_unit_sphere.dot(normal) > 0.0 {
            return on_unit_sphere;
        } else {
            return -on_unit_sphere;
        }
    }

    pub fn reflect(v: &Vec3, n: &Vec3) -> Vec3 {
        return *v - 2.0 * v.dot(n) * (*n);
    }

    pub fn refract(unit_v: &Vec3, n: &Vec3, eta_i_over_eta_t: f32) -> Self {
        let cos_theta = (-*unit_v).dot(n).min(1.0);
        let r_out_perp = eta_i_over_eta_t * (*unit_v + cos_theta * (*n));
        let r_out_parallel = -(1.0 - r_out_perp.length_squared()).abs().sqrt() * (*n);
        return r_out_perp + r_out_parallel;
    }

    pub fn length(&self) -> f32 {
        (self.length_squared()).sqrt()
    }

    pub fn length_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn vec_mul(&self, other: &Self) -> Self {
        Vec3::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }

    pub fn dot(&self, other: &Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(&self, other: &Self) -> Self {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn unit(&self) -> Self {
        let length = self.length();
        if length > 0.0 {
            *self / length
        } else {
            Vec3::new(0.0, 0.0, 0.0)
        }
    }

    /// Rotate `self` about `axis` (assumed unit length) by `angle_rad`,
    /// right-handed (Rodrigues' rotation formula).
    pub fn rotate_about_axis(&self, axis: &Vec3, angle_rad: f32) -> Vec3 {
        let (sin, cos) = angle_rad.sin_cos();
        *self * cos + axis.cross(self) * sin + *axis * (axis.dot(self) * (1.0 - cos))
    }

    pub fn near_zero(&self) -> bool {
        let s: f32 = 1e-8;
        return (self.x.abs() < s) && (self.y.abs() < s) && (self.z.abs() < s);
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self {
        Vec3::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }
}

impl std::ops::Mul<Vec3> for f32 {
    type Output = Vec3;

    fn mul(self, vec: Vec3) -> Vec3 {
        vec.mul(self)
    }
}

impl std::ops::Mul<Vec3> for Vec3 {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        self.vec_mul(&other)
    }
}

impl std::ops::Div<f32> for Vec3 {
    type Output = Self;

    fn div(self, scalar: f32) -> Self {
        if scalar != 0.0 {
            Vec3::new(self.x / scalar, self.y / scalar, self.z / scalar)
        } else {
            panic!("Division by zero in Vec3 division");
        }
    }
}

impl std::ops::Neg for Vec3 {
    type Output = Self;

    fn neg(self) -> Self {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

impl std::ops::AddAssign for Vec3 {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
    }
}

impl std::ops::SubAssign for Vec3 {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
        self.z -= other.z;
    }
}

impl std::ops::MulAssign<f32> for Vec3 {
    fn mul_assign(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
    }
}

impl std::ops::DivAssign<f32> for Vec3 {
    fn div_assign(&mut self, scalar: f32) {
        self.x /= scalar;
        self.y /= scalar;
        self.z /= scalar;
    }
}

impl Index<u32> for Vec3 {
    type Output = f32;

    fn index(&self, index: u32) -> &f32 {
        match index {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Vec3 index out of bounds: {}", index),
        }
    }
}

#[cfg(test)]
mod orbit_rotate_tests {
    use super::Vec3;
    use std::f32::consts::PI;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn rotates_x_to_y_about_z() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        let r = x.rotate_about_axis(&z, PI / 2.0);
        assert!(close(r, Vec3::new(0.0, 1.0, 0.0)), "got {:?}", r);
    }

    #[test]
    fn rotation_about_own_axis_is_noop() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let r = z.rotate_about_axis(&z, 1.234);
        assert!(close(r, z), "got {:?}", r);
    }
}
