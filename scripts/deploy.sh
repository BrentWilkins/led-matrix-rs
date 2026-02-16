#!/bin/bash
# Cross-compile and deploy to Pi (works on macOS or Linux with Docker + cross).
#
# Usage:
#   ./scripts/deploy.sh [pi-hostname]
#
# Defaults to "pi" as the SSH hostname. Configure in ~/.ssh/config:
#   Host pi
#     HostName 192.168.1.xxx
#     User pi
set -e

PI_HOST="${1:-pi}"
TARGET="armv7-unknown-linux-gnueabihf"
BINARY="led-matrix-rs"

echo "==> Cross-compiling for $TARGET..."
cross build --target "$TARGET" --release

echo "==> Deploying to $PI_HOST..."
scp "target/$TARGET/release/$BINARY" "$PI_HOST":~/led-matrix-rs/

echo "==> Restarting service..."
ssh "$PI_HOST" "sudo systemctl restart led-matrix"

echo "==> Done! Check logs with: ssh $PI_HOST journalctl -u led-matrix -f"
