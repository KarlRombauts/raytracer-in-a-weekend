//! egui widgets that edit a `Scene` in place. Each returns whether the value
//! changed, so the caller can invalidate the render only when something edited.

use eframe::egui;

use super::{icons, theme, texture_library, widgets};
use crate::color::Color;
use crate::scene::{
    self, Asset, CellTexture, Mapping, MaterialSpec, ObjectSpec, Shape, TextureSpec, Transform,
};
use crate::vec3::{Point3, Vec3};

/// A right-aligned-label colour-swatch row.
fn color_prop(ui: &mut egui::Ui, label: &str, c: &mut Color) -> bool {
    widgets::prop_row(ui, label, |ui| {
        let mut rgb = [c.x, c.y, c.z];
        let changed = ui.color_edit_button_rgb(&mut rgb).changed();
        if changed {
            *c = Color::new(rgb[0], rgb[1], rgb[2]);
        }
        changed
    })
}

/// A Phosphor type icon for the object list.
pub(crate) fn shape_icon(s: &Shape) -> &'static str {
    match s {
        Shape::Sphere { .. } => icons::SPHERE,
        Shape::Quad { .. } => icons::RECTANGLE,
        Shape::Box { .. } => icons::CUBE,
        Shape::Mesh { .. } => icons::POLYGON,
    }
}

/// Base colour shared across material types (used to carry it over on a type
/// switch). Emission returns its normalised hue.
fn shared_color(m: &MaterialSpec) -> Color {
    match m {
        MaterialSpec::Lambertian { albedo } => albedo.preview_color(),
        MaterialSpec::Glossy { albedo, .. } => albedo.preview_color(),
        MaterialSpec::Metal { albedo, .. } => *albedo,
        MaterialSpec::Dielectric { tint, .. } => *tint,
        MaterialSpec::DiffuseLight { emit } => {
            let e = emit.preview_color();
            e / e.x.max(e.y).max(e.z).max(1e-4)
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

pub(crate) fn material_controls(ui: &mut egui::Ui, m: &mut MaterialSpec) -> bool {
    let mut changed = false;

    // Natural, Blender-ish names for the shader types.
    let current = match m {
        MaterialSpec::Lambertian { .. } => "Diffuse",
        MaterialSpec::Glossy { .. } => "Glossy",
        MaterialSpec::Metal { .. } => "Metal",
        MaterialSpec::Dielectric { .. } => "Glass",
        MaterialSpec::DiffuseLight { .. } => "Emission",
    };

    changed |= widgets::prop_row(ui, "Surface", |ui| {
        let mut c = false;
        egui::ComboBox::from_id_salt("surface")
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                menu_item_style(ui);
                // Each builder receives the current shared colour/roughness so
                // switching type keeps those values instead of resetting them.
                c |= pick(
                    ui,
                    m,
                    matches!(m, MaterialSpec::Lambertian { .. }),
                    "Diffuse",
                    |col, _| MaterialSpec::Lambertian {
                        albedo: TextureSpec::solid(col),
                    },
                );
                c |= pick(
                    ui,
                    m,
                    matches!(m, MaterialSpec::Glossy { .. }),
                    "Glossy",
                    |col, r| MaterialSpec::Glossy {
                        albedo: TextureSpec::solid(col),
                        roughness: r,
                    },
                );
                c |= pick(
                    ui,
                    m,
                    matches!(m, MaterialSpec::Metal { .. }),
                    "Metal",
                    |col, r| MaterialSpec::Metal {
                        albedo: col,
                        fuzz: r,
                    },
                );
                c |= pick(
                    ui,
                    m,
                    matches!(m, MaterialSpec::Dielectric { .. }),
                    "Glass",
                    |col, r| MaterialSpec::Dielectric {
                        ior: 1.5,
                        tint: col,
                        roughness: r,
                    },
                );
                c |= pick(
                    ui,
                    m,
                    matches!(m, MaterialSpec::DiffuseLight { .. }),
                    "Emission",
                    |col, _| MaterialSpec::DiffuseLight {
                        emit: TextureSpec::solid(col * 5.0),
                    },
                );
            });
        c
    });

    match m {
        MaterialSpec::Lambertian { albedo } => changed |= texture_controls(ui, albedo),
        MaterialSpec::Glossy { albedo, roughness } => {
            changed |= texture_controls(ui, albedo);
            changed |= widgets::prop_row(ui, "Roughness", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, roughness, 0.01, Some(3), "", Some(0.0..=1.0))
            });
        }
        MaterialSpec::Metal { albedo, fuzz } => {
            changed |= color_prop(ui, "Color", albedo);
            changed |= widgets::prop_row(ui, "Roughness", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, fuzz, 0.01, Some(3), "", Some(0.0..=1.0))
            });
        }
        MaterialSpec::Dielectric {
            ior,
            tint,
            roughness,
        } => {
            changed |= color_prop(ui, "Color", tint);
            changed |= widgets::prop_row(ui, "Roughness", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, roughness, 0.01, Some(3), "", Some(0.0..=1.0))
            });
            changed |= widgets::prop_row(ui, "IOR", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, ior, 0.01, Some(3), "", Some(1.0..=3.0))
            });
        }
        MaterialSpec::DiffuseLight { emit } => {
            let e = emit.preview_color();
            let intensity = e.x.max(e.y).max(e.z).max(1e-4);
            let mut rgb = [e.x / intensity, e.y / intensity, e.z / intensity];
            let mut strength = intensity;
            let col = widgets::prop_row(ui, "Color", |ui| {
                ui.color_edit_button_rgb(&mut rgb).changed()
            });
            let str_changed = widgets::prop_row(ui, "Strength", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, &mut strength, 0.1, Some(2), "", Some(0.0..=10_000.0))
            });
            if col || str_changed {
                *emit = TextureSpec::solid(Color::new(
                    rgb[0] * strength,
                    rgb[1] * strength,
                    rgb[2] * strength,
                ));
                changed = true;
            }
        }
    }
    changed
}

/// Full texture editor: a type dropdown + per-type parameters. Used for the
/// albedo of Diffuse/Glossy materials.
pub(crate) fn texture_controls(ui: &mut egui::Ui, t: &mut TextureSpec) -> bool {
    let mut changed = false;
    let current = match t {
        TextureSpec::Solid { .. } => "Color",
        TextureSpec::Checker { .. } => "Checker",
        TextureSpec::Noise { .. } => "Noise",
        TextureSpec::Image { .. } => "Image",
    };

    changed |= widgets::prop_row(ui, "Texture", |ui| {
        let mut c = false;
        egui::ComboBox::from_id_salt("texture_type")
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                menu_item_style(ui);
                let prev = t.preview_color();
                if ui
                    .selectable_label(matches!(t, TextureSpec::Solid { .. }), "Color")
                    .clicked()
                {
                    *t = TextureSpec::Solid { color: prev };
                    c = true;
                }
                if ui
                    .selectable_label(matches!(t, TextureSpec::Checker { .. }), "Checker")
                    .clicked()
                {
                    *t = TextureSpec::Checker {
                        scale: 1.0,
                        even: CellTexture::Solid { color: prev },
                        odd: CellTexture::Solid {
                            color: Color::new(1.0, 1.0, 1.0),
                        },
                    };
                    c = true;
                }
                if ui
                    .selectable_label(matches!(t, TextureSpec::Noise { .. }), "Noise")
                    .clicked()
                {
                    *t = TextureSpec::Noise {
                        scale: 4.0,
                        depth: 7,
                    };
                    c = true;
                }
                if ui
                    .selectable_label(matches!(t, TextureSpec::Image { .. }), "Image")
                    .clicked()
                {
                    *t = TextureSpec::Image {
                        asset: Asset::empty(),
                        mapping: Mapping::default(),
                    };
                    c = true;
                }
            });
        c
    });

    match t {
        TextureSpec::Solid { color } => changed |= color_prop(ui, "Color", color),
        TextureSpec::Checker { scale, even, odd } => {
            changed |= widgets::prop_row(ui, "Scale", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, scale, 0.01, Some(3), "", Some(0.01..=100.0))
            });
            changed |= cell_texture_controls(ui, "checker_even", even);
            changed |= cell_texture_controls(ui, "checker_odd", odd);
        }
        TextureSpec::Noise { scale, depth } => {
            changed |= widgets::prop_row(ui, "Scale", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, scale, 0.01, Some(3), "", Some(0.01..=100.0))
            });
            let mut d = *depth as f32;
            if widgets::prop_row(ui, "Detail", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, &mut d, 1.0, Some(0), "", Some(1.0..=10.0))
            }) {
                *depth = d.round().clamp(1.0, 10.0) as u32;
                changed = true;
            }
        }
        TextureSpec::Image { asset, mapping } => {
            changed |= image_texture_card(ui, asset, mapping);
        }
    }
    changed
}

/// Editor for one checker cell (Solid / Noise / Image — no nested checker).
fn cell_texture_controls(ui: &mut egui::Ui, id: &str, t: &mut CellTexture) -> bool {
    let mut changed = false;
    let current = match t {
        CellTexture::Solid { .. } => "Color",
        CellTexture::Noise { .. } => "Noise",
        CellTexture::Image { .. } => "Image",
    };

    changed |= widgets::prop_row(ui, "Cell", |ui| {
        let mut c = false;
        egui::ComboBox::from_id_salt(id)
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                menu_item_style(ui);
                if ui
                    .selectable_label(matches!(t, CellTexture::Solid { .. }), "Color")
                    .clicked()
                {
                    *t = CellTexture::Solid {
                        color: Color::new(0.0, 0.0, 0.0),
                    };
                    c = true;
                }
                if ui
                    .selectable_label(matches!(t, CellTexture::Noise { .. }), "Noise")
                    .clicked()
                {
                    *t = CellTexture::Noise {
                        scale: 4.0,
                        depth: 7,
                    };
                    c = true;
                }
                if ui
                    .selectable_label(matches!(t, CellTexture::Image { .. }), "Image")
                    .clicked()
                {
                    *t = CellTexture::Image {
                        asset: Asset::empty(),
                    };
                    c = true;
                }
            });
        c
    });

    match t {
        CellTexture::Solid { color } => changed |= color_prop(ui, "Color", color),
        CellTexture::Noise { scale, depth } => {
            changed |= widgets::prop_row(ui, "Scale", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, scale, 0.01, Some(3), "", Some(0.01..=100.0))
            });
            let mut d = *depth as f32;
            if widgets::prop_row(ui, "Detail", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, &mut d, 1.0, Some(0), "", Some(1.0..=10.0))
            }) {
                *depth = d.round().clamp(1.0, 10.0) as u32;
                changed = true;
            }
        }
        CellTexture::Image { asset } => changed |= image_picker_row(ui, asset),
    }
    changed
}

/// A row showing the current image label and a button that opens a native file
/// dialog, reading the chosen file's bytes straight into the embedded `Asset`.
/// Used by `cell_texture_controls` for the checker cell Image variant.
fn image_picker_row(ui: &mut egui::Ui, asset: &mut Asset) -> bool {
    let mut changed = false;
    widgets::prop_row(ui, "Image", |ui| {
        changed |= image_load_button(ui, asset);
        let label = asset.label.clone().unwrap_or_else(|| "(none)".to_string());
        // Truncate long filenames so they don't overflow and push the panel wider.
        ui.add(egui::Label::new(label).truncate());
    });
    changed
}

/// "Load image…" button: opens a native file picker (native-only; wasm shows a
/// disabled button). Returns true if a new file was loaded into `asset`.
/// Factored out so both `image_picker_row` (cell textures) and
/// `image_texture_card` (full texture) share the rfd logic without duplication.
fn image_load_button(ui: &mut egui::Ui, asset: &mut Asset) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if ui
            .button(format!("{} Load image\u{2026}", icons::FOLDER))
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg"])
                .pick_file()
            {
                if let Ok(bytes) = std::fs::read(&path) {
                    asset.bytes = bytes.into();
                    asset.label = path.file_name().map(|s| s.to_string_lossy().into_owned());
                    return true;
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ui
            .add_enabled(false, egui::Button::new("Load image\u{2026}"))
            .on_disabled_hover_text("Image import isn't available in the browser yet");
        let _ = asset;
    }
    false
}

/// Card-style editor for `TextureSpec::Image`. Shows a thumbnail, filename,
/// load button, mapping controls, and a texture library preset grid.
fn image_texture_card(
    ui: &mut egui::Ui,
    asset: &mut Asset,
    mapping: &mut crate::scene::Mapping,
) -> bool {
    use crate::texture::Projection;

    let mut changed = false;

    egui::Frame::NONE
        .fill(theme::FIELD_BG)
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            // ---- Top row: 58×58 thumbnail + filename + load button ----
            ui.horizontal(|ui| {
                let thumb_size = egui::vec2(58.0, 58.0);
                let thumb_key = asset.label.as_deref().unwrap_or("__current__");
                let have_image = !asset.bytes.is_empty();
                let thumb_handle = if have_image {
                    texture_library::texture_for(ui.ctx(), thumb_key, &asset.bytes)
                } else {
                    None
                };

                // Reserve the thumbnail area
                let (thumb_rect, _) =
                    ui.allocate_exact_size(thumb_size, egui::Sense::hover());
                if let Some(ref handle) = thumb_handle {
                    // Paint the image directly
                    ui.painter().image(
                        handle.id(),
                        thumb_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    // Border on top
                    ui.painter().rect_stroke(
                        thumb_rect,
                        egui::CornerRadius::same(6),
                        egui::Stroke::new(1.0, theme::BORDER_FIELD),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    // Placeholder: subtle checker + IMAGE icon
                    let painter = ui.painter_at(thumb_rect);
                    painter.rect_filled(
                        thumb_rect,
                        egui::CornerRadius::same(6),
                        theme::FIELD_BG,
                    );
                    let cell = 8.0f32;
                    let cols_n = (thumb_rect.width() / cell).ceil() as i32;
                    let rows_n = (thumb_rect.height() / cell).ceil() as i32;
                    for row in 0..rows_n {
                        for col in 0..cols_n {
                            if (row + col) % 2 == 0 {
                                let r = egui::Rect::from_min_size(
                                    egui::pos2(
                                        thumb_rect.left() + col as f32 * cell,
                                        thumb_rect.top() + row as f32 * cell,
                                    ),
                                    egui::vec2(cell, cell),
                                )
                                .intersect(thumb_rect);
                                painter.rect_filled(
                                    r,
                                    egui::CornerRadius::ZERO,
                                    egui::Color32::from_rgb(0x18, 0x19, 0x1e),
                                );
                            }
                        }
                    }
                    painter.rect_stroke(
                        thumb_rect,
                        egui::CornerRadius::same(6),
                        egui::Stroke::new(1.0, theme::BORDER_FIELD),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        thumb_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        icons::IMAGE,
                        egui::FontId::proportional(22.0),
                        theme::TEXT_DIM,
                    );
                }

                // Right column: filename + load button
                ui.vertical(|ui| {
                    let name = asset.label.as_deref().unwrap_or("No image");
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(name)
                                .color(theme::TEXT)
                                .family(theme::semibold()),
                        )
                        .truncate(),
                    );
                    ui.add_space(4.0);
                    changed |= image_load_button(ui, asset);
                });
            });

            ui.add_space(8.0);

            // ---- Mapping ----
            let proj_label = match mapping.projection {
                Projection::MeshUv => "Mesh UV",
                Projection::Planar => "Planar",
                Projection::Spherical => "Spherical",
                Projection::Cylindrical => "Cylindrical",
            };
            widgets::prop_row(ui, "Mapping", |ui| {
                egui::ComboBox::from_id_salt("texture_projection")
                    .selected_text(proj_label)
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        menu_item_style(ui);
                        for (p, label) in [
                            (Projection::MeshUv, "Mesh UV"),
                            (Projection::Planar, "Planar"),
                            (Projection::Spherical, "Spherical"),
                            (Projection::Cylindrical, "Cylindrical"),
                        ] {
                            if ui
                                .selectable_label(mapping.projection == p, label)
                                .clicked()
                            {
                                mapping.projection = p;
                                changed = true;
                            }
                        }
                    });
            });
            changed |= widgets::prop_row(ui, "Scale", |ui| {
                widgets::axis_field(
                    ui,
                    widgets::Axis::None,
                    &mut mapping.scale,
                    0.01,
                    Some(3),
                    "",
                    Some(0.01..=100.0),
                )
            });

            let mut offset_u = mapping.offset.0;
            let mut offset_v = mapping.offset.1;
            if widgets::prop_row(ui, "Offset U", |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("U")
                            .monospace()
                            .color(theme::AXIS_X)
                            .size(11.0),
                    );
                    widgets::axis_field(
                        ui,
                        widgets::Axis::None,
                        &mut offset_u,
                        0.01,
                        Some(3),
                        "",
                        Some(-10.0..=10.0),
                    )
                })
                .inner
            }) {
                mapping.offset.0 = offset_u;
                changed = true;
            }
            if widgets::prop_row(ui, "Offset V", |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("V")
                            .monospace()
                            .color(theme::AXIS_Y)
                            .size(11.0),
                    );
                    widgets::axis_field(
                        ui,
                        widgets::Axis::None,
                        &mut offset_v,
                        0.01,
                        Some(3),
                        "",
                        Some(-10.0..=10.0),
                    )
                })
                .inner
            }) {
                mapping.offset.1 = offset_v;
                changed = true;
            }

            ui.add_space(8.0);

            // ---- Texture library swatch grid ----
            ui.label(
                egui::RichText::new("Texture library")
                    .color(theme::TEXT_MUTED)
                    .size(11.0),
            );
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let swatch_size = egui::vec2(40.0, 40.0);
                for preset in texture_library::presets() {
                    let is_selected = asset.label.as_deref() == Some(preset.name);
                    let border_color = if is_selected {
                        theme::ACCENT
                    } else {
                        theme::BORDER_FIELD
                    };

                    let handle =
                        texture_library::texture_for(ui.ctx(), preset.name, &preset.bytes);

                    let response = if let Some(ref tex) = handle {
                        ui.add(
                            egui::Button::image(egui::load::SizedTexture::new(
                                tex.id(),
                                swatch_size,
                            ))
                            .corner_radius(egui::CornerRadius::same(5)),
                        )
                    } else {
                        ui.add_sized(swatch_size, egui::Button::new(""))
                    };

                    // Accent border for selected preset, normal border otherwise
                    let stroke_w = if is_selected { 2.0 } else { 1.0 };
                    ui.painter().rect_stroke(
                        response.rect,
                        egui::CornerRadius::same(5),
                        egui::Stroke::new(stroke_w, border_color),
                        egui::StrokeKind::Inside,
                    );

                    if response.clicked() {
                        *asset = Asset {
                            bytes: preset.bytes.clone(),
                            label: Some(preset.name.to_string()),
                        };
                        changed = true;
                    }

                    response.on_hover_text(preset.name);
                }
            });
        });

    changed
}

/// Restyle a ComboBox popup so its `selectable_label` rows read like the clean
/// Add-object menu: a borderless subtle hover fill instead of the BORDER_HOVER
/// box, and a borderless accent-soft fill on the selected row instead of the
/// heavy blue-bordered box the global widget visuals would draw.
fn menu_item_style(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(0x25, 0x28, 0x2e);
    v.selection.bg_fill = theme::accent_soft();
    v.selection.stroke = egui::Stroke::NONE;
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

pub(crate) fn shape_controls(ui: &mut egui::Ui, s: &mut Shape) -> bool {
    let mut changed = false;
    match s {
        Shape::Sphere { center, radius } => {
            widgets::sub_label(ui, "Center");
            changed |= widgets::axis_vec(ui, center, 1.0, "", None, None);
            changed |= widgets::prop_row(ui, "Radius", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, radius, 0.5, None, "", Some(0.001..=1.0e6))
            });
        }
        Shape::Quad { q, u, v } => {
            widgets::sub_label(ui, "Q");
            changed |= widgets::axis_vec(ui, q, 1.0, "", None, None);
            widgets::sub_label(ui, "U");
            changed |= widgets::axis_vec(ui, u, 1.0, "", None, None);
            widgets::sub_label(ui, "V");
            changed |= widgets::axis_vec(ui, v, 1.0, "", None, None);
        }
        Shape::Box { a, b } => {
            widgets::sub_label(ui, "Min");
            changed |= widgets::axis_vec(ui, a, 1.0, "", None, None);
            widgets::sub_label(ui, "Max");
            changed |= widgets::axis_vec(ui, b, 1.0, "", None, None);
        }
        Shape::Mesh { .. } => {}
    }
    changed
}

pub(crate) fn transform_controls(ui: &mut egui::Ui, t: &mut Transform) -> bool {
    let mut changed = false;
    widgets::sub_label(ui, "Location");
    changed |= widgets::axis_vec(ui, &mut t.translate, 1.0, "", None, None);
    widgets::sub_label(ui, "Rotation");
    changed |= widgets::axis_vec(ui, &mut t.rotate, 1.0, "°", None, Some(-360.0..=360.0));
    widgets::sub_label(ui, "Scale");
    changed |= widgets::axis_vec(ui, &mut t.scale, 0.01, "", Some(3), Some(0.001..=1.0e4));
    changed
}

pub(crate) fn default_sphere(n: usize) -> ObjectSpec {
    ObjectSpec {
        name: format!("Sphere {}", n),
        shape: Shape::Sphere {
            center: Point3::new(278.0, 120.0, 278.0),
            radius: 80.0,
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.8, 0.3, 0.3)),
        },
        transform: Transform::identity(),
        hidden: false,
    }
}

pub(crate) fn default_box(n: usize) -> ObjectSpec {
    ObjectSpec {
        name: format!("Box {}", n),
        shape: Shape::Box {
            a: Point3::new(200.0, 0.0, 200.0),
            b: Point3::new(360.0, 160.0, 360.0),
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.7, 0.7, 0.7)),
        },
        transform: Transform::identity(),
        hidden: false,
    }
}

/// A flat quad (floor-aligned, 200×200 units centered at scene origin).
pub(crate) fn default_plane(n: usize) -> ObjectSpec {
    ObjectSpec {
        name: format!("Plane {}", n),
        shape: Shape::Quad {
            q: Point3::new(178.0, 0.0, 178.0),
            u: Vec3::new(200.0, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, 200.0),
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.7, 0.7, 0.7)),
        },
        transform: Transform::identity(),
        hidden: false,
    }
}

/// Show an "Import .obj" button and, when clicked, open a file picker and load
/// the chosen mesh. Returns `true` if a new object was successfully added.
///
/// On wasm the button is shown disabled with an explanatory tooltip.
pub(crate) fn import_obj(
    ui: &mut egui::Ui,
    objects: &mut Vec<ObjectSpec>,
    selected: &mut Option<usize>,
) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if ui
        .button(format!("{}  Import .obj\u{2026}", icons::FOLDER))
        .clicked()
    {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Wavefront OBJ", &["obj"])
            .pick_file()
        {
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
                return true;
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ui
            .add_enabled(
                false,
                egui::Button::new(format!("{}  Import .obj\u{2026}", icons::FOLDER)),
            )
            .on_disabled_hover_text("OBJ import isn't available in the browser yet");
        let _ = objects;
        let _ = selected;
    }
    false
}
