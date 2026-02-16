//! LED Matrix HTTP API Server
//!
//! Runs a web server on the Pi that accepts commands to control the LED matrix.
//! Any device on the LAN can display images, play videos, scroll text, or
//! push raw frames via simple HTTP requests.
//!
//! ## Architecture
//! - **Render thread** (std::thread): owns the LED matrix, processes commands
//! - **HTTP server** (tokio/axum): accepts API requests, sends commands via channel
//!
//! ## Rust concepts
//! - `#[tokio::main]` async entry point
//! - `std::thread::spawn` for the render thread
//! - `std::sync::mpsc` channel between async and sync worlds
//! - `Arc<Mutex<T>>` for shared status
//!
//! ## Usage
//! ```sh
//! sudo ./target/release/led-matrix-rs --media-dir /path/to/media --port 8080
//! ```

#[cfg(not(feature = "hardware"))]
fn main() {
    eprintln!("This binary requires the 'hardware' feature (rpi-led-matrix).");
    eprintln!("Build with: cargo build --release");
    eprintln!("Tests can run without it: cargo test --no-default-features");
    std::process::exit(1);
}

#[cfg(feature = "hardware")]
fn main() {
    hardware_main();
}

#[cfg(feature = "hardware")]
#[tokio::main(flavor = "current_thread")]
async fn hardware_main() {
    use clap::Parser;
    use led_matrix_rs::PanelConfig;
    use led_matrix_rs::render::{DisplayStatus, render_loop};
    use led_matrix_rs::server::{self, AppState};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    /// LED Matrix HTTP API Server
    #[derive(Parser)]
    #[command(name = "led-matrix-rs")]
    #[command(about = "HTTP API server for controlling an RGB LED matrix")]
    #[command(version)]
    struct Args {
        /// Root directory containing images/ and videos/ subdirectories
        #[arg(long, default_value = ".")]
        media_dir: PathBuf,

        /// Port to listen on
        #[arg(long, default_value = "8080")]
        port: u16,

        /// Path to BDF font directory
        #[arg(long, default_value = "fonts/bdf")]
        fonts_dir: PathBuf,

        /// Number of rows on the LED panel
        #[arg(long, default_value = "64")]
        rows: u32,

        /// Number of columns on the LED panel
        #[arg(long, default_value = "64")]
        cols: u32,
    }

    // Initialize tracing subscriber for request logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_ansi(false) // Disable ANSI color codes for systemd/journald
        .compact()
        .init();

    let args = Args::parse();
    let panel = PanelConfig::new(args.rows, args.cols);

    let media_dir = args.media_dir.canonicalize().unwrap_or_else(|_| {
        eprintln!("Warning: could not canonicalize media dir, using as-is");
        args.media_dir.clone()
    });

    let fonts_dir = args.fonts_dir.canonicalize().unwrap_or_else(|_| {
        eprintln!("Warning: could not canonicalize fonts dir, using as-is");
        args.fonts_dir.clone()
    });

    tracing::info!("LED Matrix HTTP Server v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Panel: {}x{}", panel.cols, panel.rows);
    tracing::info!("Media dir: {}", media_dir.display());
    tracing::info!("Fonts dir: {}", fonts_dir.display());
    tracing::info!("Port: {}", args.port);

    // Create the channel for sending commands to the render thread.
    let (tx, rx) = mpsc::channel();

    // Shared display status — render thread writes, HTTP handlers read.
    let status = Arc::new(Mutex::new(DisplayStatus::new()));

    // Spawn the render thread.
    let render_status = status.clone();
    let render_handle = std::thread::spawn(move || {
        render_loop(rx, render_status, fonts_dir, panel);
    });

    // Build the HTTP server
    let app_state = AppState {
        command_tx: tx,
        status,
        media_dir,
        panel,
    };

    let app = server::create_router(app_state);

    // Start listening
    let addr = format!("0.0.0.0:{}", args.port);
    tracing::info!("Listening on http://{}", addr);
    tracing::info!("API Documentation: http://localhost:{}/docs", args.port);
    tracing::info!("Try: curl http://localhost:{}/api/v1/status", args.port);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    // Run the server — this blocks until the process is killed
    axum::serve(listener, app).await.expect("Server error");

    drop(render_handle);
}
