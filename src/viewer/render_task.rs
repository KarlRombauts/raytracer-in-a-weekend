use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::camera::Camera;
use crate::render::ProgressiveRenderer;
use crate::scene::{build_world, Scene};

/// Frame handed from the render thread to the UI thread. `width`/`height`
/// always match the dimensions of `rgba`, so the UI can resize its texture
/// when the render resolution changes.
pub struct SharedFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub passes: u32,
    pub total: u32,
    pub done: bool,
    pub elapsed: f64,
}

/// Owns a long-lived render thread plus the shared buffer it publishes into.
/// The thread watches a generation counter: each time the scene is invalidated
/// it cancels the in-flight render and restarts from a freshly built world.
pub struct RenderTask {
    shared: Arc<Mutex<SharedFrame>>,
    generation: Arc<AtomicU64>,
    /// Resolution divisor: 1 = full quality, >1 = reduced preview while the
    /// user is interacting so passes complete fast enough to track the mouse.
    preview_scale: Arc<AtomicU32>,
    /// While true the render thread idles instead of tracing — used in Edit
    /// mode, where the GL preview is shown and a background path trace is just
    /// wasted work.
    paused: Arc<AtomicBool>,
}

impl RenderTask {
    /// Spawn the render thread. The window must stay on the main thread
    /// (macOS), so rendering runs in the background, publishing each finished
    /// pass into the shared buffer and waking the UI via `ctx.request_repaint`.
    /// Saves `test.png` whenever a render completes without being cancelled.
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

        let shared_bg = shared.clone();
        let generation_bg = generation.clone();
        let preview_scale_bg = preview_scale.clone();
        let paused_bg = paused.clone();
        std::thread::spawn(move || {
            let mut last_gen = u64::MAX;
            loop {
                // Idle while paused (Edit mode) — don't trace, don't busy-spin.
                if paused_bg.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                // Wait until the scene changes (or render the very first time).
                let current_gen = generation_bg.load(Ordering::Relaxed);
                if current_gen == last_gen {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                last_gen = current_gen;

                // Snapshot the scene and build the world.
                let snapshot = scene.lock().unwrap().clone();
                let world = build_world(&snapshot);
                let target = snapshot.camera.samples;

                // Render at reduced resolution while the user is interacting so
                // each pass completes fast enough to track the mouse; full
                // resolution (scale 1) when idle. Shrinking the camera's image
                // width scales the whole render down; the UI upsamples it.
                let scale = preview_scale_bg.load(Ordering::Relaxed).max(1);
                let mut cam_cfg = snapshot.camera.clone();
                if scale > 1 {
                    cam_cfg.image_width = (cam_cfg.image_width / scale).max(1);
                }
                let camera = Camera::from(cam_cfg);
                let (w, h) = (camera.image_width(), camera.image_height());

                let mut renderer = ProgressiveRenderer::new(w, h);
                let start = Instant::now();

                // Render the first pass BEFORE publishing the new dimensions, so
                // a resolution change never flashes black: the UI keeps showing
                // the previous frame until this one is ready to replace it.
                renderer.add_pass(&camera, &world);
                {
                    let mut s = shared_bg.lock().unwrap();
                    s.width = w;
                    s.height = h;
                    s.rgba = renderer.to_rgba();
                    s.passes = renderer.passes();
                    s.total = target;
                    s.done = false;
                    s.elapsed = start.elapsed().as_secs_f64();
                }
                ctx.request_repaint();

                // Keep adding passes until the target is reached or a newer edit
                // lands. The check is AFTER publishing, so even a fast drag
                // (which restarts every mouse move) still shows ≥1 sample.
                let stop = |last: u64| {
                    generation_bg.load(Ordering::Relaxed) != last
                        || paused_bg.load(Ordering::Relaxed)
                };
                let mut cancelled = stop(last_gen);
                // Stop at the sample target, a newer edit, or once every pixel
                // has converged (adaptive sampling) — whichever comes first.
                while !cancelled && renderer.passes() < target && !renderer.all_converged() {
                    renderer.add_pass(&camera, &world);
                    {
                        let mut s = shared_bg.lock().unwrap();
                        s.rgba = renderer.to_rgba();
                        s.passes = renderer.passes();
                        s.elapsed = start.elapsed().as_secs_f64();
                    }
                    ctx.request_repaint();
                    cancelled = stop(last_gen);
                }

                // Only the full-resolution image is worth saving to disk;
                // reduced previews are throwaway.
                if !cancelled && scale == 1 {
                    renderer.save_png("test.png");
                    shared_bg.lock().unwrap().done = true;
                    ctx.request_repaint();
                }
            }
        });

        RenderTask {
            shared,
            generation,
            preview_scale,
            paused,
        }
    }

    /// Signal that the scene changed: cancels the in-flight render and restarts.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Stop tracing (Edit mode). Any in-flight render bails on the next pass.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    /// Resume tracing and restart from the current scene (Render mode).
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.invalidate();
    }

    /// Set the resolution divisor for subsequent renders (1 = full quality).
    /// Takes effect on the next restart — pair with `invalidate` to apply now.
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
