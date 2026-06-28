use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::camera::Camera;
use crate::render::ProgressiveRenderer;
use crate::scene::{build_world, Scene};

/// Frame handed from the render thread to the UI thread.
pub struct SharedFrame {
    pub rgba: Vec<u8>,
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
            passes: 0,
            total: initial_total,
            done: false,
            elapsed: 0.0,
        }));
        let generation = Arc::new(AtomicU64::new(0));

        let shared_bg = shared.clone();
        let generation_bg = generation.clone();
        std::thread::spawn(move || {
            let mut last_gen = u64::MAX;
            loop {
                // Wait until the scene changes (or render the very first time).
                let current_gen = generation_bg.load(Ordering::Relaxed);
                if current_gen == last_gen {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                last_gen = current_gen;

                // Rebuild the world from the current scene snapshot.
                let snapshot = scene.lock().unwrap().clone();
                let camera = Camera::from(snapshot.camera.clone());
                let world = build_world(&snapshot);
                let target = snapshot.camera.samples;
                let (w, h) = (camera.image_width(), camera.image_height());

                {
                    let mut s = shared_bg.lock().unwrap();
                    s.passes = 0;
                    s.total = target;
                    s.done = false;
                    s.elapsed = 0.0;
                }

                let mut renderer = ProgressiveRenderer::new(w, h);
                let start = Instant::now();
                let mut cancelled = false;
                for _ in 0..target {
                    // Bail out between passes if another edit landed.
                    if generation_bg.load(Ordering::Relaxed) != last_gen {
                        cancelled = true;
                        break;
                    }
                    renderer.add_pass(&camera, &world);
                    let rgba = renderer.to_rgba();
                    {
                        let mut s = shared_bg.lock().unwrap();
                        s.rgba = rgba;
                        s.passes = renderer.passes();
                        s.elapsed = start.elapsed().as_secs_f64();
                    }
                    ctx.request_repaint();
                }

                if !cancelled {
                    renderer.save_png("test.png");
                    shared_bg.lock().unwrap().done = true;
                    ctx.request_repaint();
                }
            }
        });

        RenderTask { shared, generation }
    }

    /// Signal that the scene changed: cancels the in-flight render and restarts.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Lock and read the current shared frame.
    pub fn lock(&self) -> MutexGuard<'_, SharedFrame> {
        self.shared.lock().unwrap()
    }
}
