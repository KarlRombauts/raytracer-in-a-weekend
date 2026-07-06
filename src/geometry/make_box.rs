use std::sync::Arc;

use crate::{
    geometry::Quad,
    group::IntersectGroup,
    vec3::{Point3, Vec3},
};

pub fn make_box(a: Point3, b: Point3) -> IntersectGroup {
    // Returns the 3D box (six sides) that contains the two opposite vertices a & b.

    let mut sides = IntersectGroup::new();

    // Construct the two opposite vertices with the minimum and maximum coordinates.
    let min = Point3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
    let max = Point3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));

    let dx = Vec3::new(max.x - min.x, 0.0, 0.0);
    let dy = Vec3::new(0.0, max.y - min.y, 0.0);
    let dz = Vec3::new(0.0, 0.0, max.z - min.z);

    // front
    sides.add(Arc::new(Quad::new(Point3::new(min.x, min.y, max.z), dx, dy)));
    // right
    sides.add(Arc::new(Quad::new(Point3::new(max.x, min.y, max.z), -dz, dy)));
    // back
    sides.add(Arc::new(Quad::new(Point3::new(max.x, min.y, min.z), -dx, dy)));
    // left
    sides.add(Arc::new(Quad::new(Point3::new(min.x, min.y, min.z), dz, dy)));
    // top
    sides.add(Arc::new(Quad::new(Point3::new(min.x, max.y, max.z), dx, -dz)));
    // bottom
    sides.add(Arc::new(Quad::new(Point3::new(min.x, min.y, min.z), dx, dz)));

    sides
}
