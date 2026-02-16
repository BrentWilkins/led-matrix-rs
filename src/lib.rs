//! Shared utilities for LED matrix examples and the HTTP server.
//!
//! This module provides helpers that multiple examples use:
//! - Matrix initialization with our hardware defaults
//! - Signal handling for clean shutdown
//! - Color helper functions
//! - Panel configuration
//!
//! It also re-exports the server, render, and media modules used by
//! the main binary (HTTP API server).

pub mod media;
#[cfg(feature = "hardware")]
pub mod render;
#[cfg(feature = "hardware")]
pub mod server;

#[cfg(feature = "hardware")]
use rpi_led_matrix::{LedMatrix, LedMatrixOptions, LedRuntimeOptions};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// ── Panel configuration ────────────────────────────────────────────

/// Configuration for the LED panel dimensions.
///
/// # Rust concept: derive macros
/// `Clone, Copy` make this cheaply copyable (it's just two u32s).
/// `Debug` gives us `{:?}` formatting. `PartialEq, Eq` let us compare.
/// This is the idiomatic way to pass configuration through a system —
/// explicit, testable, and no hidden global state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelConfig {
    pub rows: u32,
    pub cols: u32,
}

impl PanelConfig {
    pub fn new(rows: u32, cols: u32) -> Self {
        Self { rows, cols }
    }

    /// Total number of pixels on the panel.
    pub fn pixel_count(&self) -> u32 {
        self.rows * self.cols
    }

    /// Number of bytes needed for a raw RGB frame (3 bytes per pixel).
    pub fn frame_byte_count(&self) -> usize {
        (self.rows * self.cols * 3) as usize
    }
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self { rows: 64, cols: 64 }
    }
}

// ── Color ──────────────────────────────────────────────────────────

/// Our own color type, decoupled from the hardware crate.
///
/// This lets us test color logic on macOS without needing `rpi-led-matrix`.
/// At the hardware boundary, we convert via `Into<LedColor>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Create a color from a hue value (0-360), with full saturation and brightness.
    /// Useful for rainbow effects.
    ///
    /// # Rust concept: match expressions
    /// Rust's `match` is exhaustive — the compiler ensures we handle all cases.
    pub fn from_hue(hue: u16) -> Self {
        let hue = hue % 360;
        let sector = hue / 60;
        let fraction = ((hue % 60) as f32) / 60.0;
        let rising = (fraction * 255.0) as u8;
        let falling = ((1.0 - fraction) * 255.0) as u8;

        match sector {
            0 => Self::new(255, rising, 0),  // Red → Yellow
            1 => Self::new(falling, 255, 0), // Yellow → Green
            2 => Self::new(0, 255, rising),  // Green → Cyan
            3 => Self::new(0, falling, 255), // Cyan → Blue
            4 => Self::new(rising, 0, 255),  // Blue → Magenta
            5 => Self::new(255, 0, falling), // Magenta → Red
            _ => Self::new(255, 0, 0),       // Unreachable, but Rust requires exhaustiveness
        }
    }

    /// Apply brightness scaling (0-100) to this color.
    pub fn apply_brightness(self, brightness: u8) -> Self {
        if brightness >= 100 {
            return self;
        }
        Self {
            r: ((self.r as u16 * brightness as u16) / 100) as u8,
            g: ((self.g as u16 * brightness as u16) / 100) as u8,
            b: ((self.b as u16 * brightness as u16) / 100) as u8,
        }
    }
}

/// Convert our Color to the hardware crate's LedColor at the boundary.
#[cfg(feature = "hardware")]
impl From<Color> for rpi_led_matrix::LedColor {
    fn from(c: Color) -> Self {
        rpi_led_matrix::LedColor {
            red: c.r,
            green: c.g,
            blue: c.b,
        }
    }
}

// ── Backward-compatible color helpers ──────────────────────────────
// These wrap the new Color type so existing code still compiles.

/// Create a Color from RGB values.
pub fn color(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b)
}

/// Create a color from a hue value (0-360), with full saturation and brightness.
pub fn color_from_hue(hue: u16) -> Color {
    Color::from_hue(hue)
}

// ── Matrix initialization ──────────────────────────────────────────

/// Create a matrix configured for our hardware:
/// Pi Zero 2 W + Adafruit Bonnet + configurable panel size.
///
/// # Rust concept: Result and the ? operator
/// This function returns `Result` because matrix initialization can fail
/// (e.g., if not running as root, or if GPIO is unavailable).
/// The caller uses `?` to propagate errors upward.
#[cfg(feature = "hardware")]
pub fn create_matrix(panel: PanelConfig) -> Result<LedMatrix, Box<dyn std::error::Error>> {
    let mut options = LedMatrixOptions::new();
    options.set_rows(panel.rows);
    options.set_cols(panel.cols);
    options.set_hardware_mapping("adafruit-hat");

    // PWM settings — matched to standalone video_player.rs which has stable output
    options.set_pwm_bits(8)?; // Full 8-bit color depth
    options.set_pwm_lsb_nanoseconds(130); // Stable timing (~143Hz refresh)

    let mut rt_options = LedRuntimeOptions::new();
    rt_options.set_gpio_slowdown(2); // Pi Zero 2 W requires slowdown=2

    // LedMatrix::new returns Result, so we can use ? directly
    // to propagate any errors upward.
    let matrix = LedMatrix::new(Some(options), Some(rt_options))?;

    Ok(matrix)
}

/// Set up a Ctrl+C handler that sets `running` to false.
///
/// # Rust concept: Arc and AtomicBool
/// We need to share the `running` flag between the main loop and the
/// signal handler. `Arc` (Atomic Reference Counting) lets multiple owners
/// share data. `AtomicBool` is a thread-safe boolean — no mutex needed
/// for a single bool.
pub fn setup_signal_handler() -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone(); // Clone the Arc, not the bool — both point to same data

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    running
}

/// Check if the main loop should keep running.
///
/// # Rust concept: Ordering
/// `Ordering::SeqCst` (Sequentially Consistent) is the strongest memory
/// ordering — guarantees all threads see writes in the same order.
/// For a simple "should I stop?" flag, it's the safe default.
pub fn is_running(running: &AtomicBool) -> bool {
    running.load(Ordering::SeqCst)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    // ── PanelConfig tests ──────────────────────────────────────────

    #[test]
    fn panel_config_default_is_64x64() {
        let panel = PanelConfig::default();
        assert_eq!(panel.rows, 64);
        assert_eq!(panel.cols, 64);
    }

    #[rstest]
    #[case(64, 64, 12288)]
    #[case(32, 32, 3072)]
    #[case(128, 64, 24576)]
    #[case(32, 64, 6144)]
    fn test_frame_byte_count(#[case] rows: u32, #[case] cols: u32, #[case] expected: usize) {
        assert_eq!(PanelConfig::new(rows, cols).frame_byte_count(), expected);
    }

    #[rstest]
    #[case(64, 64, 4096)]
    #[case(32, 32, 1024)]
    #[case(128, 64, 8192)]
    fn test_pixel_count(#[case] rows: u32, #[case] cols: u32, #[case] expected: u32) {
        assert_eq!(PanelConfig::new(rows, cols).pixel_count(), expected);
    }

    // ── Color tests ────────────────────────────────────────────────

    #[test]
    fn color_new() {
        let c = Color::new(10, 20, 30);
        assert_eq!(c.r, 10);
        assert_eq!(c.g, 20);
        assert_eq!(c.b, 30);
    }

    #[rstest]
    #[case(0, 255, 0, 0)] // Red
    #[case(60, 255, 255, 0)] // Yellow
    #[case(120, 0, 255, 0)] // Green
    #[case(180, 0, 255, 255)] // Cyan
    #[case(240, 0, 0, 255)] // Blue
    #[case(300, 255, 0, 255)] // Magenta
    fn test_color_from_hue_primary(#[case] hue: u16, #[case] r: u8, #[case] g: u8, #[case] b: u8) {
        let c = Color::from_hue(hue);
        assert_eq!(c, Color::new(r, g, b));
    }

    #[test]
    fn color_from_hue_wraps_at_360() {
        assert_eq!(Color::from_hue(0), Color::from_hue(360));
        assert_eq!(Color::from_hue(90), Color::from_hue(450));
    }

    #[test]
    fn apply_brightness_100_is_identity() {
        let c = Color::new(100, 200, 50);
        assert_eq!(c.apply_brightness(100), c);
    }

    #[test]
    fn apply_brightness_above_100_is_identity() {
        let c = Color::new(100, 200, 50);
        assert_eq!(c.apply_brightness(255), c);
    }

    #[test]
    fn apply_brightness_0_is_black() {
        let c = Color::new(255, 255, 255);
        assert_eq!(c.apply_brightness(0), Color::new(0, 0, 0));
    }

    #[test]
    fn apply_brightness_50_halves() {
        let c = Color::new(200, 100, 50);
        let dimmed = c.apply_brightness(50);
        assert_eq!(dimmed, Color::new(100, 50, 25));
    }

    // ── Backward-compatible helper tests ───────────────────────────

    #[test]
    fn color_helper_creates_color() {
        assert_eq!(color(1, 2, 3), Color::new(1, 2, 3));
    }

    #[test]
    fn color_from_hue_helper_delegates() {
        assert_eq!(color_from_hue(120), Color::from_hue(120));
    }
}
