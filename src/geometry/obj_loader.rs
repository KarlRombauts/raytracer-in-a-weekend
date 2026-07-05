use crate::{
    color::Color,
    geometry::Triangle,
    group::IntersectGroup,
    material::{self, Lambertian, Material},
    ray::Intersect,
    vec3::Vec3,
};
use std::{fs, sync::Arc};

pub struct ObjData {
    verts: Vec<Vec3>,
    /// `vt` texture coordinates, indexed by the second field of a face corner.
    tex_coords: Vec<[f32; 2]>,
    faces: Vec<[usize; 3]>,
    /// Per-triangle `vt` indices, aligned 1:1 with `faces`. `None` for a corner
    /// that gave no texture index (`v` or `v//vn`).
    face_vt: Vec<[Option<usize>; 3]>,
}

impl ObjData {
    pub fn load(file_name: &str) -> Self {
        Self::parse(&fs::read_to_string(file_name).unwrap())
    }

    /// Parse OBJ text directly (no filesystem) — used by the web upload path,
    /// where the file's bytes arrive in memory rather than as a path.
    pub fn parse(raw: &str) -> Self {
        let mut data = ObjData {
            verts: vec![],
            tex_coords: vec![],
            faces: vec![],
            face_vt: vec![],
        };

        for line in raw.lines() {
            match line.split_whitespace().next() {
                Some("v") => {
                    data.parse_vert(line);
                }
                Some("vt") => {
                    data.parse_texcoord(line);
                }
                Some("f") => {
                    data.parse_face(line);
                }
                _ => (),
            }
        }

        data
    }

    fn parse_texcoord(&mut self, line: &str) {
        let mut t = line
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse::<f32>().unwrap_or(0.0));
        let u = t.next().unwrap_or(0.0);
        let v = t.next().unwrap_or(0.0);
        self.tex_coords.push([u, v]);
    }

    fn parse_vert(&mut self, line: &str) {
        let mut v = line.split_whitespace().skip(1).map(|s| {
            s.parse::<f32>()
                .expect("vertex coordinate was not a valid float")
        });

        let x = v.next().unwrap();
        let y = v.next().unwrap();
        let z = v.next().unwrap();

        self.verts.push(Vec3::new(x, y, z));
    }

    fn parse_face(&mut self, line: &str) {
        // Each face vertex is `v`, `v/vt`, or `v/vt/vn`; keep the position index
        // and the (optional) texture index, both converted 1-based -> 0-based.
        let corners: Vec<(usize, Option<usize>)> = line
            .split_whitespace()
            .skip(1)
            .map(|s| {
                let mut parts = s.split('/');
                let pos = parts
                    .next()
                    .unwrap()
                    .parse::<usize>()
                    .expect("face index was not a valid integer")
                    - 1;
                let vt = parts
                    .next()
                    .filter(|p| !p.is_empty())
                    .map(|p| p.parse::<usize>().expect("texture index was not a valid integer") - 1);
                (pos, vt)
            })
            .collect();

        // Fan-triangulate the polygon from its first vertex: an n-gon becomes
        // n-2 triangles (v0,v1,v2), (v0,v2,v3), ... so no vertices are dropped.
        for w in corners.windows(2).skip(1) {
            self.faces.push([corners[0].0, w[0].0, w[1].0]);
            self.face_vt.push([corners[0].1, w[0].1, w[1].1]);
        }
    }

    pub fn render_mesh(&self) -> crate::geometry::RenderMesh {
        crate::geometry::RenderMesh::from_triangles_smooth(&self.verts, &self.faces)
    }

    /// Resolve a corner's `vt` index to its coordinate, defaulting to (0, 0) when
    /// the corner had no texture index or it's out of range.
    fn uv_at(&self, idx: Option<usize>) -> [f32; 2] {
        idx.and_then(|i| self.tex_coords.get(i)).copied().unwrap_or([0.0, 0.0])
    }

    /// The mesh's positions, triangle indices, and per-triangle texture
    /// coordinates — the portable description (everything else is rebuilt from
    /// these). The UV vector is empty when the OBJ carried no `vt` data, in which
    /// case meshes fall back to barycentric coordinates.
    pub fn mesh_data(&self) -> (Vec<crate::vec3::Vec3>, Vec<[u32; 3]>, Vec<[[f32; 2]; 3]>) {
        let faces = self
            .faces
            .iter()
            .map(|[i, j, k]| [*i as u32, *j as u32, *k as u32])
            .collect();
        let uvs = if self.tex_coords.is_empty() {
            Vec::new()
        } else {
            self.face_vt
                .iter()
                .map(|[a, b, c]| [self.uv_at(*a), self.uv_at(*b), self.uv_at(*c)])
                .collect()
        };
        (self.verts.clone(), faces, uvs)
    }

    pub fn into_mesh(self, material: Arc<dyn Material>) -> IntersectGroup {
        let mut group = IntersectGroup::new();

        println!("Loaded {} faces", self.faces.len());
        let vn = crate::geometry::vertex_normals(&self.verts, &self.faces);
        self.faces.into_iter().for_each(|[i, j, k]| {
            let tri = Arc::new(Triangle::from_points_smooth(
                &self.verts[i],
                &self.verts[j],
                &self.verts[k],
                &vn[i],
                &vn[j],
                &vn[k],
                material.clone(),
            )) as Arc<dyn Intersect>;
            group.add(tri);
        });

        group
    }

    pub fn into_triangles(self, material: Arc<dyn Material>) -> Vec<Triangle> {
        let mut triangles = Vec::new();

        println!("Loaded {} faces", self.faces.len());
        let vn = crate::geometry::vertex_normals(&self.verts, &self.faces);
        self.faces.into_iter().for_each(|[i, j, k]| {
            let tri = Triangle::from_points_smooth(
                &self.verts[i],
                &self.verts[j],
                &self.verts[k],
                &vn[i],
                &vn[j],
                &vn[k],
                material.clone(),
            );
            triangles.push(tri);
        });

        return triangles;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Vec<[usize; 3]> {
        let mut data = ObjData {
            verts: vec![],
            tex_coords: vec![],
            faces: vec![],
            face_vt: vec![],
        };
        data.parse_face(line);
        data.faces
    }

    #[test]
    fn triangle_face_yields_one_triangle() {
        assert_eq!(parse("f 1 2 3"), vec![[0, 1, 2]]);
    }

    #[test]
    fn quad_face_fans_into_two_triangles() {
        assert_eq!(parse("f 1 2 3 4"), vec![[0, 1, 2], [0, 2, 3]]);
    }

    #[test]
    fn pentagon_face_fans_into_three_triangles() {
        // An n-gon must triangulate into n-2 triangles via a fan from the first
        // vertex; none of its vertices may be dropped.
        assert_eq!(parse("f 1 2 3 4 5"), vec![[0, 1, 2], [0, 2, 3], [0, 3, 4]]);
    }

    #[test]
    fn ngon_with_face_index_groups_fans_completely() {
        // Indices like `v/vt/vn` keep only the position index, and a 6-gon still
        // produces 4 triangles.
        let faces = parse("f 1/1/1 2/2/2 3/3/3 4/4/4 5/5/5 6/6/6");
        assert_eq!(
            faces,
            vec![[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]],
        );
    }
}
