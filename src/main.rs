use crate::scenes::{cornell_box, new_bvh};

mod camera;
mod color;
mod geometry;
mod group;
mod interval;
mod material;
mod ray;
mod render;
mod sampling;
mod scene;
mod scenes;
mod texture;
mod vec3;
mod viewer;

fn main() {
    let _ = new_bvh;
    viewer::run(cornell_box());
}
