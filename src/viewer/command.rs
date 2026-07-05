use crate::scene::{ObjectSpec, Scene};

use super::state::Selection;

/// A scene edit requested by a panel or a keyboard shortcut, interpreted in one
/// place (`apply_scene_command`) against the scene and the selection. Unlike
/// `Action` (app-level one-shots that need render/history/platform resources),
/// a `SceneCommand` carries its data and its whole effect is `(Scene, Selection)
/// -> bool changed` — no egui, no app, so it is unit-testable.
pub enum SceneCommand {
    /// Append an object and select it.
    AddObject(ObjectSpec),
    /// Remove the object at this index.
    DeleteObject(usize),
    /// Clone the object at this index (inserted after it) and select the copy.
    DuplicateObject(usize),
}

/// Apply one scene edit and keep the selection valid. Returns whether the scene
/// actually changed (so the caller can flag a re-render / open an undo entry).
pub fn apply_scene_command(
    cmd: SceneCommand,
    scene: &mut Scene,
    selection: &mut Selection,
) -> bool {
    match cmd {
        SceneCommand::AddObject(obj) => {
            scene.objects.push(obj);
            selection.set(scene.objects.len() - 1);
            true
        }
        SceneCommand::DeleteObject(i) => {
            if i < scene.objects.len() {
                scene.objects.remove(i);
                selection.clear();
                true
            } else {
                false
            }
        }
        SceneCommand::DuplicateObject(i) => match crate::scene::duplicate_object(&mut scene.objects, i) {
            Some(n) => {
                selection.set(n);
                true
            }
            None => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::scene::{MaterialSpec, Shape, TextureSpec, Transform};
    use crate::vec3::Point3;

    fn obj(name: &str) -> ObjectSpec {
        ObjectSpec {
            name: name.to_string(),
            shape: Shape::Sphere {
                center: Point3::new(0.0, 0.0, 0.0),
                radius: 1.0,
            },
            material: MaterialSpec::Lambertian {
                albedo: TextureSpec::solid(Color::new(0.5, 0.5, 0.5)),
            },
            transform: Transform::identity(),
            hidden: false,
        }
    }

    fn scene_with(names: &[&str]) -> Scene {
        Scene {
            camera: CameraConfig::builder().build(),
            objects: names.iter().map(|n| obj(n)).collect(),
        }
    }

    #[test]
    fn add_object_appends_and_selects_it() {
        let mut scene = scene_with(&["a"]);
        let mut sel = Selection::default();
        let changed = apply_scene_command(SceneCommand::AddObject(obj("b")), &mut scene, &mut sel);
        assert!(changed);
        assert_eq!(scene.objects.len(), 2);
        assert_eq!(sel.get(scene.objects.len()), Some(1));
    }

    #[test]
    fn delete_object_removes_and_clears_selection() {
        let mut scene = scene_with(&["a", "b"]);
        let mut sel = Selection::default();
        sel.set(1);
        let changed = apply_scene_command(SceneCommand::DeleteObject(0), &mut scene, &mut sel);
        assert!(changed);
        assert_eq!(scene.objects.len(), 1);
        assert_eq!(scene.objects[0].name, "b");
        assert_eq!(sel.get(scene.objects.len()), None);
    }

    #[test]
    fn delete_out_of_range_is_a_noop() {
        let mut scene = scene_with(&["a"]);
        let mut sel = Selection::default();
        let changed = apply_scene_command(SceneCommand::DeleteObject(5), &mut scene, &mut sel);
        assert!(!changed);
        assert_eq!(scene.objects.len(), 1);
    }

    #[test]
    fn duplicate_object_inserts_after_and_selects_the_copy() {
        let mut scene = scene_with(&["a", "b"]);
        let mut sel = Selection::default();
        let changed = apply_scene_command(SceneCommand::DuplicateObject(0), &mut scene, &mut sel);
        assert!(changed);
        assert_eq!(scene.objects.len(), 3);
        assert_eq!(sel.get(scene.objects.len()), Some(1));
        assert_eq!(scene.objects[1].name, "a copy");
    }

    #[test]
    fn selection_get_rejects_a_stale_index() {
        // A selection that pointed past the end (e.g. after an undo shrank the
        // scene) validates to None rather than handing back an invalid index.
        let mut sel = Selection::default();
        sel.set(4);
        assert_eq!(sel.get(2), None);
        assert_eq!(sel.get(9), Some(4));
    }
}
