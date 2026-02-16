//! # Image Viewer Example
//!
//! Loads an image from disk and displays it on the LED matrix.
//!
//! ## Run it
//! ```sh
//! cargo build --release --example image_viewer
//! sudo ./target/release/examples/image_viewer path/to/image.png
//! ```

#[cfg(not(feature = "hardware"))]
fn main() {
    eprintln!("This example requires the 'hardware' feature.");
}

#[cfg(feature = "hardware")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use clap::Parser;
    use image::{ImageReader, imageops::FilterType};
    use led_matrix_rs::{PanelConfig, color, create_matrix, is_running, setup_signal_handler};
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    #[derive(Parser)]
    #[command(name = "image_viewer")]
    #[command(about = "Display an image on the LED matrix")]
    struct Args {
        /// Path to the image file (PNG or JPEG)
        image_path: PathBuf,
    }

    let args = Args::parse();
    let panel = PanelConfig::default();

    // Create matrix with same PWM settings as the server
    let matrix = create_matrix(panel)?;
    let running = setup_signal_handler();
    let mut canvas = matrix.offscreen_canvas();

    println!("Loading image: {}", args.image_path.display());
    let img = ImageReader::open(&args.image_path)?.decode()?;
    let resized = img
        .resize_exact(panel.cols, panel.rows, FilterType::Lanczos3)
        .to_rgb8();
    println!(
        "Image loaded and resized to {}x{}. Displaying...",
        panel.cols, panel.rows
    );

    for (x, y, pixel) in resized.enumerate_pixels() {
        let led_color = color(pixel[0], pixel[1], pixel[2]);
        canvas.set(x as i32, y as i32, &led_color.into());
    }

    canvas = matrix.swap(canvas);
    println!("Image displayed! Press Ctrl+C to exit.");

    while is_running(&running) {
        thread::sleep(Duration::from_millis(100));
    }

    println!("\nShutting down cleanly.");
    Ok(())
}
