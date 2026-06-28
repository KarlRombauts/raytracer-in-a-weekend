//! egui widgets that edit a `Scene` in place. Each returns whether the value
//! changed, so the caller can invalidate the render only when something edited.

use std::ops::RangeInclusive;

use eframe::egui;

use super::icons;
use crate::camera::CameraConfig;
use crate::color::Color;
use crate::scene::{self, MaterialSpec, ObjectSpec, Shape, Transform};
use crate::vec3::{Point3, Vec3};

/// `label | value` row (u32), Blender-style, optionally clamped to `range`.
fn int_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    range: Option<RangeInclusive<u32>>,
) -> bool {
    let mut dv = egui::DragValue::new(value).speed(1.0);
    if let Some(r) = range {
        dv = dv.range(r);
    }
    let mut changed = false;
    ui.horizontal(|ui| {
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(AXIS_LABEL_W, h),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(label);
            },
        );
        changed = ui.add_sized([ui.available_width(), h], dv).changed();
    });
    changed
}

/// A `label | content` row (Blender-style): right-aligned label in a fixed
/// column, then `content` (combo, swatch, …) filling the rest of the width.
/// Shares `AXIS_LABEL_W` with `axis_row` so labels line up across sections.
fn prop_row<R>(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(AXIS_LABEL_W, h),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(label);
            },
        );
        content(ui)
    })
    .inner
}

/// A right-aligned-label colour-swatch row.
fn color_prop(ui: &mut egui::Ui, label: &str, c: &mut Color) -> bool {
    prop_row(ui, label, |ui| {
        let mut rgb = [c.x, c.y, c.z];
        let changed = ui.color_edit_button_rgb(&mut rgb).changed();
        if changed {
            *c = Color::new(rgb[0], rgb[1], rgb[2]);
        }
        changed
    })
}

// --- Sections ---

pub fn camera_controls(ui: &mut egui::Ui, cam: &mut CameraConfig) -> bool {
    let mut c = false;

    section_header(ui, icons::CROSSHAIR, "View");
    ui.indent("cam_view", |ui| {
        c |= axis_vec(ui, "Position", &mut cam.look_from, 1.0, "", None, None);
        ui.add_space(4.0);
        c |= axis_vec(ui, "Target", &mut cam.look_at, 1.0, "", None, None);
        ui.add_space(4.0);
        c |= axis_row(ui, "Roll", &mut cam.roll, 0.5, "°", Some(1), Some(-180.0..=180.0));
    });

    section_header(ui, icons::APERTURE, "Lens");
    ui.indent("cam_lens", |ui| {
        c |= axis_row(ui, "FOV", &mut cam.fov, 0.2, "°", Some(1), Some(1.0..=179.0));
        c |= axis_row(ui, "DoF", &mut cam.dof_angle, 0.05, "°", Some(2), Some(0.0..=180.0));
        c |= axis_row(ui, "Focus", &mut cam.focus_dist, 1.0, "", Some(1), Some(0.001..=1.0e6));
    });

    section_header(ui, icons::IMAGE, "Output");
    ui.indent("cam_output", |ui| {
        // Resolution: width and height are edited independently — changing one
        // adjusts the aspect ratio so the other stays put.
        let cur_h = ((cam.image_width as f64 / cam.aspect_ratio).round().max(1.0)) as u32;
        c |= prop_row(ui, "Width", |ui| {
            let h = ui.spacing().interact_size.y;
            let mut w = cam.image_width;
            if ui
                .add_sized(
                    [ui.available_width(), h],
                    egui::DragValue::new(&mut w).speed(4.0).range(1..=8192),
                )
                .changed()
            {
                cam.image_width = w.max(1);
                cam.aspect_ratio = cam.image_width as f64 / cur_h as f64;
                true
            } else {
                false
            }
        });
        c |= prop_row(ui, "Height", |ui| {
            let h = ui.spacing().interact_size.y;
            let mut hpx = cur_h;
            if ui
                .add_sized(
                    [ui.available_width(), h],
                    egui::DragValue::new(&mut hpx).speed(4.0).range(1..=8192),
                )
                .changed()
            {
                cam.aspect_ratio = cam.image_width as f64 / hpx.max(1) as f64;
                true
            } else {
                false
            }
        });
        c |= int_row(ui, "Samples", &mut cam.samples, Some(1..=100_000));
        c |= int_row(ui, "Max Bounces", &mut cam.max_depth, Some(1..=1_000));
    });

    c
}

/// A Phosphor type icon for the object list.
fn shape_icon(s: &Shape) -> &'static str {
    match s {
        Shape::Sphere { .. } => icons::SPHERE,
        Shape::Quad { .. } => icons::RECTANGLE,
        Shape::Box { .. } => icons::CUBE,
        Shape::Mesh { .. } => icons::POLYGON,
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
    ui.horizontal_wrapped(|ui| {
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
        if ui
            .button(format!("{}  {}  OBJ", icons::PLUS, icons::POLYGON))
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Wavefront OBJ", &["obj"])
                .pick_file()
            {
                // Auto-fit the import into the existing scene: centre it and
                // scale it to a fraction of the scene's size, so it's visible
                // regardless of the OBJ's native units.
                let (center, size) = match scene::placeable_bounds(objects) {
                    Some((min, max)) => {
                        let extent = max - min;
                        let span = extent.x.max(extent.y).max(extent.z);
                        ((min + max) * 0.5, span * 0.33)
                    }
                    None => (Vec3::ZERO, 2.0),
                };
                if let Some(obj) = ObjectSpec::from_obj(&path, center, size) {
                    objects.push(obj);
                    *selected = Some(objects.len() - 1);
                    changed = true;
                }
            }
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

    let is_mesh = matches!(obj.shape, Shape::Mesh { .. });

    section_header(ui, icons::PALETTE, "Material");
    ui.indent("material_body", |ui| {
        if is_mesh {
            // A mesh's material is baked into its triangles at import; editing
            // it would mean rebuilding the BVH, so it's fixed here.
            ui.weak("baked at import");
        } else {
            changed |= material_controls(ui, &mut obj.material);
        }
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

    // Transform applies to every object — it wraps the geometry (including a
    // mesh's BVH) with Translate/Rotate/Scale, with no rebuild.
    section_header(ui, icons::ARROWS_OUT_CARDINAL, "Transform");
    ui.indent("transform_body", |ui| {
        changed |= transform_controls(ui, &mut obj.transform);
    });
    changed
}

/// Base colour shared across material types (used to carry it over on a type
/// switch). Emission returns its normalised hue.
fn shared_color(m: &MaterialSpec) -> Color {
    match m {
        MaterialSpec::Lambertian { albedo } => *albedo,
        MaterialSpec::Glossy { albedo, .. } => *albedo,
        MaterialSpec::Metal { albedo, .. } => *albedo,
        MaterialSpec::Dielectric { tint, .. } => *tint,
        MaterialSpec::DiffuseLight { emit } => {
            *emit / emit.x.max(emit.y).max(emit.z).max(1e-4)
        }
    }
}

/// Roughness-like parameter shared across material types (carried over on a
/// switch). Types without one report 0.
fn shared_roughness(m: &MaterialSpec) -> f32 {
    match m {
        MaterialSpec::Glossy { roughness, .. } => *roughness,
        MaterialSpec::Metal { fuzz, .. } => *fuzz,
        MaterialSpec::Dielectric { roughness, .. } => *roughness,
        MaterialSpec::Lambertian { .. } | MaterialSpec::DiffuseLight { .. } => 0.0,
    }
}

fn material_controls(ui: &mut egui::Ui, m: &mut MaterialSpec) -> bool {
    let mut changed = false;

    // Natural, Blender-ish names for the shader types.
    let current = match m {
        MaterialSpec::Lambertian { .. } => "Diffuse",
        MaterialSpec::Glossy { .. } => "Glossy",
        MaterialSpec::Metal { .. } => "Metal",
        MaterialSpec::Dielectric { .. } => "Glass",
        MaterialSpec::DiffuseLight { .. } => "Emission",
    };

    changed |= prop_row(ui, "Surface", |ui| {
        let mut c = false;
        egui::ComboBox::from_id_salt("surface")
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                // Each builder receives the current shared colour/roughness so
                // switching type keeps those values instead of resetting them.
                c |= pick(ui, m, matches!(m, MaterialSpec::Lambertian { .. }), "Diffuse", |col, _| {
                    MaterialSpec::Lambertian { albedo: col }
                });
                c |= pick(ui, m, matches!(m, MaterialSpec::Glossy { .. }), "Glossy", |col, r| {
                    MaterialSpec::Glossy {
                        albedo: col,
                        roughness: r,
                    }
                });
                c |= pick(ui, m, matches!(m, MaterialSpec::Metal { .. }), "Metal", |col, r| {
                    MaterialSpec::Metal {
                        albedo: col,
                        fuzz: r,
                    }
                });
                c |= pick(ui, m, matches!(m, MaterialSpec::Dielectric { .. }), "Glass", |col, r| {
                    MaterialSpec::Dielectric {
                        ior: 1.5,
                        tint: col,
                        roughness: r,
                    }
                });
                c |= pick(ui, m, matches!(m, MaterialSpec::DiffuseLight { .. }), "Emission", |col, _| {
                    MaterialSpec::DiffuseLight {
                        emit: col * 5.0,
                    }
                });
            });
        c
    });

    match m {
        MaterialSpec::Lambertian { albedo } => changed |= color_prop(ui, "Color", albedo),
        MaterialSpec::Glossy { albedo, roughness } => {
            changed |= color_prop(ui, "Color", albedo);
            changed |= axis_row(ui, "Roughness", roughness, 0.01, "", Some(3), Some(0.0..=1.0));
        }
        MaterialSpec::Metal { albedo, fuzz } => {
            changed |= color_prop(ui, "Color", albedo);
            changed |= axis_row(ui, "Roughness", fuzz, 0.01, "", Some(3), Some(0.0..=1.0));
        }
        MaterialSpec::Dielectric {
            ior,
            tint,
            roughness,
        } => {
            changed |= color_prop(ui, "Color", tint);
            changed |= axis_row(ui, "Roughness", roughness, 0.01, "", Some(3), Some(0.0..=1.0));
            changed |= axis_row(ui, "IOR", ior, 0.01, "", Some(3), Some(1.0..=3.0));
        }
        MaterialSpec::DiffuseLight { emit } => {
            // Split the HDR colour into a normalised hue + a strength multiplier.
            let intensity = emit.x.max(emit.y).max(emit.z).max(1e-4);
            let mut rgb = [emit.x / intensity, emit.y / intensity, emit.z / intensity];
            let mut strength = intensity;
            let col = prop_row(ui, "Color", |ui| ui.color_edit_button_rgb(&mut rgb).changed());
            let str_changed =
                axis_row(ui, "Strength", &mut strength, 0.1, "", Some(2), Some(0.0..=10_000.0));
            if col || str_changed {
                *emit = Color::new(rgb[0] * strength, rgb[1] * strength, rgb[2] * strength);
                changed = true;
            }
        }
    }
    changed
}

/// One selectable row inside the material combo. On click it sets `m` to
/// `make(shared_color, shared_roughness)`, preserving those across the switch.
fn pick(
    ui: &mut egui::Ui,
    m: &mut MaterialSpec,
    selected: bool,
    label: &str,
    make: impl FnOnce(Color, f32) -> MaterialSpec,
) -> bool {
    if ui.selectable_label(selected, label).clicked() {
        *m = make(shared_color(m), shared_roughness(m));
        true
    } else {
        false
    }
}

// --- Blender-style stacked axis rows: right-aligned label, full-width field ---

const AXIS_LABEL_W: f32 = 84.0;

/// One `label | [ value ]` row: a right-aligned label in a fixed column, then a
/// drag field that fills the rest of the width (Blender property style).
fn axis_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    speed: f32,
    suffix: &str,
    decimals: Option<usize>,
    range: Option<RangeInclusive<f32>>,
) -> bool {
    let mut dv = egui::DragValue::new(value).speed(speed);
    if !suffix.is_empty() {
        dv = dv.suffix(suffix);
    }
    if let Some(d) = decimals {
        dv = dv.fixed_decimals(d);
    }
    if let Some(r) = range {
        dv = dv.range(r);
    }

    let mut changed = false;
    ui.horizontal(|ui| {
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(AXIS_LABEL_W, h),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(label);
            },
        );
        changed = ui.add_sized([ui.available_width(), h], dv).changed();
    });
    changed
}

/// A `name X` / `Y` / `Z` stack, Blender-style (only the first row is named).
fn axis_vec(
    ui: &mut egui::Ui,
    name: &str,
    v: &mut Vec3,
    speed: f32,
    suffix: &str,
    decimals: Option<usize>,
    range: Option<RangeInclusive<f32>>,
) -> bool {
    let mut c = false;
    c |= axis_row(ui, &format!("{name} X"), &mut v.x, speed, suffix, decimals, range.clone());
    c |= axis_row(ui, "Y", &mut v.y, speed, suffix, decimals, range.clone());
    c |= axis_row(ui, "Z", &mut v.z, speed, suffix, decimals, range);
    c
}

fn shape_controls(ui: &mut egui::Ui, s: &mut Shape) -> bool {
    let mut changed = false;
    match s {
        Shape::Sphere { center, radius } => {
            changed |= axis_vec(ui, "Center", center, 1.0, "", None, None);
            changed |= axis_row(ui, "Radius", radius, 0.5, "", None, Some(0.001..=1.0e6));
        }
        Shape::Quad { q, u, v } => {
            changed |= axis_vec(ui, "Q", q, 1.0, "", None, None);
            changed |= axis_vec(ui, "U", u, 1.0, "", None, None);
            changed |= axis_vec(ui, "V", v, 1.0, "", None, None);
        }
        Shape::Box { a, b } => {
            changed |= axis_vec(ui, "Min", a, 1.0, "", None, None);
            changed |= axis_vec(ui, "Max", b, 1.0, "", None, None);
        }
        Shape::Mesh { .. } => {}
    }
    changed
}

fn transform_controls(ui: &mut egui::Ui, t: &mut Transform) -> bool {
    let mut changed = false;
    changed |= axis_vec(ui, "Location", &mut t.translate, 1.0, "", None, None);
    ui.add_space(4.0);
    changed |= axis_vec(ui, "Rotation", &mut t.rotate, 1.0, "°", None, Some(-360.0..=360.0));
    ui.add_space(4.0);
    changed |= axis_vec(ui, "Scale", &mut t.scale, 0.01, "", Some(3), Some(0.001..=1.0e4));
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
