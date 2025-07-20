use rand::Rng;

use crate::vec3::{Point3, Vec3};

const POINT_COUNT: usize = 256;
pub struct Perlin {
    rand_vec: [Vec3; POINT_COUNT],
    perm_x: [usize; POINT_COUNT],
    perm_y: [usize; POINT_COUNT],
    perm_z: [usize; POINT_COUNT],
}

impl Perlin {
    pub fn new() -> Self {
        let mut rng = rand::rng();

        let mut rand_vec = [Vec3::ZERO; POINT_COUNT];
        for f in &mut rand_vec {
            *f = Vec3::random_range(-1., 1.).unit();
        }

        let perm_x = Self::generate_perm(&mut rng);
        let perm_y = Self::generate_perm(&mut rng);
        let perm_z = Self::generate_perm(&mut rng);

        Perlin {
            rand_vec,
            perm_x,
            perm_y,
            perm_z,
        }
    }

    fn generate_perm(rng: &mut impl Rng) -> [usize; POINT_COUNT] {
        let mut p = std::array::from_fn(|i| i);
        // Fisher-Yates Shuffle
        for i in (1..POINT_COUNT).rev() {
            let target = rng.random_range(0..=i);
            p.swap(i, target);
        }
        return p;
    }

    pub fn noise(&self, p: &Point3) -> f32 {
        // Raw coordinate components
        let x = p.x;
        let y = p.y;
        let z = p.z;

        // Integer cell indices (may be negative)
        let ix = x.floor() as isize;
        let iy = y.floor() as isize;
        let iz = z.floor() as isize;

        // Local offsets in [0,1)
        let mut u = x - ix as f32;
        let mut v = y - iy as f32;
        let mut w = z - iz as f32;

        // Gather the eight corner values
        let mut c = [[[Vec3::ZERO; 2]; 2]; 2];
        for di in 0..2 {
            let xi = ((ix + di as isize).rem_euclid(256)) as usize;
            for dj in 0..2 {
                let yj = ((iy + dj as isize).rem_euclid(256)) as usize;
                for dk in 0..2 {
                    let zk = ((iz + dk as isize).rem_euclid(256)) as usize;
                    let idx = self.perm_x[xi] ^ self.perm_y[yj] ^ self.perm_z[zk];
                    c[di][dj][dk] = self.rand_vec[idx];
                }
            }
        }

        // Trilinear interpolation
        Self::perlin_interp(&c, u, v, w)
    }

    fn perlin_interp(c: &[[[Vec3; 2]; 2]; 2], u: f32, v: f32, w: f32) -> f32 {
        let uu = u * u * (3. - 2. * u);
        let vv = v * v * (3. - 2. * v);
        let ww = w * w * (3. - 2. * w);
        let mut accum = 0.0;

        for i in 0..2 {
            for j in 0..2 {
                for k in 0..2 {
                    let weight_v = Vec3::new(u - i as f32, v - j as f32, w - k as f32);
                    accum += (i as f32 * uu + (1 - i) as f32 * (1. - uu))
                        * (j as f32 * vv + (1 - j) as f32 * (1. - vv))
                        * (k as f32 * ww + (1 - k) as f32 * (1. - ww))
                        * c[i][j][k].dot(&weight_v);
                }
            }
        }
        return accum;
    }

    pub fn turb(&self, p: &Point3, depth: u32) -> f32 {
        let mut accum = 0.;
        let mut temp_p = *p;
        let mut weight = 1.;

        for _ in 0..depth {
            accum += weight * self.noise(&temp_p);
            weight *= 0.5;
            temp_p *= 2.;
        }

        accum.abs()
    }
}
