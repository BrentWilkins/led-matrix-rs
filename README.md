# led-matrix-rs

HTTP API server for controlling RGB LED matrix panels on Raspberry Pi. Any device on the LAN can display images, play videos, scroll text, or stream raw frames.

**Hardware**: Pi Zero 2 W + Adafruit RGB Matrix Bonnet (PID 3211) + 64x64 2mm pitch panel (PID 5362).

## Quick Start

Download a pre-built binary from [GitHub Releases](https://github.com/BrentWilkins/led-matrix-rs/releases):

```sh
# On the Pi — download the armv7 binary (Pi Zero 2 W)
curl -L -o led-matrix-rs \
  https://github.com/BrentWilkins/led-matrix-rs/releases/latest/download/led-matrix-rs-armv7
chmod +x led-matrix-rs

# Run it
sudo ./led-matrix-rs --media-dir /path/to/media --port 8080
```

For Pi 4/5, use `led-matrix-rs-aarch64` instead.

## Development

### Cross-compile and deploy (recommended)

The `deploy.sh` script cross-compiles on your dev machine (macOS or Linux with Docker) and deploys to the Pi over SSH:

```sh
./scripts/deploy.sh pi        # "pi" is an SSH hostname from ~/.ssh/config
```

This uses [cross](https://github.com/cross-rs/cross) under the hood. Install it once with:

```sh
cargo install cross --git https://github.com/cross-rs/cross
```

### Build natively on the Pi

```sh
cargo build --release
sudo ./target/release/led-matrix-rs --media-dir . --port 8080
```

### Run tests

Tests run without hardware — the `rpi-led-matrix` dependency is feature-gated:

```sh
cargo test --no-default-features
cargo clippy --no-default-features -- -D warnings
cargo fmt --check
```

### Feature gating

The `hardware` feature (enabled by default) pulls in the `rpi-led-matrix` crate. Disable it for testing or linting on machines without the ARM toolchain:

```sh
cargo test --no-default-features     # tests
cargo clippy --no-default-features   # lint
```

## CLI Options

```text
led-matrix-rs [OPTIONS]

Options:
      --media-dir <PATH>    Root directory containing images/ and videos/ [default: .]
      --port <PORT>         Port to listen on [default: 8080]
      --fonts-dir <PATH>    Path to BDF font directory [default: fonts/bdf]
      --rows <N>            Number of rows on the LED panel [default: 64]
      --cols <N>            Number of columns on the LED panel [default: 64]
  -V, --version             Print version
  -h, --help                Print help
```

## API Endpoints

| Method | Path | Description |
| ------ | ---- | ----------- |
| `GET` | `/api/v1/status` | Current display state and version |
| `GET` | `/api/v1/images` | List available images |
| `GET` | `/api/v1/videos` | List available video directories |
| `POST` | `/api/v1/display/image` | Display an image |
| `POST` | `/api/v1/display/video` | Play a video (frame sequence) |
| `POST` | `/api/v1/display/text` | Scroll text across the display |
| `POST` | `/api/v1/display/frame` | Push raw RGB bytes (rows*cols*3) |
| `GET` | `/api/v1/display/stream` | WebSocket for streaming raw RGB frames |
| `POST` | `/api/v1/display/clear` | Clear the display |
| `POST` | `/api/v1/display/stop` | Stop current playback |
| `POST` | `/api/v1/brightness` | Set brightness (0-100) |

Interactive API docs are available at `/docs` (Swagger UI).

### Example Requests

```sh
# Check status
curl http://pi:8080/api/v1/status

# List available images
curl http://pi:8080/api/v1/images

# Display an image
curl -X POST -H 'Content-Type: application/json' \
  -d '{"path":"images/test.png"}' \
  http://pi:8080/api/v1/display/image

# Display the first available image (using jq)
curl -X POST -H 'Content-Type: application/json' \
  -d "{\"path\":\"$(curl -s http://pi:8080/api/v1/images | jq -r '.[0].path')\"}" \
  http://pi:8080/api/v1/display/image

# Play a video at 30fps, looping
curl -X POST -H 'Content-Type: application/json' \
  -d '{"path":"videos/eyes_25","fps":25,"loop":true}' \
  http://pi:8080/api/v1/display/video

# Scroll text
curl -X POST -H 'Content-Type: application/json' \
  -d '{"text":"Hello!","font":"6x13","color":[255,0,0],"speed":30}' \
  http://pi:8080/api/v1/display/text

# Set brightness to 50%
curl -X POST -H 'Content-Type: application/json' \
  -d '{"value":50}' \
  http://pi:8080/api/v1/brightness

# Stop playback
curl -X POST http://pi:8080/api/v1/display/stop

# Clear display
curl -X POST http://pi:8080/api/v1/display/clear
```

## Python Scripts

Helper scripts for video streaming and preprocessing. Install dependencies with [uv](https://docs.astral.sh/uv/) (recommended) or pip:

```sh
uv venv && source .venv/bin/activate && uv pip install websockets rich typer
# or: pip install websockets rich typer
```

### WebSocket Video Streaming

Stream video from your laptop to the matrix in real time. The laptop decodes the video with `ffmpeg` and sends raw RGB frames over a WebSocket.

```sh
# Stream a video file
python scripts/stream-video.py video.mp4 ws://pi:8080/api/v1/display/stream

# Loop playback
python scripts/stream-video.py video.mp4 ws://pi:8080/api/v1/display/stream --loop

# Override fps and buffer size
python scripts/stream-video.py video.mp4 ws://pi:8080/api/v1/display/stream --fps 24 --buffer 60

# 32x32 panel
python scripts/stream-video.py video.mp4 ws://pi:8080/api/v1/display/stream --size 32

# See all options
python scripts/stream-video.py --help
```

The script auto-detects the video's native framerate via `ffprobe` and paces output accordingly. Frames are decoded in a background thread and buffered (default 30 frames) to prevent pauses. Ctrl+C exits cleanly. Requires `ffmpeg` and `ffprobe` on PATH.

### Video Preprocessing

Videos played via the `/api/v1/display/video` endpoint must be pre-extracted into frame sequences:

```sh
./scripts/preprocess_video.sh input.mp4 videos/output jpeg 30
./scripts/preprocess_video.sh input.mp4 videos/output_32 jpeg 30 32  # 32x32
```

## Deployment

### systemd Service

```sh
sudo cp scripts/led-matrix.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable led-matrix
sudo systemctl start led-matrix
```

Edit `scripts/led-matrix.service` to adjust paths or port before copying.

**After code changes, rebuild and restart:**

```sh
# Cross-compile and deploy in one step
./scripts/deploy.sh pi

# Or manually on the Pi
cargo build --release
sudo systemctl restart led-matrix
```

**View logs:**

```sh
journalctl -u led-matrix -f           # Follow in real-time
journalctl -u led-matrix -n 100       # Last 100 lines
journalctl -u led-matrix -b           # Since last boot
journalctl -u led-matrix --since "1 hour ago"
```

**Log rotation:** systemd/journald handles rotation automatically. Logs are stored in `/var/log/journal/`. Check disk usage with `journalctl --disk-usage`.

To configure rotation limits, edit `/etc/systemd/journald.conf`:

```ini
[Journal]
SystemMaxUse=100M      # Max disk space for all logs
SystemMaxFileSize=10M  # Max size per log file
MaxRetentionSec=1week  # Keep logs for 1 week
```

Then restart journald: `sudo systemctl restart systemd-journald`

### CI/CD

- **CI** (every push/PR): runs tests, clippy, fmt check, and `cross check` for ARM targets
- **Release** (on `v*` tags): builds release binaries for armv7 and aarch64, uploads to GitHub Releases

## Standalone Examples

Examples can be run individually without the server:

```sh
cargo build --release --example minimal
sudo ./target/release/examples/minimal
```

Ctrl+C to exit cleanly.

| Example | Phase | Description |
| ------- | ----- | ----------- |
| `minimal` | 1 | Pixels, lines, circles, double-buffering |
| `pulsing` | 1 | Rainbow color cycling with brightness pulse |
| `image_viewer` | 2 | Load and display a static image |
| `video_player` | 2 | Play pre-extracted video frames |

## Media Setup

Place media files in the directory passed to `--media-dir`:

```text
media-dir/
├── images/          # PNG, JPEG files
│   └── sunset.png
├── videos/          # Directories of frame sequences
│   └── flame/
│       ├── frame_0001.jpg
│       ├── frame_0002.jpg
│       └── ...
└── fonts/
    └── bdf/         # BDF font files for text scrolling
```

### Demo Videos

Two demo video frame sequences are included (from [Pexels](https://www.pexels.com/video/close-up-video-of-a-woman-eyes-7322712/), see `videos/CREDITS.md`):

- `videos/eyes_25/` -- 64x64, 235 frames @ 25fps
- `videos/eyes_25_32x32/` -- 32x32, 235 frames @ 25fps

## Hardware

- **Board**: Pi Zero 2 W
- **Bonnet**: Adafruit RGB Matrix Bonnet (PID 3211)
- **Panel**: 64x64 2mm pitch (PID 5362)
- **GPIO access**: requires `sudo`
