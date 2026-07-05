use std::sync::Arc;

use crate::{
    camera::CameraConfig,
    color::Color,
    geometry::ObjData,
    scene::{MaterialSpec, MeshData, ObjectSpec, Scene, Shape, TextureSpec, Transform},
    vec3::{Point3, Vec3},
};

pub fn new_bvh() -> Scene {
    let obj = ObjData::load("./objs/dragon.obj");
    let (verts, faces, uvs) = obj.mesh_data();
    let data = Arc::new(MeshData { verts, faces, uvs });
    let (object, render) = data.build();

    let dragon = ObjectSpec {
        name: "Dragon (mesh)".to_string(),
        shape: Shape::Mesh { data, object, render },
        // Mesh keeps its baked material; this is ignored but required by the spec.
        material: MaterialSpec::Metal {
            albedo: Color::new(1.0, 0.4, 0.2),
            fuzz: 0.2,
        },
        transform: Transform::identity(),
        hidden: false,
    };

    let floor = ObjectSpec {
        name: "Floor".to_string(),
        shape: Shape::Quad {
            q: Point3::new(-500., 0.1, 20.),
            u: Vec3::new(3000., 0.1, 0.),
            v: Vec3::new(0., 0.1, -3000.),
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.8, 0.8, 0.8)),
        },
        transform: Transform::identity(),
        hidden: false,
    };

    let camera = CameraConfig::builder()
        .aspect_ratio(4. / 3.)
        .image_width(1200)
        .samples(400)
        .max_depth(10)
        .fov(30.)
        .look_from(Vec3::new(10., 8., 10.))
        .look_at(Vec3::new(0., 2.2, 0.))
        .build();

    Scene {
        camera,
        objects: vec![dragon, floor],
    }
}
