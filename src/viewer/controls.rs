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

/// Shared editor for a procedural noise texture: pattern style, scale, the two
/// blend colours, and octave detail. Used by both the full texture editor and a
/// checker cell. `id` keeps the style ComboBox unique between the two.
fn noise_controls(
    ui: &mut egui::Ui,
    id: &str,
    scale: &mut f32,
    depth: &mut u32,
    style: &mut crate::texture::NoiseStyle,
    light: &mut Color,
    dark: &mut Color,
) -> bool {
    let mut changed = false;
    changed |= widgets::prop_row(ui, "Style", |ui| {
        let mut c = false;
        egui::ComboBox::from_id_salt(format!("{id}_noise_style"))
            .selected_text(style.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                menu_item_style(ui);
                for s in crate::texture::NoiseStyle::ALL {
                    if ui.selectable_label(*style == s, s.label()).clicked() {
                        *style = s;
                        c = true;
                    }
                }
            });
        c
    });
    changed |= widgets::prop_row(ui, "Scale", |ui| {
        widgets::axis_field(ui, widgets::Axis::None, scale, 0.01, Some(3), "", Some(0.01..=100.0))
    });
    changed |= color_prop(ui, "Light", light);
    changed |= color_prop(ui, "Dark", dark);
    let mut d = *depth as f32;
    if widgets::prop_row(ui, "Detail", |ui| {
        widgets::axis_field(ui, widgets::Axis::None, &mut d, 1.0, Some(0), "", Some(1.0..=10.0))
    }) {
        *depth = d.round().clamp(1.0, 10.0) as u32;
        changed = true;
    }
    changed
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
        TextureSpec::Noise { .. } | TextureSpec::NoiseLegacy { .. } => "Noise",
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
                    .selectable_label(
                        matches!(t, TextureSpec::Noise { .. } | TextureSpec::NoiseLegacy { .. }),
                        "Noise",
                    )
                    .clicked()
                {
                    *t = TextureSpec::Noise {
                        scale: 4.0,
                        depth: 7,
                        style: crate::texture::NoiseStyle::Turbulence,
                        // Carry the current colour as the light tone; dark to black.
                        light: prev,
                        dark: Color::new(0.0, 0.0, 0.0),
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

    // Upgrade a legacy grayscale noise to the editable coloured form (an
    // identical white/black turbulence) so its colours and style become
    // editable. Silent — the look is unchanged, so no re-render is signalled.
    if let TextureSpec::NoiseLegacy { scale, depth } = t {
        let (scale, depth) = (*scale, *depth);
        *t = TextureSpec::Noise {
            scale,
            depth,
            style: crate::texture::NoiseStyle::Turbulence,
            light: Color::new(1.0, 1.0, 1.0),
            dark: Color::new(0.0, 0.0, 0.0),
        };
    }

    match t {
        TextureSpec::Solid { color } => changed |= color_prop(ui, "Color", color),
        TextureSpec::Checker { scale, even, odd } => {
            changed |= widgets::prop_row(ui, "Scale", |ui| {
                widgets::axis_field(ui, widgets::Axis::None, scale, 0.01, Some(3), "", Some(0.01..=100.0))
            });
            changed |= cell_texture_controls(ui, "checker_even", even);
            changed |= cell_texture_controls(ui, "checker_odd", odd);
        }
        TextureSpec::Noise { scale, depth, style, light, dark } => {
            changed |= noise_controls(ui, "tex", scale, depth, style, light, dark);
        }
        // Unreachable: upgraded to `Noise` just above.
        TextureSpec::NoiseLegacy { .. } => {}
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
        CellTexture::Noise { .. } | CellTexture::NoiseLegacy { .. } => "Noise",
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
                    .selectable_label(
                        matches!(t, CellTexture::Noise { .. } | CellTexture::NoiseLegacy { .. }),
                        "Noise",
                    )
                    .clicked()
                {
                    *t = CellTexture::Noise {
                        scale: 4.0,
                        depth: 7,
                        style: crate::texture::NoiseStyle::Turbulence,
                        light: Color::new(1.0, 1.0, 1.0),
                        dark: Color::new(0.0, 0.0, 0.0),
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

    // Upgrade a legacy grayscale cell noise to the editable coloured form.
    if let CellTexture::NoiseLegacy { scale, depth } = t {
        let (scale, depth) = (*scale, *depth);
        *t = CellTexture::Noise {
            scale,
            depth,
            style: crate::texture::NoiseStyle::Turbulence,
            light: Color::new(1.0, 1.0, 1.0),
            dark: Color::new(0.0, 0.0, 0.0),
        };
    }

    match t {
        CellTexture::Solid { color } => changed |= color_prop(ui, "Color", color),
        CellTexture::Noise { scale, depth, style, light, dark } => {
            changed |= noise_controls(ui, id, scale, depth, style, light, dark);
        }
        // Unreachable: upgraded to `Noise` just above.
        CellTexture::NoiseLegacy { .. } => {}
        CellTexture::Image { asset } => changed |= image_picker_row(ui, asset, id),
    }
    changed
}

/// A row showing the current image label and a button that opens a file picker,
/// reading the chosen file's bytes straight into the embedded `Asset`. Used by
/// `cell_texture_controls` for the checker cell Image variant. `salt` keeps this
/// row's picker distinct from the main texture's (and the other cell's).
fn image_picker_row(ui: &mut egui::Ui, asset: &mut Asset, salt: &str) -> bool {
    let mut changed = false;
    widgets::prop_row(ui, "Image", |ui| {
        changed |= image_load_button(ui, asset, salt);
        let label = asset.label.clone().unwrap_or_else(|| "(none)".to_string());
        // Truncate long filenames so they don't overflow and push the panel wider.
        ui.add(egui::Label::new(label).truncate());
    });
    changed
}

/// "Load image…" button and the upload it kicks off. The pick is a blocking
/// dialog on native and an async `<input type=file>` on the web; either way the
/// [`FilePicker`](crate::platform::FilePicker) is stashed in egui's per-frame
/// data under `salt` and polled here each frame, so the bytes land in `asset`
/// once the user has chosen. `salt` must be stable across frames for a given
/// asset (and distinct between assets). Returns true the frame a file lands.
fn image_load_button(ui: &mut egui::Ui, asset: &mut Asset, salt: &str) -> bool {
    use crate::platform::{FilePicker, PickStatus};
    let id = ui.make_persistent_id(("image_pick", salt));
    let mut changed = false;

    // Poll a pick started on an earlier frame (on native it already resolved).
    if let Some(picker) = ui.data(|d| d.get_temp::<FilePicker>(id)) {
        match picker.poll() {
            PickStatus::Pending => ui.ctx().request_repaint(), // keep polling
            PickStatus::Done(file) => {
                ui.data_mut(|d| d.remove::<FilePicker>(id));
                asset.bytes = file.bytes.into();
                asset.label = Some(file.name);
                changed = true;
            }
            PickStatus::Cancelled | PickStatus::Failed(_) => {
                ui.data_mut(|d| d.remove::<FilePicker>(id));
            }
        }
    }

    if ui
        .button(format!("{} Load image\u{2026}", icons::FOLDER))
        .clicked()
    {
        let picker = crate::platform::pick_file("Image", &["png", "jpg", "jpeg"]);
        ui.data_mut(|d| d.insert_temp(id, picker));
        ui.ctx().request_repaint();
    }
    changed
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
                    changed |= image_load_button(ui, asset, "texture_main");
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
        // Unit sphere (radius 1) centred at the origin.
        shape: Shape::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 1.0,
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
        // Unit cube (1×1×1) centred at the origin.
        shape: Shape::Box {
            a: Point3::new(-0.5, -0.5, -0.5),
            b: Point3::new(0.5, 0.5, 0.5),
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.7, 0.7, 0.7)),
        },
        transform: Transform::identity(),
        hidden: false,
    }
}

/// A flat unit quad (1×1, floor-aligned in XZ) centred at the origin.
pub(crate) fn default_plane(n: usize) -> ObjectSpec {
    ObjectSpec {
        name: format!("Plane {}", n),
        shape: Shape::Quad {
            q: Point3::new(-0.5, 0.0, -0.5),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, 1.0),
        },
        material: MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.7, 0.7, 0.7)),
        },
        transform: Transform::identity(),
        hidden: false,
    }
}

/// egui-data key for the in-flight OBJ import. Global (not tied to the Add-menu
/// popup, which closes on click) so [`poll_obj_import`] can pick it up from a
/// panel that stays mounted.
const OBJ_PICK_ID: &str = "pending_obj_import";

/// Where to drop a freshly imported mesh: the centre of the existing placeable
/// geometry and a size ~⅓ of its span, so an import lands visible inside the
/// scene rather than at the origin. Falls back to a 2-unit object at the origin
/// for an empty scene.
fn import_fit_target(objects: &[ObjectSpec]) -> (Vec3, f32) {
    match scene::placeable_bounds(objects) {
        Some((min, max)) => {
            let extent = max - min;
            let span = extent.x.max(extent.y).max(extent.z);
            ((min + max) * 0.5, span * 0.33)
        }
        None => (Vec3::ZERO, 2.0),
    }
}

/// The in-flight OBJ import stored in egui data. Two stages: first the bytes are
/// fetched/read (`Fetching`, with an optional display-name override — set for
/// bundled sample meshes, `None` for an uploaded file), then they're parsed and
/// the mesh BVH is built on a worker (`Building`). The build is off-thread for
/// the same reason scene decode is: `BVH::build` forks via rayon, which can't
/// run on the browser's main thread.
#[derive(Clone)]
enum ObjImport {
    Fetching(crate::platform::FilePicker, Option<String>),
    Building(crate::platform::ObjBuilder),
}

/// Outcome of [`poll_obj_import`] for the frame.
pub(crate) enum ImportStatus {
    /// Nothing importing.
    Idle,
    /// A mesh is downloading/reading or building (kept off the UI thread).
    Loading,
    /// A mesh was just added (caller should mark the scene dirty).
    Added,
}

/// Start loading a bundled sample mesh from `assets/objs/<file>` — a disk read
/// on native, an HTTP fetch on the web (Trunk copies `assets/objs` into the
/// bundle). The handle is stashed in egui data and [`poll_obj_import`] builds +
/// adds it once the bytes arrive, under the display name `label`.
pub(crate) fn add_sample_mesh(ui: &egui::Ui, label: &str, file: &str) {
    let picker = crate::platform::fetch_file(&format!("assets/objs/{file}"));
    let state = ObjImport::Fetching(picker, Some(label.to_string()));
    ui.data_mut(|d| d.insert_temp(egui::Id::new(OBJ_PICK_ID), state));
    ui.ctx().request_repaint();
}

/// Show an "Import .obj" button that starts a file pick (a blocking dialog on
/// native, an async `<input type=file>` on the web). The picker is stashed in
/// egui data; [`poll_obj_import`] — called from a panel that stays mounted —
/// builds + adds the mesh once the bytes arrive. (The Add-menu popup closes on
/// click, so it can't do the adding itself.)
pub(crate) fn import_obj(ui: &mut egui::Ui) {
    if ui
        .button(format!("{}  Import .obj\u{2026}", icons::FOLDER))
        .clicked()
    {
        let picker = crate::platform::pick_file("Wavefront OBJ", &["obj"]);
        let state = ObjImport::Fetching(picker, None);
        ui.data_mut(|d| d.insert_temp(egui::Id::new(OBJ_PICK_ID), state));
        ui.ctx().request_repaint();
    }
}

/// Poll a pending OBJ import once per frame from a panel that always renders
/// (the outliner). Drives the fetch -> worker-build -> add pipeline. Works for
/// both uploads and bundled sample meshes, native and web.
pub(crate) fn poll_obj_import(
    ui: &egui::Ui,
    objects: &mut Vec<ObjectSpec>,
    selected: &mut super::state::Selection,
) -> ImportStatus {
    use crate::platform::PickStatus;
    let id = egui::Id::new(OBJ_PICK_ID);
    let Some(state) = ui.data(|d| d.get_temp::<ObjImport>(id)) else {
        return ImportStatus::Idle;
    };
    match state {
        ObjImport::Fetching(picker, name_override) => match picker.poll() {
            PickStatus::Pending => {
                ui.ctx().request_repaint(); // keep polling until the fetch resolves
                ImportStatus::Loading
            }
            PickStatus::Done(file) => {
                // Hand the parse + BVH build to a worker (mustn't block the UI
                // thread, especially in the browser). Fit is computed here, cheaply.
                let (center, size) = import_fit_target(objects);
                let name = name_override.unwrap_or_else(|| {
                    std::path::Path::new(&file.name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("mesh")
                        .to_string()
                });
                let builder = crate::platform::build_obj(name, file.bytes, center, size);
                ui.data_mut(|d| d.insert_temp(id, ObjImport::Building(builder)));
                ui.ctx().request_repaint();
                ImportStatus::Loading
            }
            PickStatus::Cancelled | PickStatus::Failed(_) => {
                ui.data_mut(|d| d.remove::<ObjImport>(id));
                ImportStatus::Idle
            }
        },
        ObjImport::Building(builder) => match builder.poll() {
            None => {
                ui.ctx().request_repaint(); // keep polling until the build finishes
                ImportStatus::Loading
            }
            Some(obj) => {
                ui.data_mut(|d| d.remove::<ObjImport>(id));
                objects.push(obj);
                selected.set(objects.len() - 1);
                ImportStatus::Added
            }
        },
    }
}
