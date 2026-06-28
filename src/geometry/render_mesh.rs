//! Triangle geometry for the rasterized preview, derived from the same shapes
//! the path tracer uses. Three vertices per triangle. Boxes and quads use flat
//! (per-face) normals for crisp edges; spheres and imported meshes use smooth
//! (radial / vertex-averaged) normals so curved forms shade smoothly. Object-
//! local / definition space — the per-object model matrix applies the transform.

use crate::vec3::{Point3, Vec3};

pub struct RenderMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}

impl RenderMesh {
    fn push_tri(&mut self, a: Point3, b: Point3, c: Point3, center: Point3) {
        let cross_product = (b - a).cross(&(c - a));
        let n = if cross_product.length_squared() < 1e-6 {
            // Degenerate triangle (e.g., at sphere poles): centroid direction from center.
            (((a + b + c) / 3.0) - center).unit()
        } else {
            cross_product.unit()
        };
        for p in [a, b, c] {
            self.positions.push([p.x, p.y, p.z]);
            self.normals.push([n.x, n.y, n.z]);
        }
    }

    /// Push a triangle with explicit per-vertex normals (for smooth shading).
    fn push_tri_smooth(&mut self, a: Point3, b: Point3, c: Point3, na: Vec3, nb: Vec3, nc: Vec3) {
        for (p, n) in [(a, na), (b, nb), (c, nc)] {
            self.positions.push([p.x, p.y, p.z]);
            self.normals.push([n.x, n.y, n.z]);
        }
    }

    pub fn from_triangles(verts: &[Vec3], faces: &[[usize; 3]]) -> RenderMesh {
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        for &[i, j, k] in faces {
            // Degenerate fallback unreachable for real mesh data; Vec3::ZERO is fine.
            m.push_tri(verts[i], verts[j], verts[k], Vec3::ZERO);
        }
        m
    }

    /// Like `from_triangles`, but with smooth (area-weighted, vertex-averaged)
    /// normals so curved organic meshes shade smoothly instead of faceted.
    pub fn from_triangles_smooth(verts: &[Vec3], faces: &[[usize; 3]]) -> RenderMesh {
        // Accumulate area-weighted face normals (un-normalized cross products)
        // into each shared vertex, then normalize.
        let mut acc = vec![Vec3::ZERO; verts.len()];
        for &[i, j, k] in faces {
            let fnv = (verts[j] - verts[i]).cross(&(verts[k] - verts[i]));
            acc[i] = acc[i] + fnv;
            acc[j] = acc[j] + fnv;
            acc[k] = acc[k] + fnv;
        }
        let vn: Vec<Vec3> = acc
            .iter()
            .map(|n| {
                if n.length_squared() < 1e-12 {
                    Vec3::new(0.0, 1.0, 0.0)
                } else {
                    n.unit()
                }
            })
            .collect();

        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        for &[i, j, k] in faces {
            m.push_tri_smooth(verts[i], verts[j], verts[k], vn[i], vn[j], vn[k]);
        }
        m
    }

    pub fn quad(q: Point3, u: Vec3, v: Vec3) -> RenderMesh {
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        // Degenerate fallback unreachable for non-degenerate quads; Vec3::ZERO is fine.
        m.push_tri(q, q + u, q + u + v, Vec3::ZERO);
        m.push_tri(q, q + u + v, q + v, Vec3::ZERO);
        m
    }

    pub fn unit_box(a: Point3, b: Point3) -> RenderMesh {
        let (min, max) = (Vec3::min(&a, &b), Vec3::max(&a, &b));
        // 8 corners
        let c = |x: f32, y: f32, z: f32| Point3::new(x, y, z);
        let corners = [
            c(min.x, min.y, min.z), c(max.x, min.y, min.z),
            c(max.x, max.y, min.z), c(min.x, max.y, min.z),
            c(min.x, min.y, max.z), c(max.x, min.y, max.z),
            c(max.x, max.y, max.z), c(min.x, max.y, max.z),
        ];
        // 6 faces, CCW outward, as two tris each.
        let faces = [
            [0, 3, 2, 1], // -z
            [4, 5, 6, 7], // +z
            [0, 1, 5, 4], // -y
            [3, 7, 6, 2], // +y
            [0, 4, 7, 3], // -x
            [1, 2, 6, 5], // +x
        ];
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        for [i, j, k, l] in faces {
            // Degenerate fallback unreachable for axis-aligned box faces; Vec3::ZERO is fine.
            m.push_tri(corners[i], corners[j], corners[k], Vec3::ZERO);
            m.push_tri(corners[i], corners[k], corners[l], Vec3::ZERO);
        }
        m
    }

    pub fn sphere(center: Point3, radius: f32, rings: u32, segments: u32) -> RenderMesh {
        use std::f32::consts::PI;
        let p = |ring: u32, seg: u32| {
            let theta = PI * ring as f32 / rings as f32; // 0..PI (lat)
            let phi = 2.0 * PI * seg as f32 / segments as f32; // 0..2PI (lon)
            center
                + radius
                    * Vec3::new(
                        theta.sin() * phi.cos(),
                        theta.cos(),
                        theta.sin() * phi.sin(),
                    )
        };
        // Smooth normals: each vertex's normal is its outward radial direction.
        let nrm = |pt: Point3| (pt - center).unit();
        let mut m = RenderMesh { positions: Vec::new(), normals: Vec::new() };
        for ring in 0..rings {
            for seg in 0..segments {
                let (a, b) = (p(ring, seg), p(ring, seg + 1));
                let (cc, d) = (p(ring + 1, seg), p(ring + 1, seg + 1));
                m.push_tri_smooth(a, b, d, nrm(a), nrm(b), nrm(d));
                m.push_tri_smooth(a, d, cc, nrm(a), nrm(d), nrm(cc));
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_len(n: &[f32; 3]) -> bool {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        (l - 1.0).abs() < 1e-4
    }

    #[test]
    fn box_has_12_triangles_and_unit_normals() {
        let m = RenderMesh::unit_box(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
        assert_eq!(m.positions.len(), 36); // 12 tris * 3
        assert_eq!(m.normals.len(), 36);
        assert!(m.normals.iter().all(unit_len));
    }

    #[test]
    fn quad_has_2_triangles() {
        let m = RenderMesh::quad(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert_eq!(m.positions.len(), 6);
        assert!(m.normals.iter().all(unit_len));
    }

    #[test]
    fn sphere_vertex_count_matches_tessellation() {
        let (rings, segs) = (8, 12);
        let m = RenderMesh::sphere(Point3::new(0.0, 0.0, 0.0), 1.0, rings, segs);
        // Each ring band is `segs` quads = 2 tris; `rings` bands.
        assert_eq!(m.positions.len() as u32, rings * segs * 2 * 3);
        assert!(m.normals.iter().all(unit_len));
        // All sphere vertices lie on the radius.
        assert!(m
            .positions
            .iter()
            .all(|p| ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 1.0).abs() < 1e-3));
    }

    #[test]
    fn from_triangles_expands_faces() {
        let verts = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let faces = vec![[0usize, 1, 2]];
        let m = RenderMesh::from_triangles(&verts, &faces);
        assert_eq!(m.positions.len(), 3);
        assert!(unit_len(&m.normals[0]));
    }

    /// Translated sphere pole normals must point radially outward from the sphere
    /// center, not from the world origin. For a sphere at (5,0,0) every face
    /// normal n (paired with any vertex v of that face) must satisfy
    /// n.dot(v - center) >= -1e-3 (outward-ish or tangent, never inward).
    #[test]
    fn translated_sphere_pole_normals_point_outward() {
        let center = Point3::new(5.0, 0.0, 0.0);
        let radius = 1.0_f32;
        let m = RenderMesh::sphere(center, radius, 8, 12);
        // Iterate triangles (3 vertices each)
        let n_verts = m.positions.len();
        assert!(n_verts % 3 == 0);
        for i in (0..n_verts).step_by(3) {
            let v = m.positions[i];
            let n = m.normals[i];
            // vector from sphere center to vertex
            let to_v = [v[0] - center.x, v[1] - center.y, v[2] - center.z];
            let dot = n[0] * to_v[0] + n[1] * to_v[1] + n[2] * to_v[2];
            assert!(
                dot >= -1e-3,
                "face normal points inward: n={:?}, v={:?}, center={:?}, dot={}",
                n, v, [center.x, center.y, center.z], dot
            );
        }
    }
}
