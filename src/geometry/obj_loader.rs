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
    faces: Vec<[usize; 3]>,
}

impl ObjData {
    pub fn load(file_name: &str) -> Self {
        let raw = fs::read_to_string(file_name).unwrap();

        let mut data = ObjData {
            verts: vec![],
            faces: vec![],
        };

        for line in raw.lines() {
            match line.split_whitespace().next() {
                Some("v") => {
                    data.parse_vert(line);
                }
                Some("f") => {
                    data.parse_face(line);
                }
                _ => (),
            }
        }

        data
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
        let mut v = line.split_whitespace().skip(1).map(|mut s| {
            if s.contains("/") {
                s = s.split("/").next().unwrap()
            }
            s.parse::<usize>()
                .expect("face index was not a valid integer")
        });

        let a = v.next().unwrap();
        let b = v.next().unwrap();
        let c = v.next().unwrap();

        let face = [a - 1, b - 1, c - 1];
        self.faces.push(face);
        if let Some(d) = v.next() {
            let face = [a - 1, c - 1, d - 1];
            self.faces.push(face);
        }
    }

    pub fn into_mesh(self, material: Arc<dyn Material>) -> IntersectGroup {
        let mut group = IntersectGroup::new();

        println!("Loaded {} faces", self.faces.len());
        self.faces.into_iter().for_each(|[i, j, k]| {
            let tri = Arc::new(Triangle::from_points(
                &self.verts[i],
                &self.verts[j],
                &self.verts[k],
                material.clone(),
            )) as Arc<dyn Intersect>;
            group.add(tri);
        });

        group
    }
}
