#!/bin/bash
# preprocess_video.sh - Extract video frames for LED matrix playback
#
# This script uses ffmpeg to convert any video format into a sequence of
# square image frames that can be played back on the LED matrix.
#
# Usage: ./preprocess_video.sh <input_video> <output_dir> [format] [fps] [size]
#   format: jpeg (default) or png
#   fps: frame rate (default: 30)
#   size: output dimension in pixels (default: 64, e.g. 32 for 32x32)
#
# Examples:
#   ./preprocess_video.sh myvideo.mp4 videos/myvideo
#   ./preprocess_video.sh myvideo.mp4 videos/myvideo png 24
#   ./preprocess_video.sh myvideo.mov videos/myvideo jpeg 30
#   ./preprocess_video.sh myvideo.mp4 videos/myvideo_32 jpeg 30 32

set -e  # Exit on error

usage() {
    echo "Usage: $0 <input_video> <output_dir> [format] [fps] [size]"
    echo "  format: jpeg (default) or png"
    echo "  fps: frame rate (default: 30)"
    echo "  size: output dimension (default: 64, produces size x size frames)"
    echo ""
    echo "Examples:"
    echo "  $0 video.mp4 videos/output"
    echo "  $0 video.mp4 videos/output png 24"
    echo "  $0 video.mov videos/output jpeg 30"
    echo "  $0 video.mp4 videos/output_32 jpeg 30 32"
    exit 1
}

# Check if ffmpeg is installed
if ! command -v ffmpeg &> /dev/null; then
    echo "Error: ffmpeg is not installed"
    echo "Install it with: brew install ffmpeg (macOS) or apt install ffmpeg (Linux)"
    exit 1
fi

# Parse arguments
INPUT="${1}"
OUTPUT_DIR="${2}"
FORMAT="${3:-jpeg}"
FPS="${4:-30}"
SIZE="${5:-64}"

# Validate required arguments
if [ -z "$INPUT" ] || [ -z "$OUTPUT_DIR" ]; then
    usage
fi

# Check if input file exists
if [ ! -f "$INPUT" ]; then
    echo "Error: input file '$INPUT' not found"
    exit 1
fi

# Validate format
if [[ "$FORMAT" != "jpeg" && "$FORMAT" != "png" ]]; then
    echo "Error: format must be 'jpeg' or 'png', got '$FORMAT'"
    usage
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

echo "Preprocessing video: $INPUT"
echo "Output directory: $OUTPUT_DIR"
echo "Format: $FORMAT"
echo "FPS: $FPS"
echo "Size: ${SIZE}x${SIZE}"
echo ""

# Extract frames
# -vf filter chain:
#   fps=$FPS - Set frame rate
#   crop - Crop to square (center-crop from longer dimension)
#   scale - Downsample to 64x64 with high-quality lanczos filter
# -q:v 2 - JPEG quality (2 = high quality, range 2-31)
# frame_%04d - Zero-padded 4-digit numbering (frame_0001.jpg, etc.)

# Crop to square keeping center, then scale to target size
# crop=min(iw,ih):min(iw,ih) - crop to square using smaller dimension
# The crop will be centered automatically when only w:h is specified
FILTER="fps=$FPS,crop='min(iw,ih)':'min(iw,ih)',scale=${SIZE}:${SIZE}:flags=lanczos"

if [[ "$FORMAT" == "jpeg" ]]; then
    echo "Extracting JPEG frames (center-cropped to ${SIZE}x${SIZE})..."
    ffmpeg -i "$INPUT" -vf "$FILTER" -pix_fmt yuvj420p -q:v 2 "$OUTPUT_DIR/frame_%04d.jpg"
else
    echo "Extracting PNG frames (center-cropped to ${SIZE}x${SIZE})..."
    ffmpeg -i "$INPUT" -vf "$FILTER" -pix_fmt rgb24 "$OUTPUT_DIR/frame_%04d.png"
fi

# Count frames
FRAME_COUNT=$(find "$OUTPUT_DIR" -name "frame_*" | wc -l | tr -d ' ')

echo ""
echo "✓ Extraction complete!"
echo "  Frames: $FRAME_COUNT"
echo "  Location: $OUTPUT_DIR"

# Calculate approximate duration
DURATION=$(echo "scale=1; $FRAME_COUNT / $FPS" | bc)
echo "  Duration: ~${DURATION}s at ${FPS} fps"

# Show disk usage
DISK_USAGE=$(du -sh "$OUTPUT_DIR" | cut -f1)
echo "  Disk usage: $DISK_USAGE"

echo ""
echo "Ready for playback! Run on the Pi:"
echo "  sudo ./target/release/examples/video_player $OUTPUT_DIR --fps $FPS --loop"
