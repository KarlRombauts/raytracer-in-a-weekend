use crate::vec3::Vec3;

use super::{ObjectSpec, Shape};

/// Duplicate the object at `i`: insert a clone right after it with " copy"
/// appended to the name. Returns the new object's index, or None if `i` is out
/// of range. Cheap — meshes share their `Arc` BVH.
pub fn duplicate_object(objects: &mut Vec<ObjectSpec>, i: usize) -> Option<usize> {
    let mut clone = objects.get(i)?.clone();
    clone.name = format!("{} copy", clone.name);
    objects.insert(i + 1, clone);
    Some(i + 1)
}

/// Rough world-space bounds `(min, max)` of the placeable primitives, using
/// their base shape parameters (ignoring transforms). Meshes are skipped — they
/// can't be bounded without building them. Used to auto-fit imported meshes into
/// the existing scene. Returns `None` if there are no such primitives.
pub fn placeable_bounds(objects: &[ObjectSpec]) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3::new(-f32::INFINITY, -f32::INFINITY, -f32::INFINITY);
    let mut any = false;

    for o in objects {
        let (lo, hi) = match &o.shape {
            Shape::Sphere { center, radius } => {
                let r = Vec3::new(*radius, *radius, *radius);
                (*center - r, *center + r)
            }
            Shape::Box { a, b } => (Vec3::min(a, b), Vec3::max(a, b)),
            Shape::Quad { q, u, v } => {
                let corners = [*q, *q + *u, *q + *v, *q + *u + *v];
                let mut lo = corners[0];
                let mut hi = corners[0];
                for c in &corners[1..] {
                    lo = Vec3::min(&lo, c);
                    hi = Vec3::max(&hi, c);
                }
                (lo, hi)
            }
            Shape::Mesh { .. } => continue,
        };
        min = Vec3::min(&min, &lo);
        max = Vec3::max(&max, &hi);
        any = true;
    }

    any.then_some((min, max))
}

#[cfg(test)]
mod duplicate_tests {
    use super::*;
    use crate::color::Color;
    use crate::scene::{MaterialSpec, Shape, TextureSpec, Transform};
    use crate::vec3::Point3;

    fn emissive(name: &str) -> ObjectSpec {
        ObjectSpec {
            name: name.into(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 0.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 1.0, 0.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        }
    }

    #[test]
    fn duplicate_inserts_clone_after_with_suffixed_name() {
        let mut objs = vec![emissive("Light"), emissive("Box")];
        let new_i = duplicate_object(&mut objs, 0).unwrap();
        assert_eq!(new_i, 1);
        assert_eq!(objs.len(), 3);
        assert_eq!(objs[1].name, "Light copy");
        assert_eq!(objs[2].name, "Box"); // original order preserved after the insert
        assert!(duplicate_object(&mut objs, 99).is_none());
    }
}
