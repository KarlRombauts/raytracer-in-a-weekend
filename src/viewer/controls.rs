//! egui widgets that edit a `Scene` in place. Each returns whether the value
//! changed, so the caller can invalidate the render only when something edited.

use eframe::egui;

use super::icons;
use crate::camera::CameraConfig;
use crate::color::Color;
use crate::scene::{MaterialSpec, ObjectSpec, Shape, Transform};
use crate::vec3::{Point3, Vec3};

// --- Small grid-row helpers (call inside an `egui::Grid`) ---

/// `label | x | y | z` row.
fn vec_row(ui: &mut egui::Ui, label: &str, v: &mut Vec3, speed: f32) -> bool {
    ui.label(label);
    let mut c = ui.add(egui::DragValue::new(&mut v.x).speed(speed)).changed();
    c |= ui.add(egui::DragValue::new(&mut v.y).speed(speed)).changed();
    c |= ui.add(egui::DragValue::new(&mut v.z).speed(speed)).changed();
    ui.end_row();
    c
}

/// `label | value` row (f32).
fn f32_row(ui: &mut egui::Ui, label: &str, v: &mut f32, speed: f32) -> bool {
    ui.label(label);
    let c = ui.add(egui::DragValue::new(v).speed(speed)).changed();
    ui.end_row();
    c
}

/// `label | value` row (u32).
fn u32_row(ui: &mut egui::Ui, label: &str, v: &mut u32) -> bool {
    ui.label(label);
    let c = ui.add(egui::DragValue::new(v)).changed();
    ui.end_row();
    c
}

/// `label | swatch` row using a colour picker (clamped to [0,1]).
fn color_row(ui: &mut egui::Ui, label: &str, c: &mut Color) -> bool {
    let mut rgb = [c.x, c.y, c.z];
    ui.label(label);
    let changed = ui.color_edit_button_rgb(&mut rgb).changed();
    ui.end_row();
    if changed {
        *c = Color::new(rgb[0], rgb[1], rgb[2]);
    }
    changed
}

/// `emit | swatch | ×strength` row: an HDR emissive colour split into a
/// normalised hue and an intensity multiplier so lights can exceed 1.0.
fn emissive_row(ui: &mut egui::Ui, emit: &mut Color) -> bool {
    let intensity = emit.x.max(emit.y).max(emit.z).max(1e-4);
    let mut rgb = [emit.x / intensity, emit.y / intensity, emit.z / intensity];
    let mut strength = intensity;

    ui.label("emit");
    let mut changed = ui.color_edit_button_rgb(&mut rgb).changed();
    changed |= ui
        .add(egui::DragValue::new(&mut strength).speed(0.1).prefix("×"))
        .changed();
    ui.end_row();

    if changed {
        *emit = Color::new(rgb[0] * strength, rgb[1] * strength, rgb[2] * strength);
    }
    changed
}

// --- Sections ---

pub fn camera_controls(ui: &mut egui::Ui, cam: &mut CameraConfig) -> bool {
    let mut c = false;
    egui::Grid::new("camera")
        .num_columns(4)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            c |= vec_row(ui, "look from", &mut cam.look_from, 1.0);
            c |= vec_row(ui, "look at", &mut cam.look_at, 1.0);
            c |= f32_row(ui, "fov", &mut cam.fov, 0.2);
            ui.label("roll");
            c |= ui
                .add(egui::Slider::new(&mut cam.roll, -180.0..=180.0).suffix("°"))
                .changed();
            ui.end_row();
            c |= u32_row(ui, "samples", &mut cam.samples);
            c |= u32_row(ui, "depth", &mut cam.max_depth);
            c |= f32_row(ui, "dof", &mut cam.dof_angle, 0.05);
        });
    c
}

/// A Phosphor type icon for the object list.
fn shape_icon(s: &Shape) -> &'static str {
    match s {
        Shape::Sphere { .. } => icons::SPHERE,
        Shape::Quad { .. } => icons::RECTANGLE,
        Shape::Box { .. } => icons::CUBE,
        Shape::Mesh(_) => icons::POLYGON,
    }
}

/// Selectable list of objects + buttons to add new ones. Returns whether the
/// object set changed (selection changes alone don't require a re-render).
pub fn object_list(
    ui: &mut egui::Ui,
    objects: &mut Vec<ObjectSpec>,
    selected: &mut Option<usize>,
) -> bool {
    let mut changed = false;

    for (i, obj) in objects.iter().enumerate() {
        let label = format!("{}  {}", shape_icon(&obj.shape), obj.name);
        if ui.selectable_label(*selected == Some(i), label).clicked() {
            *selected = Some(i);
        }
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .button(format!("{}  {}  Sphere", icons::PLUS, icons::SPHERE))
            .clicked()
        {
            objects.push(default_sphere(objects.len()));
            *selected = Some(objects.len() - 1);
            changed = true;
        }
        if ui
            .button(format!("{}  {}  Box", icons::PLUS, icons::CUBE))
            .clicked()
        {
            objects.push(default_box(objects.len()));
            *selected = Some(objects.len() - 1);
            changed = true;
        }
    });
    changed
}

/// A bold, iconed sub-heading used to group the settings.
fn section_header(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("{}  {}", icon, title)).strong());
}

/// Editor for a single selected object (name, material, geometry, transform).
pub fn object_settings(ui: &mut egui::Ui, obj: &mut ObjectSpec) -> bool {
    let mut changed = false;

    // Renaming is cosmetic, so it doesn't invalidate the render.
    ui.horizontal(|ui| {
        ui.label("name");
        ui.text_edit_singleline(&mut obj.name);
    });

    section_header(ui, icons::PALETTE, "Material");
    ui.indent("material_body", |ui| {
        changed |= material_controls(ui, &mut obj.material);
    });

    // Geometry is shown only for primitives with intuitive parameters. Quads
    // (cryptic q/u/v) and meshes are positioned via the Transform section.
    let show_geometry = matches!(obj.shape, Shape::Sphere { .. } | Shape::Box { .. });
    if show_geometry {
        section_header(ui, icons::SHAPES, "Geometry");
        ui.indent("geometry_body", |ui| {
            changed |= shape_controls(ui, &mut obj.shape);
        });
    }

    if obj.shape.is_editable() {
        section_header(ui, icons::ARROWS_OUT_CARDINAL, "Transform");
        ui.indent("transform_body", |ui| {
            changed |= transform_controls(ui, &mut obj.transform);
        });
    }
    changed
}

fn material_controls(ui: &mut egui::Ui, m: &mut MaterialSpec) -> bool {
    let mut changed = false;
    let current = match m {
        MaterialSpec::Lambertian { .. } => "Lambertian",
        MaterialSpec::Metal { .. } => "Metal",
        MaterialSpec::Dielectric { .. } => "Dielectric",
        MaterialSpec::DiffuseLight { .. } => "Light",
    };
    egui::ComboBox::from_id_salt("mat")
        .selected_text(current)
        .show_ui(ui, |ui| {
            changed |= pick(ui, m, matches!(m, MaterialSpec::Lambertian { .. }), "Lambertian", || {
                MaterialSpec::Lambertian {
                    albedo: Color::new(0.7, 0.7, 0.7),
                }
            });
            changed |= pick(ui, m, matches!(m, MaterialSpec::Metal { .. }), "Metal", || {
                MaterialSpec::Metal {
                    albedo: Color::new(0.8, 0.8, 0.8),
                    fuzz: 0.0,
                }
            });
            changed |= pick(ui, m, matches!(m, MaterialSpec::Dielectric { .. }), "Dielectric", || {
                MaterialSpec::Dielectric { ior: 1.5 }
            });
            changed |= pick(ui, m, matches!(m, MaterialSpec::DiffuseLight { .. }), "Light", || {
                MaterialSpec::DiffuseLight {
                    emit: Color::new(10.0, 10.0, 10.0),
                }
            });
        });

    egui::Grid::new("material")
        .num_columns(3)
        .spacing([6.0, 4.0])
        .show(ui, |ui| match m {
            MaterialSpec::Lambertian { albedo } => changed |= color_row(ui, "albedo", albedo),
            MaterialSpec::Metal { albedo, fuzz } => {
                changed |= color_row(ui, "albedo", albedo);
                changed |= f32_row(ui, "fuzz", fuzz, 0.01);
            }
            MaterialSpec::Dielectric { ior } => changed |= f32_row(ui, "ior", ior, 0.01),
            MaterialSpec::DiffuseLight { emit } => changed |= emissive_row(ui, emit),
        });
    changed
}

/// One selectable row inside the material combo; sets `m` to `make()` on click.
fn pick(
    ui: &mut egui::Ui,
    m: &mut MaterialSpec,
    selected: bool,
    label: &str,
    make: impl FnOnce() -> MaterialSpec,
) -> bool {
    if ui.selectable_label(selected, label).clicked() {
        *m = make();
        true
    } else {
        false
    }
}

fn shape_controls(ui: &mut egui::Ui, s: &mut Shape) -> bool {
    if let Shape::Mesh(_) = s {
        ui.weak("mesh — geometry not editable");
        return false;
    }
    let mut changed = false;
    egui::Grid::new("shape")
        .num_columns(4)
        .spacing([6.0, 4.0])
        .show(ui, |ui| match s {
            Shape::Sphere { center, radius } => {
                changed |= vec_row(ui, "center", center, 1.0);
                changed |= f32_row(ui, "radius", radius, 0.5);
            }
            Shape::Quad { q, u, v } => {
                changed |= vec_row(ui, "q", q, 1.0);
                changed |= vec_row(ui, "u", u, 1.0);
                changed |= vec_row(ui, "v", v, 1.0);
            }
            Shape::Box { a, b } => {
                changed |= vec_row(ui, "min", a, 1.0);
                changed |= vec_row(ui, "max", b, 1.0);
            }
            Shape::Mesh(_) => {}
        });
    changed
}

fn transform_controls(ui: &mut egui::Ui, t: &mut Transform) -> bool {
    let mut changed = false;
    egui::Grid::new("xform")
        .num_columns(4)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            changed |= vec_row(ui, "rotate", &mut t.rotate, 1.0);
            changed |= vec_row(ui, "scale", &mut t.scale, 0.01);
            changed |= vec_row(ui, "translate", &mut t.translate, 1.0);
        });
    changed
}

fn default_sphere(n: usize) -> ObjectSpec {
    ObjectSpec {
        name: format!("Sphere {}", n),
        shape: Shape::Sphere {
            center: Point3::new(278.0, 120.0, 278.0),
            radius: 80.0,
        },
        material: MaterialSpec::Lambertian {
            albedo: Color::new(0.8, 0.3, 0.3),
        },
        transform: Transform::identity(),
    }
}

fn default_box(n: usize) -> ObjectSpec {
    ObjectSpec {
        name: format!("Box {}", n),
        shape: Shape::Box {
            a: Point3::new(200.0, 0.0, 200.0),
            b: Point3::new(360.0, 160.0, 360.0),
        },
        material: MaterialSpec::Lambertian {
            albedo: Color::new(0.7, 0.7, 0.7),
        },
        transform: Transform::identity(),
    }
}
