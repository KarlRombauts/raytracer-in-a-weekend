pub mod camera;
pub mod color;
pub mod geometry;
pub mod group;
pub mod interval;
pub mod material;
pub mod platform;
pub mod ray;
pub mod render;
pub mod sampling;
pub mod scene;
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
