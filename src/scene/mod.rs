//! The Scene document and the world build.
//!
//! A **Scene** is the editable, serializable document — a camera plus a set of
//! objects. Its plain-data *spec* types (textures, materials, shapes, objects)
//! each know how to `build()` themselves into runtime machinery. The **World**
//! ([`crate::world::World`]) is the acceleration-backed runtime structure the
//! path tracer walks; [`build_world`] is the single seam that assembles one from
//! a Scene.
//!
//! Module layout mirrors those responsibilities:
//! - [`texture`], [`material`], [`shape`], [`object`] — the document spec types,
//!   split by kind, each with its own `build()` and tests.
//! - [`build`] — the world build: [`build_world`] plus its (private) placement,
//!   baking, and light-collection helpers.
//! - [`edit`] — editor document operations over a `Vec<ObjectSpec>`.
//!
//! The spec types and `build_world` are re-exported here, so `crate::scene::…`
//! resolves regardless of which submodule an item lives in.

use serde::{Deserialize, Serialize};

use crate::camera::CameraConfig;

mod build;
mod edit;
mod material;
mod object;
mod shape;
mod texture;

pub use build::*;
pub use edit::*;
pub use material::*;
pub use object::*;
pub use shape::*;
pub use texture::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    pub camera: CameraConfig,
    pub objects: Vec<ObjectSpec>,
}
