pub mod camera;
pub mod color;
pub mod geometry;
pub mod group;
pub mod integrator;
pub mod interval;
pub mod material;
pub mod platform;
pub mod ray;
pub mod render;
pub mod sampling;
pub mod scene;
pub mod scene_file;
pub mod scenes;
pub mod texture;
pub mod vec3;
pub mod viewer;

/// Native entry: open the interactive viewer on the default scene.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_default() {
    use crate::scenes::cornell_box;
    viewer::run(cornell_box());
}

/// Render pre-baked thumbnails for every library sample scene to
/// `assets/thumbnails/`. Driven by `just thumbnails`.
#[cfg(not(target_arch = "wasm32"))]
pub fn gen_thumbnails() -> std::io::Result<()> {
    viewer::samples::gen_thumbnails()
}

/// Headless render: load a `.scene`, apply CLI overrides (samples / bounces /
/// width), render to a PNG file with the chosen integrator. No window. `width`
/// sets the image width and lets the height follow from the scene's aspect ratio.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_render_cli(
    scene_path: &std::path::Path,
    out_path: &std::path::Path,
    samples: Option<u32>,
    bounces: Option<u32>,
    width: Option<u32>,
    integrator: crate::camera::config::IntegratorKind,
) -> std::io::Result<()> {
    let bytes = std::fs::read(scene_path)?;
    let mut scene = scene_file::decode(&bytes)
        .unwrap_or_else(|e| panic!("could not load {}: {e}", scene_path.display()))
        .scene;

    if let Some(s) = samples {
        scene.camera.samples = s;
    }
    if let Some(b) = bounces {
        scene.camera.max_depth = b;
    }
    if let Some(w) = width {
        scene.camera.image_width = w.max(1);
    }
    scene.camera.integrator = integrator;

    let world = scene::build_world(&scene);
    let camera = camera::Camera::from(scene.camera.clone());
    let integ = integrator::build_integrator(&scene.camera);
    let png = render::ProgressiveRenderer::render_to_png(
        &camera,
        integ.as_ref(),
        &world,
        scene.camera.firefly_clamp,
        scene.camera.samples,
    );
    std::fs::write(out_path, png)?;
    eprintln!("wrote {}", out_path.display());
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod cli_tests {
    use super::*;
    use crate::camera::config::IntegratorKind;

    #[test]
    fn run_render_cli_writes_a_valid_png_with_overrides() {
        // A real round-trip: encode a scene, decode it back through the CLI path,
        // apply overrides, render headless, and check the PNG on disk.
        let scene = scenes::cornell_box();
        let bytes = scene_file::encode(&scene, None, &[]);
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let scene_path = dir.join(format!("integ_cli_{pid}.scene"));
        let out_path = dir.join(format!("integ_cli_{pid}.png"));
        std::fs::write(&scene_path, &bytes).unwrap();

        run_render_cli(&scene_path, &out_path, Some(4), Some(2), Some(16), IntegratorKind::Naive).unwrap();

        let png = std::fs::read(&out_path).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let img = image::load_from_memory(&png).expect("valid PNG");
        assert_eq!(img.width(), 16, "width override applied");

        let _ = std::fs::remove_file(&scene_path);
        let _ = std::fs::remove_file(&out_path);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// JS-facing handle. `new WebHandle()` then `await handle.start(canvas)`.
    #[wasm_bindgen]
    pub struct WebHandle {
        runner: eframe::WebRunner,
    }

    #[wasm_bindgen]
    impl WebHandle {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Self {
            console_error_panic_hook::set_once();
            Self {
                runner: eframe::WebRunner::new(),
            }
        }

        /// Initialize the rayon worker pool, then start the eframe app on the
        /// given canvas. Must be `await`ed from JS.
        #[wasm_bindgen]
        pub async fn start(
            &self,
            canvas: web_sys::HtmlCanvasElement,
        ) -> Result<(), JsValue> {
            // Spawn the Web Worker pool BEFORE any rayon `par_iter` runs.
            let threads = web_sys::window()
                .and_then(|w| w.navigator().hardware_concurrency().into())
                .map(|n: f64| n as usize)
                .filter(|n| *n > 0)
                .unwrap_or(4);
            wasm_bindgen_rayon::init_thread_pool(threads).await?;

            let scene = crate::scenes::cornell_box();
            self.runner
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |cc| {
                        Ok(Box::new(crate::viewer::web_app(cc, scene)) as Box<dyn eframe::App>)
                    }),
                )
                .await
        }
    }
}
