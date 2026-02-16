//! Render thread: owns the LED matrix and processes commands via a channel.
//!
//! The `rpi-led-matrix` C library is not thread-safe, so all matrix operations
//! happen on a single dedicated thread. The async HTTP server communicates
//! with this thread by sending `RenderCommand` values through an `mpsc` channel.
//!
//! ## Rust concepts
//! - `std::sync::mpsc` channels for thread communication
//! - `enum` with data variants (tagged unions)
//! - `Arc<Mutex<T>>` for shared mutable state
//! - `try_recv()` for non-blocking channel reads
//! - Loop labels (`'playback: loop`) for breaking out of nested loops

use crate::{Color, PanelConfig, color, create_matrix};
use image::imageops::FilterType;
use image::{ImageReader, RgbImage};
use rpi_led_matrix::{LedCanvas, LedFont};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ── Commands ─────────────────────────────────────────────────────────

/// Commands sent from the HTTP server to the render thread.
///
/// Rust concept: ENUMS WITH DATA
/// Unlike C enums (just numbers), Rust enums can carry data in each variant.
/// This is sometimes called a "tagged union" or "sum type". The compiler
/// ensures you handle every variant when pattern matching.
pub enum RenderCommand {
    /// Display a static image (path relative to media dir)
    ShowImage(PathBuf),
    /// Play a sequence of pre-extracted video frames
    PlayVideo {
        dir: PathBuf,
        fps: u32,
        loop_playback: bool,
    },
    /// Scroll text across the display
    ScrollText {
        text: String,
        font: String,
        color: (u8, u8, u8),
        speed: u32,
    },
    /// Display a raw RGB frame (rows*cols*3 bytes)
    ShowFrame(Vec<u8>),
    /// Clear the display (all pixels off)
    Clear,
    /// Stop current playback and go idle
    Stop,
    /// Set display brightness (0-100)
    SetBrightness(u8),
}

// ── Status ───────────────────────────────────────────────────────────

/// What the display is currently doing.
#[derive(Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplayState {
    Idle,
    ShowingImage,
    PlayingVideo,
    ScrollingText,
    Streaming,
}

/// Shared status that the HTTP server can read to report current state.
///
/// Rust concept: Arc<Mutex<T>>
/// `Arc` = atomic reference counting (shared ownership across threads)
/// `Mutex` = mutual exclusion (only one thread can access at a time)
/// Together they allow the render thread to update status while the
/// HTTP server reads it.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct DisplayStatus {
    /// Current display state
    pub state: DisplayState,
    /// Currently displayed media (if any)
    pub current_media: Option<String>,
    /// Current frame number (for videos)
    pub frame: Option<usize>,
    /// Total frame count (for videos)
    pub total_frames: Option<usize>,
    /// Current brightness (0-100)
    pub brightness: u8,
    /// Server version
    pub version: String,
}

impl DisplayStatus {
    pub fn new() -> Self {
        Self {
            state: DisplayState::Idle,
            current_media: None,
            frame: None,
            total_frames: None,
            brightness: 75,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn set_idle(&mut self) {
        self.state = DisplayState::Idle;
        self.current_media = None;
        self.frame = None;
        self.total_frames = None;
    }
}

// ── Helper functions (refactored from examples) ──────────────────────

/// Load an image from disk and resize it to the panel dimensions.
pub fn load_and_resize_image(
    path: &Path,
    panel: PanelConfig,
) -> Result<RgbImage, Box<dyn std::error::Error>> {
    let img = ImageReader::open(path)?.decode()?;
    let resized = img
        .resize_exact(panel.cols, panel.rows, FilterType::Lanczos3)
        .to_rgb8();
    Ok(resized)
}

/// Discover and sort all frame image files in a directory.
pub fn load_frame_paths(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut paths = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if let Some(ext) = path.extension() {
            let ext_str = ext.to_str().unwrap_or("");
            if ext_str == "jpg" || ext_str == "jpeg" || ext_str == "png" {
                paths.push(path);
            }
        }
    }

    paths.sort();

    if paths.is_empty() {
        return Err(format!("No image files found in {}", dir.display()).into());
    }

    Ok(paths)
}

/// Load a single frame image from disk (expected to match panel dimensions).
pub fn load_frame(path: &Path) -> Result<RgbImage, Box<dyn std::error::Error>> {
    let img = ImageReader::open(path)?.decode()?.to_rgb8();
    Ok(img)
}

/// Draw an RgbImage onto the LED canvas pixel by pixel.
pub fn draw_frame_to_canvas(canvas: &mut LedCanvas, img: &RgbImage) {
    for (x, y, pixel) in img.enumerate_pixels() {
        let led_color = color(pixel[0], pixel[1], pixel[2]);
        canvas.set(x as i32, y as i32, &led_color.into());
    }
}

// ── Brightness helpers ───────────────────────────────────────────────

/// Draw an image to canvas with brightness scaling applied.
fn draw_frame_with_brightness(canvas: &mut LedCanvas, img: &RgbImage, brightness: u8) {
    if brightness >= 100 {
        draw_frame_to_canvas(canvas, img);
    } else {
        for (x, y, pixel) in img.enumerate_pixels() {
            let c = Color::new(pixel[0], pixel[1], pixel[2]).apply_brightness(brightness);
            canvas.set(x as i32, y as i32, &c.into());
        }
    }
}

/// Draw raw RGB bytes to canvas with brightness scaling.
fn draw_raw_frame(canvas: &mut LedCanvas, data: &[u8], panel: PanelConfig, brightness: u8) {
    for y in 0..panel.rows {
        for x in 0..panel.cols {
            let offset = ((y * panel.cols + x) * 3) as usize;
            let c = Color::new(data[offset], data[offset + 1], data[offset + 2])
                .apply_brightness(brightness);
            canvas.set(x as i32, y as i32, &c.into());
        }
    }
}

/// Apply brightness to an entire image, returning a new image.
fn apply_brightness_to_image(img: &RgbImage, brightness: u8) -> RgbImage {
    if brightness >= 100 {
        return img.clone();
    }

    let mut result = img.clone();
    for pixel in result.pixels_mut() {
        let c = Color::new(pixel[0], pixel[1], pixel[2]).apply_brightness(brightness);
        pixel[0] = c.r;
        pixel[1] = c.g;
        pixel[2] = c.b;
    }
    result
}

// ── Render loop ──────────────────────────────────────────────────────

/// Main render loop — runs on a dedicated thread, owns the LED matrix.
///
/// This function never returns until the channel is closed (sender dropped).
/// It receives commands and executes them, updating shared status along the way.
///
/// ## Interrupt pattern
/// During long-running operations (video playback, text scrolling), we use
/// `try_recv()` between frames to check for new commands. If a new command
/// arrives, we store it in `pending_cmd` and break out of the playback loop.
/// The main loop then processes the pending command instead of blocking on
/// `recv()`.
pub fn render_loop(
    rx: Receiver<RenderCommand>,
    status: Arc<Mutex<DisplayStatus>>,
    fonts_dir: PathBuf,
    panel: PanelConfig,
) {
    // Initialize the matrix — if this fails, we can't do anything
    let matrix = match create_matrix(panel) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("Failed to initialize LED matrix: {}", e);
            return;
        }
    };

    let mut canvas = matrix.offscreen_canvas();

    // Shared brightness — can be updated without interrupting playback
    let brightness = Arc::new(Mutex::new(75u8));

    // Pending command — set when a playback loop is interrupted
    let mut pending_cmd: Option<RenderCommand> = None;

    tracing::info!("Render thread started, waiting for commands...");

    loop {
        // Get the next command: either a pending one or wait for a new one
        let cmd = if let Some(cmd) = pending_cmd.take() {
            cmd
        } else {
            match rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => {
                    tracing::info!("Render thread: channel closed, shutting down.");
                    break;
                }
            }
        };

        match cmd {
            RenderCommand::Clear => {
                canvas.clear();
                canvas = matrix.swap(canvas);
                status.lock().unwrap().set_idle();
            }

            RenderCommand::Stop => {
                status.lock().unwrap().set_idle();
            }

            RenderCommand::SetBrightness(value) => {
                let new_brightness = value.min(100);
                *brightness.lock().unwrap() = new_brightness;
                status.lock().unwrap().brightness = new_brightness;
            }

            RenderCommand::ShowImage(path) => {
                let path_str = path.display().to_string();
                {
                    let mut s = status.lock().unwrap();
                    s.state = DisplayState::ShowingImage;
                    s.current_media = Some(path_str.clone());
                    s.frame = None;
                    s.total_frames = None;
                }

                match load_and_resize_image(&path, panel) {
                    Ok(img) => {
                        let current_brightness = *brightness.lock().unwrap();
                        draw_frame_with_brightness(&mut canvas, &img, current_brightness);
                        canvas = matrix.swap(canvas);
                        tracing::info!("Displaying image: {}", path_str);
                    }
                    Err(e) => {
                        tracing::error!("Failed to load image {}: {}", path_str, e);
                        status.lock().unwrap().set_idle();
                    }
                }
            }

            RenderCommand::ShowFrame(data) => {
                let expected = panel.frame_byte_count();
                if data.len() == expected {
                    let current_brightness = *brightness.lock().unwrap();
                    draw_raw_frame(&mut canvas, &data, panel, current_brightness);
                    canvas = matrix.swap(canvas);
                } else {
                    tracing::error!(
                        "Invalid frame size: expected {} bytes, got {}",
                        expected,
                        data.len()
                    );
                }
            }

            RenderCommand::PlayVideo {
                dir,
                fps,
                loop_playback,
            } => {
                let dir_str = dir.display().to_string();

                let frame_paths = match load_frame_paths(&dir) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Failed to load video frames from {}: {}", dir_str, e);
                        continue;
                    }
                };

                // Get current brightness before loading frames
                let current_brightness = *brightness.lock().unwrap();

                // Pre-load all frames into memory with brightness pre-applied
                tracing::info!(
                    "Pre-loading {} frames from {} (brightness: {})...",
                    frame_paths.len(),
                    dir_str,
                    current_brightness
                );
                let mut frames: Vec<RgbImage> = Vec::new();
                for (i, path) in frame_paths.iter().enumerate() {
                    match load_frame(path) {
                        Ok(img) => {
                            // Pre-apply brightness to eliminate per-pixel math during playback
                            let adjusted = apply_brightness_to_image(&img, current_brightness);
                            frames.push(adjusted);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to pre-load frame {}: {}", i, e);
                            // Continue with frames we have
                        }
                    }
                }

                if frames.is_empty() {
                    tracing::error!("No frames loaded from {}", dir_str);
                    continue;
                }

                let frame_count = frames.len();
                let frame_duration = Duration::from_millis(1000 / fps.max(1) as u64);

                {
                    let mut s = status.lock().unwrap();
                    s.state = DisplayState::PlayingVideo;
                    s.current_media = Some(dir_str.clone());
                    s.frame = Some(0);
                    s.total_frames = Some(frame_count);
                }

                tracing::info!(
                    "Playing video: {} ({} frames @ {} fps)",
                    dir_str,
                    frame_count,
                    fps
                );

                let mut frame_index = 0;

                // Track frame timing for performance debugging
                let mut slow_frame_count = 0;
                let target_frame_time = frame_duration;

                'playback: loop {
                    let frame_start = std::time::Instant::now();

                    // Check for new commands (non-blocking)
                    if let Ok(new_cmd) = rx.try_recv() {
                        // Brightness changes won't affect current playback (already applied to frames)
                        match new_cmd {
                            RenderCommand::SetBrightness(value) => {
                                let new_brightness = value.min(100);
                                *brightness.lock().unwrap() = new_brightness;
                                status.lock().unwrap().brightness = new_brightness;
                                tracing::info!(
                                    "Brightness set to {} (will apply to next video)",
                                    new_brightness
                                );
                                // Continue playback with current frames
                            }
                            _ => {
                                // Any other command interrupts playback
                                pending_cmd = Some(new_cmd);
                                break 'playback;
                            }
                        }
                    }

                    // Draw frame from pre-loaded memory (brightness already applied)
                    let img = &frames[frame_index];

                    let draw_start = std::time::Instant::now();
                    draw_frame_to_canvas(&mut canvas, img);
                    let draw_time = draw_start.elapsed();

                    let swap_start = std::time::Instant::now();
                    canvas = matrix.swap(canvas);
                    let swap_time = swap_start.elapsed();

                    // Log timing details for first few frames
                    let frame_time = frame_start.elapsed();
                    if frame_index < 5 {
                        tracing::info!(
                            "Frame {}: draw={}µs swap={}µs total={}ms (img: {}x{})",
                            frame_index,
                            draw_time.as_micros(),
                            swap_time.as_micros(),
                            frame_time.as_millis(),
                            img.width(),
                            img.height()
                        );
                    }

                    // Log slow frames for performance debugging
                    if frame_time > target_frame_time {
                        slow_frame_count += 1;
                        if slow_frame_count <= 5 {
                            // Only log first 5 slow frames
                            tracing::warn!(
                                "Frame {} took {}ms (target: {}ms)",
                                frame_index,
                                frame_time.as_millis(),
                                target_frame_time.as_millis()
                            );
                        }
                    }

                    {
                        let mut s = status.lock().unwrap();
                        s.frame = Some(frame_index);
                    }

                    frame_index += 1;

                    if frame_index >= frame_count {
                        if loop_playback {
                            frame_index = 0;
                        } else {
                            // Clear display when non-looping video finishes
                            canvas.clear();
                            canvas = matrix.swap(canvas);
                            status.lock().unwrap().set_idle();
                            if slow_frame_count > 0 {
                                tracing::warn!(
                                    "Video finished with {} slow frames out of {}",
                                    slow_frame_count,
                                    frame_count
                                );
                            }
                            tracing::info!("Video playback finished");
                            break 'playback;
                        }
                    }

                    thread::sleep(frame_duration);
                }
            }

            RenderCommand::ScrollText {
                text,
                font: font_name,
                color: (r, g, b),
                speed,
            } => {
                let font_path = fonts_dir.join(format!("{font_name}.bdf"));
                let font = match LedFont::new(&font_path) {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::error!("Failed to load font {}: {}", font_path.display(), e);
                        continue;
                    }
                };

                {
                    let mut s = status.lock().unwrap();
                    s.state = DisplayState::ScrollingText;
                    s.current_media = Some(text.clone());
                    s.frame = None;
                    s.total_frames = None;
                }

                // Scroll from right edge to off the left side, then loop
                let text_width = (text.len() as i32) * 8;
                let start_x = panel.cols as i32;
                let end_x = -text_width;
                let y_pos = 40; // Roughly vertically centered
                let scroll_delay = Duration::from_millis(1000 / speed.max(1) as u64);

                let mut x = start_x;
                // Cache brightness locally to avoid mutex lock on every frame
                let mut current_brightness = *brightness.lock().unwrap();

                'scroll: loop {
                    // Check for new commands (non-blocking)
                    if let Ok(new_cmd) = rx.try_recv() {
                        // Allow brightness changes without interrupting scrolling
                        match new_cmd {
                            RenderCommand::SetBrightness(value) => {
                                current_brightness = value.min(100);
                                *brightness.lock().unwrap() = current_brightness;
                                status.lock().unwrap().brightness = current_brightness;
                                // Continue scrolling
                            }
                            _ => {
                                // Any other command interrupts scrolling
                                pending_cmd = Some(new_cmd);
                                break 'scroll;
                            }
                        }
                    }

                    // Calculate text color with current brightness
                    let text_color = Color::new(r, g, b).apply_brightness(current_brightness);

                    canvas.clear();
                    canvas.draw_text(&font, &text, x, y_pos, &text_color.into(), 0, false);
                    canvas = matrix.swap(canvas);

                    x -= 1;
                    if x < end_x {
                        x = start_x;
                    }

                    thread::sleep(scroll_delay);
                }
            }
        }
    }
}
