use crate::{
    camera::CameraConfig,
    color::Color,
    scene::{MaterialSpec, ObjectSpec, Scene, Shape, TextureSpec, Transform},
    vec3::{Point3, Vec3},
};

fn quad(name: &str, q: Point3, u: Vec3, v: Vec3, material: MaterialSpec) -> ObjectSpec {
    ObjectSpec {
        name: name.to_string(),
        shape: Shape::Quad { q, u, v },
        material,
        transform: Transform::identity(),
        hidden: false,
    }
}

pub fn cornell_box() -> Scene {
    let red = MaterialSpec::Lambertian {
        albedo: TextureSpec::solid(Color::new(0.65, 0.05, 0.05)),
    };
    let white = MaterialSpec::Lambertian {
        albedo: TextureSpec::solid(Color::new(0.73, 0.73, 0.73)),
    };
    let green = MaterialSpec::Lambertian {
        albedo: TextureSpec::solid(Color::new(0.12, 0.45, 0.15)),
    };
    let light = MaterialSpec::DiffuseLight {
        emit: TextureSpec::solid(Color::new(15.0, 15.0, 15.0)),
    };

    let objects = vec![
        quad(
            "Right wall",
            Point3::new(555.0, 0.0, 0.0),
            Vec3::new(0.0, 555.0, 0.0),
            Vec3::new(0.0, 0.0, 555.0),
            green,
        ),
        quad(
            "Left wall",
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 555.0, 0.0),
            Vec3::new(0.0, 0.0, 555.0),
            red,
        ),
        quad(
            "Light",
            Point3::new(343.0, 554.0, 332.0),
            Vec3::new(-130.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -105.0),
            light,
        ),
        quad(
            "Floor",
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(555.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 555.0),
            white.clone(),
        ),
        quad(
            "Ceiling",
            Point3::new(555.0, 555.0, 555.0),
            Vec3::new(-555.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -555.0),
            white.clone(),
        ),
        quad(
            "Back wall",
            Point3::new(0.0, 0.0, 555.0),
            Vec3::new(555.0, 0.0, 0.0),
            Vec3::new(0.0, 555.0, 0.0),
            white.clone(),
        ),
        ObjectSpec {
            name: "Tall box".to_string(),
            shape: Shape::Box {
                a: Point3::new(130.0, 0.0, 65.0),
                b: Point3::new(295.0, 165.0, 230.0),
            },
            material: white.clone(),
            transform: Transform::identity(),
            hidden: false,
        },
        ObjectSpec {
            name: "Short box".to_string(),
            shape: Shape::Box {
                a: Point3::new(265.0, 0.0, 295.0),
                b: Point3::new(430.0, 330.0, 460.0),
            },
            material: white,
            transform: Transform::identity(),
            hidden: false,
        },
    ];

    let camera = CameraConfig::builder()
        .aspect_ratio(1.0)
        .image_width(600)
        .samples(200)
        .max_depth(50)
        .background(Color::zeros())
        .fov(40.0)
        .look_from(Vec3::new(278.0, 278.0, -800.0))
        .look_at(Vec3::new(278.0, 278.0, 0.0))
        .dof_angle(0.0)
        .firefly_clamp(10.0)
        .build();

    Scene { camera, objects }
}
