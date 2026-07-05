use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use web_time::Instant;

use eframe::egui;

use crate::camera::Camera;
use crate::group::IntersectGroup;
use crate::integrator::{build_integrator, Integrator};
use crate::render::ProgressiveRenderer;
use crate::scene::{build_world, Scene};

/// Frame handed from the renderer to the UI. `width`/`height` always match the
/// dimensions of `rgba`, so the UI can resize its texture when the render
/// resolution changes.
pub struct SharedFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub passes: u32,
    pub total: u32,
    pub done: bool,
    pub elapsed: f64,
}

/// One in-flight render: the renderer plus the immutable inputs it was started
/// with. Recreated whenever the scene is invalidated.
struct ActiveRender {
    renderer: ProgressiveRenderer,
    camera: Camera,
    integrator: Box<dyn Integrator>,
    world: IntersectGroup,
    target: u32,
    scale: u32,
    start: Instant,
    /// Set once the target is reached and `done` has been published, so we stop
    /// re-publishing / re-waking the UI.
    finished: bool,
}

/// The platform-independent render core. `tick()` advances the render by at most
/// one pass; the native thread and the wasm per-frame pump both drive it.
pub struct RenderEngine {
    ctx: egui::Context,
    scene: Arc<Mutex<Scene>>,
    shared: Arc<Mutex<SharedFrame>>,
    generation: Arc<AtomicU64>,
    preview_scale: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
    /// Last generation we started a render for; `u64::MAX` means "never".
    last_gen: u64,
    active: Option<ActiveRender>,
}

impl RenderEngine {
    pub fn new(
        ctx: egui::Context,
        scene: Arc<Mutex<Scene>>,
        shared: Arc<Mutex<SharedFrame>>,
        generation: Arc<AtomicU64>,
        preview_scale: Arc<AtomicU32>,
        paused: Arc<AtomicBool>,
    ) -> Self {
        RenderEngine {
            ctx,
            scene,
            shared,
            generation,
            preview_scale,
            paused,
            last_gen: u64::MAX,
            active: None,
        }
    }

    /// Build a fresh render from the current scene snapshot, render its first
    /// pass, and publish it (dimensions + pixels) before returning.
    fn start_render(&mut self) {
        let snapshot = self.scene.lock().unwrap().clone();
        let world = build_world(&snapshot);
        let target = snapshot.camera.samples;

        // Reduced resolution while interacting so passes complete fast enough to
        // track the mouse; full resolution (scale 1) when idle.
        let scale = self.preview_scale.load(Ordering::Relaxed).max(1);
        let mut cam_cfg = snapshot.camera.clone();
        if scale > 1 {
            cam_cfg.image_width = (cam_cfg.image_width / scale).max(1);
        }
        let integrator = build_integrator(&cam_cfg);
        let firefly = cam_cfg.firefly_clamp;
        let camera = Camera::from(cam_cfg);
        let (w, h) = (camera.image_width(), camera.image_height());

        let mut renderer = ProgressiveRenderer::new(w, h, firefly);
        let start = Instant::now();

        // Render the first pass BEFORE publishing the new dimensions so a
        // resolution change never flashes black: the UI keeps showing the
        // previous frame until this one is ready to replace it.
        renderer.add_pass(&camera, integrator.as_ref(), &world);
        {
            let mut s = self.shared.lock().unwrap();
            s.width = w;
            s.height = h;
            s.rgba = renderer.to_rgba();
            s.passes = renderer.passes();
            s.total = target;
            s.done = false;
            s.elapsed = start.elapsed().as_secs_f64();
        }
        self.ctx.request_repaint();

        self.active = Some(ActiveRender {
            renderer,
            camera,
            integrator,
            world,
            target,
            scale,
            start,
            finished: false,
        });
    }

    /// Advance the render by at most one pass. Returns `true` if work was done
    /// (call again soon), `false` if idle (paused, waiting for an edit, or the
    /// current render has finished).
    pub fn tick(&mut self) -> bool {
        // Idle while paused (Edit mode); drop any in-flight render.
        if self.paused.load(Ordering::Relaxed) {
            self.active = None;
            return false;
        }

        // A newer edit (or the very first render) restarts from a fresh world.
        let current_gen = self.generation.load(Ordering::Relaxed);
        if current_gen != self.last_gen {
            self.last_gen = current_gen;
            self.start_render();
            return true;
        }

        let Some(a) = self.active.as_mut() else {
            return false; // nothing to do until the next invalidate
        };

        // Stop at the sample target or once every pixel has converged (adaptive
        // sampling) — whichever comes first.
        if a.renderer.passes() >= a.target || a.renderer.all_converged() {
            // Reached the target: mark done once (full-resolution only — reduced
            // previews are throwaway and snap back via a later invalidate).
            if !a.finished {
                a.finished = true;
                if a.scale == 1 {
                    self.shared.lock().unwrap().done = true;
                    self.ctx.request_repaint();
                }
            }
            return false;
        }

        // Add one more pass and publish it.
        a.renderer.add_pass(&a.camera, a.integrator.as_ref(), &a.world);
        {
            let mut s = self.shared.lock().unwrap();
            s.rgba = a.renderer.to_rgba();
            s.passes = a.renderer.passes();
            s.elapsed = a.start.elapsed().as_secs_f64();
        }
        self.ctx.request_repaint();
        true
    }
}

/// Owns the render core plus the shared state it publishes into. On native a
/// background thread drives the engine; on wasm the UI pumps it once per frame.
pub struct RenderTask {
    shared: Arc<Mutex<SharedFrame>>,
    generation: Arc<AtomicU64>,
    /// Resolution divisor: 1 = full quality, >1 = reduced preview while the user
    /// is interacting.
    preview_scale: Arc<AtomicU32>,
    /// While true the renderer idles instead of tracing (Edit mode).
    paused: Arc<AtomicBool>,
    /// wasm-only: the engine lives on the main thread and is pumped each frame.
    #[cfg(target_arch = "wasm32")]
    engine: std::cell::RefCell<RenderEngine>,
}

impl RenderTask {
    /// Create the shared buffer + control atomics and start driving the engine.
    /// On native this spawns the long-lived render thread (the window must stay
    /// on the main thread). On wasm the engine is stored for per-frame pumping.
    pub fn spawn(
        ctx: egui::Context,
        scene: Arc<Mutex<Scene>>,
        width: u32,
        height: u32,
        initial_total: u32,
    ) -> Self {
        let shared = Arc::new(Mutex::new(SharedFrame {
            rgba: vec![0u8; (width * height * 4) as usize],
            width,
            height,
            passes: 0,
            total: initial_total,
            done: false,
            elapsed: 0.0,
        }));
        let generation = Arc::new(AtomicU64::new(0));
        let preview_scale = Arc::new(AtomicU32::new(1));
        let paused = Arc::new(AtomicBool::new(false));

        let engine = RenderEngine::new(
            ctx,
            scene,
            shared.clone(),
            generation.clone(),
            preview_scale.clone(),
            paused.clone(),
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut engine = engine;
            std::thread::spawn(move || loop {
                // `tick` returns false when idle (paused, no pending edit, or
                // finished); sleep then so we neither busy-spin nor trace.
                if !engine.tick() {
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            RenderTask {
                shared,
                generation,
                preview_scale,
                paused,
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            RenderTask {
                shared,
                generation,
                preview_scale,
                paused,
                engine: std::cell::RefCell::new(engine),
            }
        }
    }

    /// Drive the engine forward. No-op on native (the thread does it); on wasm
    /// this advances one pass per call and is invoked once per egui frame.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn pump(&self) {}

    #[cfg(target_arch = "wasm32")]
    pub fn pump(&self) {
        // `tick` self-requests a repaint when it publishes, which schedules the
        // next frame (and therefore the next pump) until the render goes idle.
        let _ = self.engine.borrow_mut().tick();
    }

    /// Signal that the scene changed: cancels the in-flight render and restarts.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Stop tracing (Edit mode).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    /// Resume tracing and restart from the current scene (Render mode).
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.invalidate();
    }

    /// Set the resolution divisor for subsequent renders (1 = full quality).
    pub fn set_preview_scale(&self, scale: u32) {
        self.preview_scale.store(scale.max(1), Ordering::Relaxed);
    }

    /// Current resolution divisor.
    pub fn preview_scale(&self) -> u32 {
        self.preview_scale.load(Ordering::Relaxed)
    }

    /// Lock and read the current shared frame.
    pub fn lock(&self) -> MutexGuard<'_, SharedFrame> {
        self.shared.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::cornell_box;

    /// A fresh engine, once invalidated, accumulates one pass per tick until it
    /// reaches the scene's target sample count, then goes idle.
    #[test]
    fn engine_accumulates_passes_to_target() {
        let scene = cornell_box();
        // Keep the test fast: trace a tiny image with a few samples.
        let mut s = scene.clone();
        s.camera.image_width = 16;
        s.camera.samples = 3;
        let target = s.camera.samples;
        let scene = std::sync::Arc::new(std::sync::Mutex::new(s));

        let ctx = eframe::egui::Context::default();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(SharedFrame {
            rgba: vec![],
            width: 0,
            height: 0,
            passes: 0,
            total: target,
            done: false,
            elapsed: 0.0,
        }));
        let generation = std::sync::Arc::new(AtomicU64::new(0));
        let preview_scale = std::sync::Arc::new(AtomicU32::new(1));
        let paused = std::sync::Arc::new(AtomicBool::new(false));

        let mut engine = RenderEngine::new(
            ctx,
            scene,
            shared.clone(),
            generation.clone(),
            preview_scale,
            paused,
        );

        // First invalidation kicks off a render.
        generation.fetch_add(1, Ordering::Relaxed);

        // Tick until idle; bounded so a bug can't hang the test.
        let mut ticks = 0;
        while engine.tick() {
            ticks += 1;
            assert!(ticks < 100, "engine never went idle");
        }

        let frame = shared.lock().unwrap();
        assert_eq!(frame.passes, target, "should accumulate exactly `target` passes");
        assert!(frame.done, "should be marked done at full resolution");
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }
}
