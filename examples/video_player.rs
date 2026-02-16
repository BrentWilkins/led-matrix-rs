//! # Video Player Example
//!
//! Plays pre-extracted video frames on the LED matrix.
//!
//! ## Run it
//! ```sh
//! cargo build --release --example video_player
//! sudo ./target/release/examples/video_player videos/myvideo --fps 30 --loop
//! ```

#[cfg(not(feature = "hardware"))]
fn main() {
    eprintln!("This example requires the 'hardware' feature.");
}

#[cfg(feature = "hardware")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use clap::Parser;
    use image::ImageReader;
    use image::RgbImage;
    use led_matrix_rs::{PanelConfig, color, create_matrix, is_running, setup_signal_handler};
    use rpi_led_matrix::LedCanvas;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;

    #[derive(Parser)]
    #[command(name = "video_player")]
    #[command(about = "Play pre-extracted video frames on the LED matrix")]
    struct Args {
        /// Directory containing frame images
        frames_dir: PathBuf,
        /// Frame rate (frames per second)
        #[arg(short, long, default_value = "30")]
        fps: u32,
        /// Loop playback indefinitely
        #[arg(short, long)]
        loop_playback: bool,
    }

    const PRELOAD_THRESHOLD: usize = 900;

    fn load_frame_paths(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
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

    fn load_frame(path: &Path) -> Result<RgbImage, Box<dyn std::error::Error>> {
        let img = ImageReader::open(path)?.decode()?.to_rgb8();
        Ok(img)
    }

    fn draw_frame_to_canvas(canvas: &mut LedCanvas, img: &RgbImage) {
        for (x, y, pixel) in img.enumerate_pixels() {
            let led_color = color(pixel[0], pixel[1], pixel[2]);
            canvas.set(x as i32, y as i32, &led_color.into());
        }
    }

    let args = Args::parse();
    if args.fps == 0 {
        return Err("FPS must be greater than 0".into());
    }

    let panel = PanelConfig::default();
    let matrix = create_matrix(panel)?;
    let running = setup_signal_handler();
    let mut canvas = matrix.offscreen_canvas();

    println!("Scanning for frames in: {}", args.frames_dir.display());
    let frame_paths = load_frame_paths(&args.frames_dir)?;
    let frame_count = frame_paths.len();
    println!("Found {} frames", frame_count);

    let duration_secs = frame_count as f32 / args.fps as f32;
    println!("Video duration: {:.1}s at {} fps", duration_secs, args.fps);

    let use_preload = frame_count <= PRELOAD_THRESHOLD;

    let preloaded_frames: Option<Vec<RgbImage>> = if use_preload {
        println!("Pre-loading all {} frames into memory...", frame_count);
        let mut frames = Vec::with_capacity(frame_count);
        for (i, path) in frame_paths.iter().enumerate() {
            if i % 100 == 0 || i == frame_count - 1 {
                print!("\rLoading frame {}/{}...", i + 1, frame_count);
            }
            let img = load_frame(path)?;
            frames.push(img);
        }
        println!("\nAll frames loaded!");
        Some(frames)
    } else {
        println!(
            "Using streaming mode ({} > {} frames)",
            frame_count, PRELOAD_THRESHOLD
        );
        None
    };

    let frame_duration = Duration::from_millis(1000 / args.fps as u64);
    let mut frame_index = 0;

    loop {
        if !is_running(&running) {
            break;
        }

        let current_frame = match &preloaded_frames {
            Some(frames) => &frames[frame_index],
            None => {
                let path = &frame_paths[frame_index];
                let img = load_frame(path)?;
                draw_frame_to_canvas(&mut canvas, &img);
                canvas = matrix.swap(canvas);
                frame_index += 1;
                if frame_index >= frame_count {
                    if args.loop_playback {
                        frame_index = 0;
                    } else {
                        break;
                    }
                }
                thread::sleep(frame_duration);
                continue;
            }
        };

        draw_frame_to_canvas(&mut canvas, current_frame);
        canvas = matrix.swap(canvas);

        frame_index += 1;
        if frame_index >= frame_count {
            if args.loop_playback {
                frame_index = 0;
                println!("Looping video...");
            } else {
                break;
            }
        }
        thread::sleep(frame_duration);
    }

    println!("\nPlayback stopped. Shutting down cleanly.");
    Ok(())
}
