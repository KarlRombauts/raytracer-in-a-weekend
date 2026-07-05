#[cfg(not(target_arch = "wasm32"))]
use clap::{Parser, Subcommand, ValueEnum};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Parser)]
#[command(about = "A Monte-Carlo path tracer with an interactive editor")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Subcommand)]
enum Command {
    /// Render a .scene file headlessly to a PNG (no editor window).
    Render {
        /// Path to the .scene file to render.
        scene: std::path::PathBuf,
        /// Output PNG path.
        #[arg(short, long, default_value = "render.png")]
        out: std::path::PathBuf,
        /// Override samples per pixel.
        #[arg(long)]
        samples: Option<u32>,
        /// Override max bounces (path depth).
        #[arg(long)]
        bounces: Option<u32>,
        /// Override image width in pixels (height follows the aspect ratio).
        #[arg(long)]
        width: Option<u32>,
        /// Integration algorithm.
        #[arg(long, value_enum, default_value_t = CliIntegrator::Mis)]
        integrator: CliIntegrator,
    },
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, ValueEnum)]
enum CliIntegrator {
    Mis,
    Naive,
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use raytracer_in_a_weekend::camera::config::IntegratorKind;

    // Preserve the legacy bare flag before clap (which would reject it).
    if std::env::args().any(|a| a == "--gen-thumbnails") {
        raytracer_in_a_weekend::gen_thumbnails().expect("thumbnail generation");
        return;
    }

    match Cli::parse().command {
        None => raytracer_in_a_weekend::run_default(),
        Some(Command::Render { scene, out, samples, bounces, width, integrator }) => {
            let kind = match integrator {
                CliIntegrator::Mis => IntegratorKind::Mis,
                CliIntegrator::Naive => IntegratorKind::Naive,
            };
            raytracer_in_a_weekend::run_render_cli(&scene, &out, samples, bounces, width, kind)
                .expect("headless render");
        }
    }
}

// The wasm build ships as a cdylib driven from JS (see `WebHandle` in the lib);
// the binary target has no role there, so give it an empty `main` to satisfy
// `cargo check --target wasm32-unknown-unknown`.
#[cfg(target_arch = "wasm32")]
fn main() {}
