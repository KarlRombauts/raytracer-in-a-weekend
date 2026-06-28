//! A minimal flat-shaded OpenGL renderer for the scene preview, driven through
//! eframe's glow context inside an egui paint callback.

use glow::HasContext;

use crate::scene::{MaterialSpec, Scene};
use super::camera_gl;

const AMBIENT: f32 = 0.25;

struct ObjectBuffers {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    vertex_count: i32,
}

pub struct SceneRenderer {
    program: glow::Program,
    objects: Vec<ObjectBuffers>,
    built_count: usize, // object count the buffers were built for
}

impl SceneRenderer {
    pub fn new(gl: &glow::Context) -> Self {
        let shader_version = if cfg!(target_arch = "wasm32") {
            "#version 300 es\nprecision highp float;\n"
        } else {
            "#version 330\n"
        };
        let vert = format!("{shader_version}{VERT}");
        let frag = format!("{shader_version}{FRAG}");
        let program = unsafe { link_program(gl, &vert, &frag) };
        SceneRenderer { program, objects: Vec::new(), built_count: usize::MAX }
    }

    fn rebuild(&mut self, gl: &glow::Context, scene: &Scene) {
        unsafe {
            for o in self.objects.drain(..) {
                gl.delete_vertex_array(o.vao);
                gl.delete_buffer(o.vbo);
            }
            for obj in &scene.objects {
                let mesh = obj.shape.render_mesh();
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
                self.objects.push(ObjectBuffers { vao, vbo, vertex_count: mesh.positions.len() as i32 });
            }
            self.built_count = scene.objects.len();
        }
    }

    pub fn paint(&mut self, gl: &glow::Context, scene: &Scene, viewport_px: (f32, f32)) {
        if self.built_count != scene.objects.len() {
            self.rebuild(gl, scene);
        }
        let view = camera_gl::view_matrix(&scene.camera);
        let proj = camera_gl::projection_matrix(&scene.camera, 0.05, 1000.0);
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
                let center = mesh_center(obj);
                let model = camera_gl::model_matrix(&obj.transform, center);
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
        }
        let _ = viewport_px; // glow uses the callback's scissor/viewport
    }
}

/// Compute the axis-aligned centroid of the render mesh for use as the pivot
/// point in `model_matrix`. (Shape::build is private; we derive from the
/// public render_mesh instead.)
fn mesh_center(obj: &crate::scene::ObjectSpec) -> glam::Vec3 {
    let mesh = obj.shape.render_mesh();
    if mesh.positions.is_empty() {
        return glam::Vec3::ZERO;
    }
    let mut min = glam::Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = glam::Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in &mesh.positions {
        let v = glam::Vec3::new(p[0], p[1], p[2]);
        min = min.min(v);
        max = max.max(v);
    }
    (min + max) * 0.5
}

/// Preview color and whether the material is emissive (drawn unlit at full color).
fn preview_color(m: &MaterialSpec) -> ([f32; 3], bool) {
    match m {
        MaterialSpec::Lambertian { albedo } => ([albedo.x, albedo.y, albedo.z], false),
        MaterialSpec::Glossy { albedo, .. } => ([albedo.x, albedo.y, albedo.z], false),
        MaterialSpec::Metal { albedo, .. } => ([albedo.x, albedo.y, albedo.z], false),
        MaterialSpec::Dielectric { .. } => ([0.85, 0.9, 1.0], false),
        MaterialSpec::DiffuseLight { emit } => ([emit.x, emit.y, emit.z], true),
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
void main() {
    mat4 mv = u_view * u_model;
    v_normal_view = mat3(mv) * a_normal;
    gl_Position = u_proj * mv * vec4(a_pos, 1.0);
}
"#;

const FRAG: &str = r#"
in vec3 v_normal_view;
uniform vec3 u_color;
uniform float u_ambient;
uniform bool u_emissive;
out vec4 frag;
void main() {
    if (u_emissive) {
        frag = vec4(u_color, 1.0);
        return;
    }
    // Headlight: light direction = view direction = -z in view space, so the
    // shade is just the normal's view-space z (facing the camera).
    vec3 n = normalize(v_normal_view);
    float ndotv = max(n.z, 0.0);
    float shade = u_ambient + (1.0 - u_ambient) * ndotv;
    frag = vec4(u_color * shade, 1.0);
}
"#;
