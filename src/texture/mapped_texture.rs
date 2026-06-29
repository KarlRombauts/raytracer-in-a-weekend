use std::sync::Arc;

use crate::color::Color;
use crate::texture::Texture;
use crate::vec3::Point3;

/// How a texture's (u, v) lookup coordinates are derived for a hit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Projection {
    /// Use the geometry's own (u, v).
    MeshUv,
    /// Project the hit point onto the world XZ plane: (p.x, p.z).
    Planar,
    /// Spherical coordinates of the direction of p (about the origin).
    Spherical,
    /// Cylindrical: angle about the world Y axis, and height p.y.
    Cylindrical,
}

/// Wraps a texture, remapping the (u, v) it samples with by `projection`, then a
/// `scale` and `offset`, wrapping the result into [0, 1) so `scale > 1` tiles.
pub struct MappedTexture {
    inner: Arc<dyn Texture>,
    projection: Projection,
    scale: f32,
    offset: (f32, f32),
}

impl MappedTexture {
    pub fn new(
        inner: Arc<dyn Texture>,
        projection: Projection,
        scale: f32,
        offset: (f32, f32),
    ) -> Self {
        MappedTexture {
            inner,
            projection,
            scale,
            offset,
        }
    }
}

impl Texture for MappedTexture {
    fn value(&self, u: f32, v: f32, p: &Point3) -> Color {
        use std::f32::consts::PI;
        let (bu, bv) = match self.projection {
            Projection::MeshUv => (u, v),
            Projection::Planar => (p.x, p.z),
            Projection::Spherical => {
                let d = p.unit();
                (
                    f32::atan2(d.z, d.x) / (2.0 * PI) + 0.5,
                    (d.y.clamp(-1.0, 1.0)).acos() / PI,
                )
            }
            Projection::Cylindrical => (f32::atan2(p.z, p.x) / (2.0 * PI) + 0.5, p.y),
        };
        let uu = (bu * self.scale + self.offset.0).rem_euclid(1.0);
        let vv = (bv * self.scale + self.offset.1).rem_euclid(1.0);
        self.inner.value(uu, vv, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::Point3;

    /// Test texture that echoes its (u, v) back as a color, so we can assert
    /// exactly which coordinates `MappedTexture` produced.
    struct UvProbe;
    impl Texture for UvProbe {
        fn value(&self, u: f32, v: f32, _p: &Point3) -> Color {
            Color::new(u, v, 0.0)
        }
    }

    fn probe(projection: Projection, scale: f32, offset: (f32, f32)) -> MappedTexture {
        MappedTexture::new(Arc::new(UvProbe), projection, scale, offset)
    }

    #[test]
    fn mesh_uv_identity_passes_uv_through() {
        let c = probe(Projection::MeshUv, 1.0, (0.0, 0.0)).value(0.3, 0.7, &Point3::new(9.0, 9.0, 9.0));
        assert!((c.x - 0.3).abs() < 1e-5 && (c.y - 0.7).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn planar_uses_xz_of_point() {
        // p.x=0.2, p.z=0.4 → uv (0.2, 0.4); incoming uv ignored.
        let c = probe(Projection::Planar, 1.0, (0.0, 0.0)).value(0.9, 0.9, &Point3::new(0.2, 5.0, 0.4));
        assert!((c.x - 0.2).abs() < 1e-5 && (c.y - 0.4).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn scale_and_offset_tile_with_wrap() {
        // MeshUv, scale 2, offset 0: u 0.6 → 1.2 → wrap → 0.2.
        let c = probe(Projection::MeshUv, 2.0, (0.0, 0.0)).value(0.6, 0.1, &Point3::new(0.0, 0.0, 0.0));
        assert!((c.x - 0.2).abs() < 1e-5, "expected wrapped 0.2, got {c:?}");
    }

    #[test]
    fn spherical_maps_axis_points() {
        // d=(1,0,0): atan2(0,1)/2π+0.5 = 0.5; acos(0)/π = 0.5 → (0.5, 0.5).
        let c = probe(Projection::Spherical, 1.0, (0.0, 0.0)).value(0.9, 0.9, &Point3::new(2.0, 0.0, 0.0));
        assert!((c.x - 0.5).abs() < 1e-5 && (c.y - 0.5).abs() < 1e-5, "{c:?}");
        // d=(0,1,0) (north pole): acos(1)/π = 0 → v = 0.
        let c = probe(Projection::Spherical, 1.0, (0.0, 0.0)).value(0.0, 0.0, &Point3::new(0.0, 3.0, 0.0));
        assert!(c.y.abs() < 1e-5, "north pole v should be 0, got {c:?}");
    }

    #[test]
    fn cylindrical_maps_angle_and_height() {
        // p=(1,0,h): atan2(0,1)/2π+0.5 = 0.5; height bv=p.y. With p.y=0.25 → v 0.25.
        let c = probe(Projection::Cylindrical, 1.0, (0.0, 0.0)).value(0.9, 0.9, &Point3::new(1.0, 0.25, 0.0));
        assert!((c.x - 0.5).abs() < 1e-5 && (c.y - 0.25).abs() < 1e-5, "{c:?}");
    }
}
