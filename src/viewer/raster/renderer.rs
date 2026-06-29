//! A minimal flat-shaded OpenGL renderer for the scene preview, driven through
//! eframe's glow context inside an egui paint callback.

use glow::HasContext;

use crate::scene::{MaterialSpec, Scene};
use super::camera_gl;

const AMBIENT: f32 = 0.25;

/// Solid colour of the selection outline (bright amber, reads clearly against
/// the muted preview shading).
const OUTLINE_COLOR: [f32; 3] = [1.0, 0.55, 0.1];
/// Outline half-thickness in physical pixels — the dilation radius. Constant in
/// screen space, so the border is the same width on every side regardless of
/// the object's shape, distance, or perspective.
const OUTLINE_RADIUS_PX: i32 = 3;
/// Opacity of the outline where it is occluded by nearer geometry, vs. where it
/// is unobstructed. The faint pass keeps the selection legible behind objects.
const OUTLINE_ALPHA_FULL: f32 = 1.0;
const OUTLINE_ALPHA_FAINT: f32 = 0.28;

/// Faint, feathered dark outline drawn around *every* object — wherever the
/// object ID changes between neighbouring pixels (silhouette, object-vs-object,
/// and contact lines), like Blender's solid view. All tunable here.
const EDGE_COLOR: [f32; 3] = [0.02, 0.02, 0.03];
/// Peak opacity of the edge is roughly half this (a boundary is ~50% "other
/// object" within the disc), so 0.25 → ~0.12 alpha at its darkest.
const EDGE_STRENGTH: f32 = 0.25;
/// Feather radius in pixels: the soft band half-width around each ID boundary.
const EDGE_RADIUS_PX: f32 = 3.0;

struct ObjectBuffers {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    vertex_count: i32,
    center: glam::Vec3,
}

/// Offscreen render targets for the outline passes, sized to the preview
/// viewport in physical pixels and recreated when that size changes.
struct OutlineTargets {
    w: i32,
    h: i32,
    /// Depth-only FBO the selected object's silhouette is rendered into; its
    /// depth texture doubles as the silhouette mask (texel < 1 ⇒ covered).
    mask_fbo: glow::Framebuffer,
    mask_depth: glow::Texture,
    /// FBO the per-object IDs are rendered into, with a colour texture (the ID,
    /// for the edge pass) and a depth texture (the whole-scene depth, single-
    /// sample, used by the selection outline's occlusion test). Rendering the
    /// scene depth here avoids blitting it out of the multisampled window
    /// framebuffer — which GL forbids.
    id_fbo: glow::Framebuffer,
    id_tex: glow::Texture,
    id_depth: glow::Texture,
}

pub struct SceneRenderer {
    program: glow::Program,
    /// Fullscreen pass that dilates the silhouette mask into the outline and
    /// fades it against the scene depth.
    composite: glow::Program,
    /// Fullscreen pass that draws the faint dark depth-discontinuity edges.
    edge: glow::Program,
    /// Attribute-less VAO for the fullscreen triangle (positions come from
    /// gl_VertexID); core profiles require a bound VAO to draw.
    empty_vao: glow::VertexArray,
    targets: Option<OutlineTargets>,
    objects: Vec<ObjectBuffers>,
    built_count: usize, // object count the buffers were built for
    /// World-space AABB of the whole scene, used to pick near/far planes that
    /// don't clip the scene. Recomputed on rebuild.
    bounds: Option<(glam::Vec3, glam::Vec3)>,
}

impl SceneRenderer {
    pub fn new(gl: &glow::Context) -> Self {
        let shader_version = if cfg!(target_arch = "wasm32") {
            // GLES3 fragment shaders have no default int precision; the outline
            // shader uses ints, so set one explicitly.
            "#version 300 es\nprecision highp float;\nprecision highp int;\n"
        } else {
            "#version 330\n"
        };
        let vert = format!("{shader_version}{VERT}");
        let frag = format!("{shader_version}{FRAG}");
        let program = unsafe { link_program(gl, &vert, &frag) };
        let composite = unsafe {
            link_program(
                gl,
                &format!("{shader_version}{OUTLINE_VERT}"),
                &format!("{shader_version}{OUTLINE_FRAG}"),
            )
        };
        let edge = unsafe {
            link_program(
                gl,
                &format!("{shader_version}{OUTLINE_VERT}"),
                &format!("{shader_version}{EDGE_FRAG}"),
            )
        };
        let empty_vao = unsafe { gl.create_vertex_array().unwrap() };
        SceneRenderer {
            program,
            composite,
            edge,
            empty_vao,
            targets: None,
            objects: Vec::new(),
            built_count: usize::MAX,
            bounds: None,
        }
    }

    fn rebuild(&mut self, gl: &glow::Context, scene: &Scene) {
        unsafe {
            for o in self.objects.drain(..) {
                gl.delete_vertex_array(o.vao);
                gl.delete_buffer(o.vbo);
            }
            let mut wmin = glam::Vec3::splat(f32::INFINITY);
            let mut wmax = glam::Vec3::splat(f32::NEG_INFINITY);
            for obj in &scene.objects {
                let mesh = obj.shape.render_mesh();
                // Local-space AABB of this object's geometry.
                let mut lmin = glam::Vec3::splat(f32::INFINITY);
                let mut lmax = glam::Vec3::splat(f32::NEG_INFINITY);
                for p in &mesh.positions {
                    let v = glam::Vec3::new(p[0], p[1], p[2]);
                    lmin = lmin.min(v);
                    lmax = lmax.max(v);
                }
                // Pivot centre (cached so paint() never re-tessellates).
                let center = if mesh.positions.is_empty() {
                    glam::Vec3::ZERO
                } else {
                    (lmin + lmax) * 0.5
                };
                // Accumulate the scene's world-space bounds by transforming this
                // object's 8 AABB corners through its model matrix.
                if !mesh.positions.is_empty() {
                    let model = camera_gl::model_matrix(&obj.transform, center);
                    for i in 0..8u32 {
                        let corner = glam::Vec3::new(
                            if i & 1 == 0 { lmin.x } else { lmax.x },
                            if i & 2 == 0 { lmin.y } else { lmax.y },
                            if i & 4 == 0 { lmin.z } else { lmax.z },
                        );
                        let wc = model.transform_point3(corner);
                        wmin = wmin.min(wc);
                        wmax = wmax.max(wc);
                    }
                }
                // Interleave position+normal as 6 f32 per vertex.
                let mut data: Vec<f32> = Vec::with_capacity(mesh.positions.len() * 6);
                for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
                    data.extend_from_slice(p);
                    data.extend_from_slice(n);
                }
                let vao = gl.create_vertex_array().unwrap();
                let vbo = gl.create_buffer().unwrap();
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&data),
                    glow::STATIC_DRAW,
                );
                let stride = 6 * std::mem::size_of::<f32>() as i32;
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
                gl.enable_vertex_attrib_array(1);
                gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 3 * 4);
                self.objects.push(ObjectBuffers { vao, vbo, vertex_count: mesh.positions.len() as i32, center });
            }
            self.built_count = scene.objects.len();
            self.bounds = if wmin.x.is_finite() {
                Some((wmin, wmax))
            } else {
                None
            };
        }
    }

    /// Near/far planes that bracket the whole scene from the current eye, so
    /// large scenes (e.g. the Cornell box) aren't clipped and small ones keep
    /// depth precision. Falls back to a fixed range when bounds are unknown.
    fn near_far(&self, eye: glam::Vec3) -> (f32, f32) {
        let Some((bmin, bmax)) = self.bounds else {
            return (0.05, 1000.0);
        };
        let mut dmin = f32::INFINITY;
        let mut dmax = 0.0f32;
        for i in 0..8u32 {
            let c = glam::Vec3::new(
                if i & 1 == 0 { bmin.x } else { bmax.x },
                if i & 2 == 0 { bmin.y } else { bmax.y },
                if i & 4 == 0 { bmin.z } else { bmax.z },
            );
            let d = (c - eye).length();
            dmin = dmin.min(d);
            dmax = dmax.max(d);
        }
        let near = (dmin * 0.5).max(0.01);
        let far = (dmax * 1.5).max(near + 0.1);
        (near, far)
    }

    pub fn paint(
        &mut self,
        gl: &glow::Context,
        scene: &Scene,
        info: &eframe::egui::PaintCallbackInfo,
        selected: Option<usize>,
    ) {
        if self.built_count != scene.objects.len() {
            self.rebuild(gl, scene);
        }
        let vp = info.viewport_in_pixels();
        let view = camera_gl::view_matrix(&scene.camera);
        // Aspect from the actual viewport so the preview isn't stretched.
        let aspect = if vp.height_px > 0 {
            vp.width_px as f32 / vp.height_px as f32
        } else {
            1.0
        };
        let eye = glam::Vec3::new(
            scene.camera.look_from.x,
            scene.camera.look_from.y,
            scene.camera.look_from.z,
        );
        let (near, far) = self.near_far(eye);
        let proj = camera_gl::projection_matrix(&scene.camera, aspect, near, far);
        unsafe {
            gl.enable(glow::DEPTH_TEST);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.use_program(Some(self.program));
            uniform_mat4(gl, self.program, "u_view", &view);
            uniform_mat4(gl, self.program, "u_proj", &proj);
            gl.uniform_1_f32(
                gl.get_uniform_location(self.program, "u_ambient").as_ref(),
                AMBIENT,
            );
            for (obj, buf) in scene.objects.iter().zip(&self.objects) {
                let model = camera_gl::model_matrix(&obj.transform, buf.center);
                uniform_mat4(gl, self.program, "u_model", &model);
                let (color, emissive) = preview_color(&obj.material);
                gl.uniform_3_f32(
                    gl.get_uniform_location(self.program, "u_color").as_ref(),
                    color[0], color[1], color[2],
                );
                gl.uniform_1_i32(
                    gl.get_uniform_location(self.program, "u_emissive").as_ref(),
                    emissive as i32,
                );
                gl.bind_vertex_array(Some(buf.vao));
                gl.draw_arrays(glow::TRIANGLES, 0, buf.vertex_count);
            }
            gl.disable(glow::DEPTH_TEST);

            // Screen-space post effects: a faint feathered edge around every
            // object (from an ID pass), then the selection outline on top.
            let (w, h) = (vp.width_px, vp.height_px);
            if !scene.objects.is_empty() && w > 0 && h > 0 && self.ensure_targets(gl, w, h) {
                self.id_pass(gl, scene, &view, &proj, &vp);
                self.edge_pass(gl, w, h);
                if let Some(i) = selected {
                    if i < self.objects.len() {
                        let model = camera_gl::model_matrix(
                            &scene.objects[i].transform,
                            self.objects[i].center,
                        );
                        // The whole-scene depth is the ID pass's depth texture.
                        self.draw_outline(gl, &view, &proj, &model, i, &vp);
                    }
                }
            }
        }
    }

    /// Render each object's ID (1-based, background = 0) into the ID target,
    /// depth-tested so the front-most object wins. Reuses the scene program in
    /// its emissive (flat-colour) path. Restores the default framebuffer.
    unsafe fn id_pass(
        &self,
        gl: &glow::Context,
        scene: &Scene,
        view: &glam::Mat4,
        proj: &glam::Mat4,
        vp: &eframe::egui::epaint::ViewportInPixels,
    ) {
        let t = self.targets.as_ref().unwrap();
        let (w, h) = (t.w, t.h);
        let scissor_was_on = gl.is_enabled(glow::SCISSOR_TEST);
        gl.disable(glow::SCISSOR_TEST);
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(t.id_fbo));
        gl.viewport(0, 0, w, h);
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.use_program(Some(self.program));
        uniform_mat4(gl, self.program, "u_view", view);
        uniform_mat4(gl, self.program, "u_proj", proj);
        gl.uniform_1_i32(gl.get_uniform_location(self.program, "u_emissive").as_ref(), 1);
        for (idx, (obj, buf)) in scene.objects.iter().zip(&self.objects).enumerate() {
            let model = camera_gl::model_matrix(&obj.transform, buf.center);
            uniform_mat4(gl, self.program, "u_model", &model);
            let id = (idx + 1) as f32 / 255.0;
            gl.uniform_3_f32(gl.get_uniform_location(self.program, "u_color").as_ref(), id, id, id);
            gl.bind_vertex_array(Some(buf.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, buf.vertex_count);
        }
        gl.disable(glow::DEPTH_TEST);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.viewport(vp.left_px, vp.from_bottom_px, w, h);
        if scissor_was_on {
            gl.enable(glow::SCISSOR_TEST);
        }
    }

    /// Fullscreen pass: a faint, feathered dark line wherever the object ID
    /// changes between neighbouring pixels.
    unsafe fn edge_pass(&self, gl: &glow::Context, w: i32, h: i32) {
        let t = self.targets.as_ref().unwrap();
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        gl.use_program(Some(self.edge));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(t.id_tex));
        gl.uniform_1_i32(gl.get_uniform_location(self.edge, "u_id").as_ref(), 0);
        gl.uniform_2_f32(
            gl.get_uniform_location(self.edge, "u_texel").as_ref(),
            1.0 / w as f32,
            1.0 / h as f32,
        );
        gl.uniform_1_f32(gl.get_uniform_location(self.edge, "u_radius").as_ref(), EDGE_RADIUS_PX);
        gl.uniform_1_f32(gl.get_uniform_location(self.edge, "u_strength").as_ref(), EDGE_STRENGTH);
        gl.uniform_3_f32(
            gl.get_uniform_location(self.edge, "u_color").as_ref(),
            EDGE_COLOR[0], EDGE_COLOR[1], EDGE_COLOR[2],
        );
        gl.bind_vertex_array(Some(self.empty_vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.disable(glow::BLEND);
    }

    /// Screen-space selection outline. The selected object's silhouette is drawn
    /// into an offscreen depth target (the mask); the scene depth is blitted into
    /// a second target. A fullscreen pass then paints the ring within
    /// `OUTLINE_RADIUS_PX` of the silhouette in a constant screen-space width
    /// (even on every side), fading it where the scene depth is nearer than the
    /// object — i.e. where it is occluded.
    unsafe fn draw_outline(
        &mut self,
        gl: &glow::Context,
        view: &glam::Mat4,
        proj: &glam::Mat4,
        model: &glam::Mat4,
        index: usize,
        vp: &eframe::egui::epaint::ViewportInPixels,
    ) {
        let (w, h) = (vp.width_px, vp.height_px);
        if w <= 0 || h <= 0 || !self.ensure_targets(gl, w, h) {
            return;
        }
        let buf = &self.objects[index];

        // egui leaves the scissor set to the panel's clip rect (window-space);
        // it would reject our offscreen draws (which use a 0,0-origin viewport),
        // so disable it for the offscreen passes and restore it for the
        // composite back to the default framebuffer.
        let scissor_was_on = gl.is_enabled(glow::SCISSOR_TEST);
        gl.disable(glow::SCISSOR_TEST);

        // Pass 1: render the selected object's depth into the mask target. The
        // scene program is reused; with no colour attachment only depth is kept.
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.targets.as_ref().unwrap().mask_fbo));
        gl.viewport(0, 0, w, h);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.use_program(Some(self.program));
        uniform_mat4(gl, self.program, "u_view", view);
        uniform_mat4(gl, self.program, "u_proj", proj);
        uniform_mat4(gl, self.program, "u_model", model);
        gl.bind_vertex_array(Some(buf.vao));
        gl.draw_arrays(glow::TRIANGLES, 0, buf.vertex_count);

        // Composite the outline back onto the on-screen preview. (Scene depth
        // for the occlusion test comes from the ID pass's depth texture.)
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.viewport(vp.left_px, vp.from_bottom_px, w, h);
        if scissor_was_on {
            gl.enable(glow::SCISSOR_TEST);
        }
        gl.disable(glow::DEPTH_TEST);
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

        let t = self.targets.as_ref().unwrap();
        gl.use_program(Some(self.composite));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(t.mask_depth));
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(t.id_depth));
        gl.uniform_1_i32(gl.get_uniform_location(self.composite, "u_mask_depth").as_ref(), 0);
        gl.uniform_1_i32(gl.get_uniform_location(self.composite, "u_scene_depth").as_ref(), 1);
        gl.uniform_2_f32(
            gl.get_uniform_location(self.composite, "u_texel").as_ref(),
            1.0 / w as f32,
            1.0 / h as f32,
        );
        gl.uniform_1_i32(
            gl.get_uniform_location(self.composite, "u_radius").as_ref(),
            OUTLINE_RADIUS_PX,
        );
        gl.uniform_3_f32(
            gl.get_uniform_location(self.composite, "u_color").as_ref(),
            OUTLINE_COLOR[0], OUTLINE_COLOR[1], OUTLINE_COLOR[2],
        );
        gl.uniform_1_f32(
            gl.get_uniform_location(self.composite, "u_alpha_full").as_ref(),
            OUTLINE_ALPHA_FULL,
        );
        gl.uniform_1_f32(
            gl.get_uniform_location(self.composite, "u_alpha_faint").as_ref(),
            OUTLINE_ALPHA_FAINT,
        );
        gl.bind_vertex_array(Some(self.empty_vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 3);

        // Leave the texture units and blend state as egui expects to find them.
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.disable(glow::BLEND);
    }

    /// Create or resize the offscreen depth targets to `w`×`h`. Returns whether
    /// usable targets are available (false if a framebuffer is incomplete, in
    /// which case the outline is skipped rather than risking a GL error).
    unsafe fn ensure_targets(&mut self, gl: &glow::Context, w: i32, h: i32) -> bool {
        if let Some(t) = &self.targets {
            if t.w == w && t.h == h {
                return true;
            }
            gl.delete_framebuffer(t.mask_fbo);
            gl.delete_texture(t.mask_depth);
            gl.delete_framebuffer(t.id_fbo);
            gl.delete_texture(t.id_tex);
            gl.delete_texture(t.id_depth);
            self.targets = None;
        }

        let (mask_fbo, mask_depth) = create_depth_target(gl, w, h);
        let mask_ok = gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
        let (id_fbo, id_tex, id_depth) = create_id_target(gl, w, h);
        let id_ok = gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);

        if !mask_ok || !id_ok {
            gl.delete_framebuffer(mask_fbo);
            gl.delete_texture(mask_depth);
            gl.delete_framebuffer(id_fbo);
            gl.delete_texture(id_tex);
            gl.delete_texture(id_depth);
            return false;
        }

        self.targets = Some(OutlineTargets { w, h, mask_fbo, mask_depth, id_fbo, id_tex, id_depth });
        true
    }
}

/// Create an FBO with an RGBA8 colour texture (object IDs) and a depth texture
/// (the scene depth), sized `w`×`h`. Both are sampleable. Leaves it bound.
unsafe fn create_id_target(
    gl: &glow::Context,
    w: i32,
    h: i32,
) -> (glow::Framebuffer, glow::Texture, glow::Texture) {
    let color = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(color));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA8 as i32,
        w,
        h,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(None),
    );
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);

    let depth = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(depth));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::DEPTH_COMPONENT24 as i32,
        w,
        h,
        0,
        glow::DEPTH_COMPONENT,
        glow::UNSIGNED_INT,
        glow::PixelUnpackData::Slice(None),
    );
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_COMPARE_MODE, glow::NONE as i32);

    let fbo = gl.create_framebuffer().unwrap();
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
    gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(color), 0);
    gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::TEXTURE_2D, Some(depth), 0);
    (fbo, color, depth)
}

/// Create a depth texture + a depth-only framebuffer attaching it, sized `w`×`h`.
/// Leaves the framebuffer bound so the caller can check completeness.
unsafe fn create_depth_target(gl: &glow::Context, w: i32, h: i32) -> (glow::Framebuffer, glow::Texture) {
    let tex = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::DEPTH_COMPONENT24 as i32,
        w,
        h,
        0,
        glow::DEPTH_COMPONENT,
        glow::UNSIGNED_INT,
        glow::PixelUnpackData::Slice(None),
    );
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
    // Sample as a plain value, not a shadow comparison.
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_COMPARE_MODE, glow::NONE as i32);
    let fbo = gl.create_framebuffer().unwrap();
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
    gl.framebuffer_texture_2d(
        glow::FRAMEBUFFER,
        glow::DEPTH_ATTACHMENT,
        glow::TEXTURE_2D,
        Some(tex),
        0,
    );
    // Depth-only: no colour buffer to draw to or read from.
    gl.draw_buffer(glow::NONE);
    gl.read_buffer(glow::NONE);
    (fbo, tex)
}

/// Preview color and whether the material is emissive (drawn unlit at full color).
fn preview_color(m: &MaterialSpec) -> ([f32; 3], bool) {
    match m {
        MaterialSpec::Lambertian { albedo } => {
            let c = albedo.preview_color();
            ([c.x, c.y, c.z], false)
        }
        MaterialSpec::Glossy { albedo, .. } => {
            let c = albedo.preview_color();
            ([c.x, c.y, c.z], false)
        }
        MaterialSpec::Metal { albedo, .. } => ([albedo.x, albedo.y, albedo.z], false),
        MaterialSpec::Dielectric { .. } => ([0.85, 0.9, 1.0], false),
        MaterialSpec::DiffuseLight { emit } => {
            let c = emit.preview_color();
            ([c.x, c.y, c.z], true)
        }
    }
}

unsafe fn uniform_mat4(gl: &glow::Context, prog: glow::Program, name: &str, m: &glam::Mat4) {
    let loc = gl.get_uniform_location(prog, name);
    gl.uniform_matrix_4_f32_slice(loc.as_ref(), false, &m.to_cols_array());
}

unsafe fn link_program(gl: &glow::Context, vert: &str, frag: &str) -> glow::Program {
    let program = gl.create_program().unwrap();
    let shaders = [(glow::VERTEX_SHADER, vert), (glow::FRAGMENT_SHADER, frag)];
    let mut compiled = Vec::new();
    for (ty, src) in shaders {
        let s = gl.create_shader(ty).unwrap();
        gl.shader_source(s, src);
        gl.compile_shader(s);
        assert!(gl.get_shader_compile_status(s), "{}", gl.get_shader_info_log(s));
        gl.attach_shader(program, s);
        compiled.push(s);
    }
    gl.link_program(program);
    assert!(gl.get_program_link_status(program), "{}", gl.get_program_info_log(program));
    for s in compiled {
        gl.detach_shader(program, s);
        gl.delete_shader(s);
    }
    program
}

const VERT: &str = r#"
layout (location = 0) in vec3 a_pos;
layout (location = 1) in vec3 a_normal;
uniform mat4 u_model;
uniform mat4 u_view;
uniform mat4 u_proj;
out vec3 v_normal_view;
out vec3 v_normal_world;
void main() {
    mat4 mv = u_view * u_model;
    v_normal_view = mat3(mv) * a_normal;
    v_normal_world = mat3(u_model) * a_normal;
    gl_Position = u_proj * mv * vec4(a_pos, 1.0);
}
"#;

const FRAG: &str = r#"
in vec3 v_normal_view;
in vec3 v_normal_world;
uniform vec3 u_color;
uniform float u_ambient;
uniform bool u_emissive;
out vec4 frag;
void main() {
    if (u_emissive) {
        frag = vec4(u_color, 1.0);
        return;
    }
    // View-space normal drives the camera-relative key/fill (like solid mode);
    // world-space normal drives the hemisphere ambient (fixed up direction).
    vec3 nv = normalize(v_normal_view);
    vec3 nw = normalize(v_normal_world);
    if (!gl_FrontFacing) { nv = -nv; nw = -nw; }   // two-sided
    vec3 v = vec3(0.0, 0.0, 1.0);                   // toward the camera

    // Hemisphere ambient: brighter "sky" from above, darker "ground" below, so
    // up-facing surfaces (box tops, floor) separate from vertical walls/sides
    // even when everything shares one colour.
    vec3 sky = vec3(0.48, 0.50, 0.54);
    vec3 ground = vec3(0.20, 0.19, 0.18);
    vec3 ambient = mix(ground, sky, clamp(nw.y * 0.5 + 0.5, 0.0, 1.0));

    // Camera-relative key + fill.
    vec3 key = normalize(vec3(0.45, 0.65, 0.6));
    vec3 fill = normalize(vec3(-0.6, -0.15, 0.4));
    float diffuse = 0.6 * max(dot(nv, key), 0.0) + 0.22 * max(dot(nv, fill), 0.0);

    // Fresnel rim light to pop silhouettes.
    float rim = pow(1.0 - max(dot(nv, v), 0.0), 3.0) * 0.3;

    vec3 col = u_color * (ambient + diffuse) + vec3(rim);
    frag = vec4(col, 1.0);
}
"#;

// Fullscreen triangle generated from gl_VertexID; v_uv spans [0,1] over the
// viewport with (0,0) at the bottom-left, matching the depth targets.
const OUTLINE_VERT: &str = r#"
out vec2 v_uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

// Paints the selection outline: any pixel outside the silhouette but within
// u_radius of it becomes the outline colour, faded where the scene depth in
// front is nearer than the object (i.e. the object is occluded there).
const OUTLINE_FRAG: &str = r#"
in vec2 v_uv;
uniform highp sampler2D u_mask_depth;   // selected object's depth (1.0 = empty)
uniform highp sampler2D u_scene_depth;  // whole-scene depth at this pixel
uniform vec2 u_texel;
uniform int u_radius;
uniform vec3 u_color;
uniform float u_alpha_full;
uniform float u_alpha_faint;
out vec4 frag;

const int R = 4;              // compile-time max; u_radius clamps the disc
const float COVERED = 0.99999;

void main() {
    // Don't draw over the object itself, only the ring around it.
    if (texture(u_mask_depth, v_uv).r < COVERED) discard;

    float obj_depth = 1.0;
    bool ring = false;
    for (int dy = -R; dy <= R; ++dy) {
        for (int dx = -R; dx <= R; ++dx) {
            if (dx * dx + dy * dy > u_radius * u_radius) continue;
            float d = texture(u_mask_depth, v_uv + vec2(float(dx), float(dy)) * u_texel).r;
            if (d < COVERED) { ring = true; obj_depth = min(obj_depth, d); }
        }
    }
    if (!ring) discard;

    float scene_depth = texture(u_scene_depth, v_uv).r;
    float alpha = (scene_depth < obj_depth - 1e-4) ? u_alpha_faint : u_alpha_full;
    frag = vec4(u_color, alpha);
}
"#;

// Faint, feathered dark outline around every object. Samples the object-ID
// buffer in a disc and accumulates a distance-weighted measure of how much of
// the neighbourhood belongs to a *different* object. This fires on silhouettes,
// object-vs-object overlaps, AND contact lines (where depths match but IDs
// differ), and the distance weighting makes the line soft on both sides.
const EDGE_FRAG: &str = r#"
in vec2 v_uv;
uniform highp sampler2D u_id;
uniform vec2 u_texel;
uniform float u_radius;
uniform float u_strength;
uniform vec3 u_color;
out vec4 frag;

const int R = 6;                  // compile-time max; u_radius clamps the disc

void main() {
    float center = texture(u_id, v_uv).r;
    float acc = 0.0;
    float wsum = 0.0;
    // Uniform weights: the fraction of the disc belonging to a *different*
    // object is ~0.5 right on a boundary and falls smoothly to 0 over the full
    // radius — a natural feather with no saturated core.
    for (int dy = -R; dy <= R; ++dy) {
        for (int dx = -R; dx <= R; ++dx) {
            if (length(vec2(float(dx), float(dy))) > u_radius) continue;
            float id = texture(u_id, v_uv + vec2(float(dx), float(dy)) * u_texel).r;
            wsum += 1.0;
            if (abs(id - center) > (0.5 / 255.0)) acc += 1.0;
        }
    }
    float frac = (wsum > 0.0) ? acc / wsum : 0.0;
    float alpha = frac * u_strength;        // peaks at ~0.5 * u_strength
    if (alpha <= 0.003) discard;
    frag = vec4(u_color, alpha);
}
"#;
