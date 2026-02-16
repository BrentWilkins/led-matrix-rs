//! Media discovery: scan directories for available images and videos.
//!
//! ## Rust concepts
//! - `fs::read_dir()` for directory traversal
//! - `Path` and `PathBuf` for cross-platform file paths
//! - `serde::Serialize` for automatic JSON conversion
//! - Collecting iterators into `Vec`

use serde::Serialize;
use std::fs;
use std::path::Path;

/// Information about a single media file.
#[derive(Serialize, utoipa::ToSchema)]
pub struct MediaEntry {
    /// Filename (e.g., "sunset.png")
    pub name: String,
    /// Relative path from media dir (e.g., "images/sunset.png")
    pub path: String,
    /// File size in bytes
    pub size: u64,
}

/// Information about a video directory (folder of frame images).
#[derive(Serialize, utoipa::ToSchema)]
pub struct VideoEntry {
    /// Directory name (e.g., "flame")
    pub name: String,
    /// Relative path from media dir (e.g., "videos/flame")
    pub path: String,
    /// Number of frame files in the directory
    pub frame_count: usize,
}

/// Scan the images directory for PNG and JPEG files.
pub fn list_images(media_dir: &Path) -> Vec<MediaEntry> {
    let images_dir = media_dir.join("images");
    let mut entries = Vec::new();

    let read_dir = match fs::read_dir(&images_dir) {
        Ok(rd) => rd,
        Err(_) => return entries,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "png" | "jpg" | "jpeg" | "gif" | "bmp"));

        if is_image {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let rel_path = format!("images/{name}");

            entries.push(MediaEntry {
                name,
                path: rel_path,
                size,
            });
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Scan the videos directory for subdirectories containing frame images.
///
/// Each video is a directory of sequentially-numbered frame images
/// (e.g., `videos/flame/frame_0001.jpg`).
pub fn list_videos(media_dir: &Path) -> Vec<VideoEntry> {
    let videos_dir = media_dir.join("videos");
    let mut entries = Vec::new();

    let read_dir = match fs::read_dir(&videos_dir) {
        Ok(rd) => rd,
        Err(_) => return entries,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Count image files in this subdirectory
        let frame_count = fs::read_dir(&path)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| {
                        e.path()
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| matches!(ext, "png" | "jpg" | "jpeg"))
                    })
                    .count()
            })
            .unwrap_or(0);

        if frame_count > 0 {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let rel_path = format!("videos/{name}");

            entries.push(VideoEntry {
                name,
                path: rel_path,
                frame_count,
            });
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Scan the fonts directory for available BDF fonts.
pub fn list_fonts(media_dir: &Path) -> Vec<String> {
    let fonts_dir = media_dir.join("fonts").join("bdf");
    let mut fonts = Vec::new();

    let read_dir = match fs::read_dir(&fonts_dir) {
        Ok(rd) => rd,
        Err(_) => return fonts,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_bdf = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "bdf");

        if is_bdf {
            // Return just the font name without .bdf extension
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                fonts.push(name.to_string());
            }
        }
    }

    fonts.sort();
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_file(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"fake").unwrap();
    }

    #[test]
    fn list_images_finds_supported_formats() {
        let tmp = TempDir::new().unwrap();
        let images_dir = tmp.path().join("images");
        std::fs::create_dir(&images_dir).unwrap();

        create_file(&images_dir, "photo.png");
        create_file(&images_dir, "pic.jpg");
        create_file(&images_dir, "shot.jpeg");
        create_file(&images_dir, "anim.gif");
        create_file(&images_dir, "raw.bmp");
        create_file(&images_dir, "readme.txt"); // should be excluded

        let entries = list_images(tmp.path());
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert_eq!(entries.len(), 5);
        assert!(names.contains(&"photo.png"));
        assert!(names.contains(&"pic.jpg"));
        assert!(names.contains(&"shot.jpeg"));
        assert!(names.contains(&"anim.gif"));
        assert!(names.contains(&"raw.bmp"));
    }

    #[test]
    fn list_images_returns_empty_when_no_dir() {
        let tmp = TempDir::new().unwrap();
        let entries = list_images(tmp.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn list_images_sorted_alphabetically() {
        let tmp = TempDir::new().unwrap();
        let images_dir = tmp.path().join("images");
        std::fs::create_dir(&images_dir).unwrap();

        create_file(&images_dir, "zebra.png");
        create_file(&images_dir, "apple.png");
        create_file(&images_dir, "mango.jpg");

        let entries = list_images(tmp.path());
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["apple.png", "mango.jpg", "zebra.png"]);
    }

    #[test]
    fn list_videos_finds_directories_with_frames() {
        let tmp = TempDir::new().unwrap();
        let videos_dir = tmp.path().join("videos");
        let flame_dir = videos_dir.join("flame");
        std::fs::create_dir_all(&flame_dir).unwrap();

        create_file(&flame_dir, "frame_0001.jpg");
        create_file(&flame_dir, "frame_0002.jpg");
        create_file(&flame_dir, "frame_0003.png");

        let entries = list_videos(tmp.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "flame");
        assert_eq!(entries[0].frame_count, 3);
        assert_eq!(entries[0].path, "videos/flame");
    }

    #[test]
    fn list_videos_skips_empty_dirs() {
        let tmp = TempDir::new().unwrap();
        let videos_dir = tmp.path().join("videos");
        let empty_dir = videos_dir.join("empty");
        std::fs::create_dir_all(&empty_dir).unwrap();

        let entries = list_videos(tmp.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn list_fonts_finds_bdf_files() {
        let tmp = TempDir::new().unwrap();
        let fonts_dir = tmp.path().join("fonts").join("bdf");
        std::fs::create_dir_all(&fonts_dir).unwrap();

        create_file(&fonts_dir, "6x13.bdf");
        create_file(&fonts_dir, "9x18.bdf");
        create_file(&fonts_dir, "readme.txt"); // should be excluded

        let fonts = list_fonts(tmp.path());
        assert_eq!(fonts, vec!["6x13", "9x18"]);
    }

    #[test]
    fn list_fonts_returns_empty_when_no_dir() {
        let tmp = TempDir::new().unwrap();
        let fonts = list_fonts(tmp.path());
        assert!(fonts.is_empty());
    }
}
