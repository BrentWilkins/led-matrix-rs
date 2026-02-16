//! # Pulsing Colors Example
//!
//! Fills the entire panel with color that smoothly cycles through
//! the rainbow while also pulsing brightness up and down.
//!
//! ## Rust concepts introduced
//! - `match` with ranges and guards
//! - Numeric casting between types (`as`)
//! - Wrapping arithmetic for overflow-safe counters
//! - Closures (anonymous functions)
//!
//! ## Run it
//! ```sh
//! cargo build --release --example pulsing
//! sudo ./target/release/examples/pulsing
//! ```

#[cfg(not(feature = "hardware"))]
fn main() {
    eprintln!("This example requires the 'hardware' feature.");
}

#[cfg(feature = "hardware")]
fn main() {
    use led_matrix_rs::{
        PanelConfig, color_from_hue, create_matrix, is_running, setup_signal_handler,
    };
    use std::thread;
    use std::time::Duration;

    let panel = PanelConfig::default();
    let matrix = create_matrix(panel).expect("Failed to create matrix");
    let running = setup_signal_handler();
    let mut canvas = matrix.offscreen_canvas();
    let mut frame: u32 = 0;

    while is_running(&running) {
        let hue = ((frame / 2) % 360) as u16;
        let base_color = color_from_hue(hue);

        // Triangle wave brightness: 0 → 100 → 0 over 200 frames
        let brightness_cycle = (frame % 200) as u8;
        let brightness = if brightness_cycle < 100 {
            brightness_cycle
        } else {
            (200 - brightness_cycle as u16) as u8
        };

        let dimmed = base_color.apply_brightness(brightness);
        canvas.fill(&dimmed.into());

        canvas = matrix.swap(canvas);
        frame = frame.wrapping_add(1);
        thread::sleep(Duration::from_millis(16));
    }

    println!("\nShutting down cleanly.");
}
