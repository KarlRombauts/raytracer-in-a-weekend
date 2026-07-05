use crate::{
    camera::CameraConfig,
    color::Color,
    scene::{MaterialSpec, ObjectSpec, Scene, Shape, TextureSpec, Transform},
    vec3::{Point3, Vec3},
};

/// A wall: a unit quad (1×1, centred on the origin in the XY plane) sized,
/// oriented and placed by a transform. Building the room from unit primitives
/// keeps it described in clean 4-unit proportions instead of raw coordinates.
fn wall(name: &str, material: MaterialSpec, scale: Vec3, rotate: Vec3, translate: Vec3) -> ObjectSpec {
    ObjectSpec {
        name: name.to_string(),
        shape: Shape::Quad {
            q: Point3::new(-0.5, -0.5, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
        },
        material,
        transform: Transform {
            rotate,
            scale,
            translate,
        },
        hidden: false,
    }
}

/// A box: a unit cube (1×1×1, centred on the origin) sized and placed by a
/// transform. `scale` gives its full extents, `translate` its centre.
fn cube(name: &str, material: MaterialSpec, scale: Vec3, translate: Vec3) -> ObjectSpec {
    ObjectSpec {
        name: name.to_string(),
        shape: Shape::Box {
            a: Point3::new(-0.5, -0.5, -0.5),
            b: Point3::new(0.5, 0.5, 0.5),
        },
        material,
        transform: Transform {
            rotate: Vec3::ZERO,
            scale,
            translate,
        },
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

    // A 4×4×4 room: floor at y=0, ceiling at y=4, walls at x=±2 and z=+2, open
    // toward the camera (−z). Walls are 4×4 quads (scale (4,4,1)).
    let wall_scale = Vec3::new(4.0, 4.0, 1.0);

    let objects = vec![
        // Side walls: the unit quad's XY plane rotated 90° about Y into ZY.
        wall(
            "Right wall",
            green,
            wall_scale,
            Vec3::new(0.0, 90.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
        ),
        wall(
            "Left wall",
            red,
            wall_scale,
            Vec3::new(0.0, 90.0, 0.0),
            Vec3::new(-2.0, 2.0, 0.0),
        ),
        // Ceiling light: kept as a directly-sized quad rather than a transformed
        // unit primitive. Only shapes with non-zero `area()` are registered for
        // direct light sampling in `build_world`, and the transform wrappers
        // don't forward `area()` — so a bare Quad keeps the one light importance-
        // sampled (and far less noisy). A ~1.0×0.8 panel centred on the ceiling.
        ObjectSpec {
            name: "Light".to_string(),
            shape: Shape::Quad {
                q: Point3::new(-0.5, 3.99, -0.4),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 0.8),
            },
            material: light,
            transform: Transform::identity(),
            hidden: false,
        },
        // Floor and ceiling: the unit quad's XY plane rotated 90° about X into XZ.
        wall(
            "Floor",
            white.clone(),
            wall_scale,
            Vec3::new(90.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
        ),
        wall(
            "Ceiling",
            white.clone(),
            wall_scale,
            Vec3::new(90.0, 0.0, 0.0),
            Vec3::new(0.0, 4.0, 0.0),
        ),
        // Back wall: the unit quad is already in the XY plane.
        wall(
            "Back wall",
            white.clone(),
            wall_scale,
            Vec3::ZERO,
            Vec3::new(0.0, 2.0, 2.0),
        ),
        // Two boxes, sitting on the floor (centre y = half-height).
        cube(
            "Tall box",
            white.clone(),
            Vec3::new(1.2, 1.2, 1.2),
            Vec3::new(-0.5, 0.6, -0.9),
        ),
        cube(
            "Short box",
            white,
            Vec3::new(1.2, 2.4, 1.2),
            Vec3::new(0.5, 1.2, 0.7),
        ),
    ];

    let camera = CameraConfig::builder()
        .aspect_ratio(1.0)
        .image_width(600)
        .samples(200)
        .max_depth(50)
        .background(Color::zeros())
        .fov(40.0)
        .look_from(Vec3::new(0.0, 2.0, -7.77))
        .look_at(Vec3::new(0.0, 2.0, -2.0))
        .dof_angle(0.0)
        .firefly_clamp(10.0)
        .build();

    Scene { camera, objects }
}
