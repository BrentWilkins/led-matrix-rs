//! # Minimal LED Matrix Example
//!
//! This is your first Rust LED matrix program! It demonstrates:
//! - Initializing the matrix with our hardware config
//! - Setting individual pixels
//! - Drawing shapes (lines, circles)
//! - Double-buffering with `swap()` for flicker-free animation
//! - Clean shutdown with Ctrl+C
//!
//! ## Run it
//! ```sh
//! cargo build --release --example minimal
//! sudo ./target/release/examples/minimal
//! ```
//!
//! ## Rust concepts introduced
//! - `use` imports and module paths
//! - `let` vs `let mut` (immutability by default)
//! - Ownership: each value has exactly one owner
//! - Borrowing: `&` (shared reference) vs `&mut` (exclusive reference)
//! - The main loop pattern with `std::thread::sleep`

#[cfg(not(feature = "hardware"))]
fn main() {
    eprintln!("This example requires the 'hardware' feature.");
}

#[cfg(feature = "hardware")]
fn main() {
    use led_matrix_rs::{
        PanelConfig, color, color_from_hue, create_matrix, is_running, setup_signal_handler,
    };
    use std::thread;
    use std::time::Duration;

    // ── Setup ──────────────────────────────────────────────────────
    let panel = PanelConfig::default();
    let matrix = create_matrix(panel).expect("Failed to create matrix");
    let running = setup_signal_handler();
    let mut canvas = matrix.offscreen_canvas();
    let mut frame: u16 = 0;

    let max_x = (panel.cols - 1) as i32;
    let max_y = (panel.rows - 1) as i32;
    let center_x = (panel.cols / 2) as i32;
    let center_y = (panel.rows / 2) as i32;

    // ── Main loop ──────────────────────────────────────────────────
    while is_running(&running) {
        canvas.clear();

        // Phase A: Moving pixel across the top row
        let x = (frame % panel.cols as u16) as i32;
        let white = color(255, 255, 255);
        canvas.set(x, 0, &white.into());

        // Phase B: Color-cycling pixel at center
        let hue = frame.wrapping_mul(5);
        let rainbow = color_from_hue(hue);
        canvas.set(center_x, center_y, &rainbow.into());

        // Phase C: X pattern
        let red = color(255, 0, 0);
        let green = color(0, 255, 0);
        canvas.draw_line(0, 0, max_x, max_y, &red.into());
        canvas.draw_line(max_x, 0, 0, max_y, &green.into());

        // Phase D: Pulsing circle
        let pulse = (frame % 40) as u32;
        let radius = if pulse < 20 { pulse } else { 40 - pulse };
        let blue = color(0, 100, 255);
        canvas.draw_circle(center_x, center_y, radius, &blue.into());

        canvas = matrix.swap(canvas);
        frame = frame.wrapping_add(1);
        thread::sleep(Duration::from_millis(16));
    }

    println!("\nShutting down cleanly.");
}
