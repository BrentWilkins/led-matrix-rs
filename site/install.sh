#!/bin/sh
# LED Matrix RS Installer
# Download and install led-matrix-rs from GitHub releases

set -u

# Constants
APP_NAME="led-matrix-rs"
REPO_OWNER="BrentWilkins"
REPO_NAME="led-matrix-rs"
GITHUB_BASE_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}"

# Environment variables with defaults
LED_MATRIX_VERSION="${LED_MATRIX_VERSION:-latest}"
LED_MATRIX_INSTALL_DIR="${LED_MATRIX_INSTALL_DIR:-}"
LED_MATRIX_GITHUB_TOKEN="${LED_MATRIX_GITHUB_TOKEN:-}"
LED_MATRIX_VERBOSE="${LED_MATRIX_VERBOSE:-0}"
LED_MATRIX_QUIET="${LED_MATRIX_QUIET:-0}"

# Parse command-line flags
INSTALL_SYSTEMD="auto"

while [ $# -gt 0 ]; do
    case "$1" in
    --help | -h)
        cat <<EOF
LED Matrix RS Installer

USAGE:
    curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sh
    curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sudo sh

OPTIONS:
    --help, -h              Show this help message
    --version VERSION       Install specific version (default: latest)
    --no-systemd            Skip systemd service installation
    --verbose, -v           Verbose output
    --quiet, -q             Quiet output

ENVIRONMENT VARIABLES:
    LED_MATRIX_VERSION          Version to install (default: latest)
    LED_MATRIX_INSTALL_DIR      Override install location
    LED_MATRIX_GITHUB_TOKEN     GitHub token for authenticated downloads
    LED_MATRIX_VERBOSE          Verbose output (0/1)
    LED_MATRIX_QUIET            Quiet output (0/1)

EXAMPLES:
    # User install (no sudo, installs to ~/.local/bin)
    curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sh

    # System install with systemd service (auto-enabled)
    curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sudo sh

    # System install without systemd
    curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sudo sh -s -- --no-systemd

    # Install specific version
    curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sh -s -- --version v0.1.1

EOF
        exit 0
        ;;
    --version)
        LED_MATRIX_VERSION="$2"
        shift 2
        ;;
    --no-systemd)
        INSTALL_SYSTEMD="no"
        shift
        ;;
    --verbose | -v)
        LED_MATRIX_VERBOSE=1
        shift
        ;;
    --quiet | -q)
        LED_MATRIX_QUIET=1
        shift
        ;;
    *)
        err "Unknown option: $1"
        ;;
    esac
done

# Helper functions
say() {
    if [ "$LED_MATRIX_QUIET" = "0" ]; then
        printf 'install.sh: %s\n' "$1" >&2
    fi
}

say_verbose() {
    if [ "$LED_MATRIX_VERBOSE" = "1" ]; then
        say "$1"
    fi
}

warn() {
    if [ "$LED_MATRIX_QUIET" = "0" ]; then
        printf 'install.sh: WARNING: %s\n' "$1" >&2
    fi
}

err() {
    if [ "$LED_MATRIX_QUIET" = "0" ]; then
        printf 'install.sh: ERROR: %s\n' "$1" >&2
    fi
    exit 1
}

need_cmd() {
    if ! check_cmd "$1"; then
        err "need '$1' (command not found)"
    fi
}

check_cmd() {
    command -v "$1" >/dev/null 2>&1
}

assert_nz() {
    if [ -z "$1" ]; then
        err "assert_nz $2"
    fi
}

ensure() {
    if ! "$@"; then
        err "command failed: $*"
    fi
}

ignore() {
    "$@" || true
}

# Download with curl or wget fallback
downloader() {
    _url="$1"
    _file="$2"

    if check_cmd curl; then
        say_verbose "Downloading with curl: $_url"
        if [ -n "$LED_MATRIX_GITHUB_TOKEN" ]; then
            curl --proto '=https' --tlsv1.2 -fL -H "Authorization: token $LED_MATRIX_GITHUB_TOKEN" "$_url" -o "$_file"
        else
            curl --proto '=https' --tlsv1.2 -fL "$_url" -o "$_file"
        fi
    elif check_cmd wget; then
        say_verbose "Downloading with wget: $_url"
        if [ -n "$LED_MATRIX_GITHUB_TOKEN" ]; then
            wget --https-only --header="Authorization: token $LED_MATRIX_GITHUB_TOKEN" "$_url" -O "$_file"
        else
            wget --https-only "$_url" -O "$_file"
        fi
    else
        err "need 'curl' or 'wget' (command not found)"
    fi
}

# Architecture detection
get_architecture() {
    _ostype=""
    _cputype=""

    _ostype="$(uname -s)"
    _cputype="$(uname -m)"

    say_verbose "Detected OS: $_ostype"
    say_verbose "Detected CPU: $_cputype"

    # Only support Linux (Raspberry Pi OS)
    if [ "$_ostype" != "Linux" ]; then
        err "This installer only supports Linux (Raspberry Pi OS). Detected: $_ostype"
    fi

    # Map CPU architecture to binary name
    case "$_cputype" in
    armv6l)
        echo "armv6"
        ;;
    armv7l | armv8l)
        echo "armv7"
        ;;
    aarch64 | arm64)
        echo "aarch64"
        ;;
    *)
        err "Unsupported architecture: $_cputype. This installer supports: armv6l, armv7l, aarch64"
        ;;
    esac
}

# Auto-detect sudo and determine install directory
get_install_dir() {
    # Check if user overrode install directory
    if [ -n "$LED_MATRIX_INSTALL_DIR" ]; then
        echo "$LED_MATRIX_INSTALL_DIR"
        return
    fi

    # Auto-detect based on running as root
    if [ "$(id -u)" = "0" ]; then
        # Running as root - install to system location
        echo "/usr/local/bin"
    else
        # Running as user - install to user location
        echo "${HOME}/.local/bin"
    fi
}

# Check if running with sudo (for systemd service installation)
is_root() {
    [ "$(id -u)" = "0" ]
}

# Download binary from GitHub releases
download_binary() {
    _arch="$1"
    _version="$2"
    _tmpdir=""
    _tmpfile=""
    _url=""

    say "Downloading ${APP_NAME} ${_version} for ${_arch}..."

    # Create temp directory
    _tmpdir="$(mktemp -d)"
    _tmpfile="${_tmpdir}/${APP_NAME}"

    # Construct download URL
    if [ "$_version" = "latest" ]; then
        _url="${GITHUB_BASE_URL}/releases/latest/download/${APP_NAME}-${_arch}"
    else
        _url="${GITHUB_BASE_URL}/releases/download/${_version}/${APP_NAME}-${_arch}"
    fi

    say_verbose "Download URL: $_url"

    # Download
    if ! downloader "$_url" "$_tmpfile"; then
        rm -rf "$_tmpdir"
        err "Failed to download binary from $_url"
    fi

    # Verify download
    if [ ! -f "$_tmpfile" ]; then
        rm -rf "$_tmpdir"
        err "Downloaded file not found: $_tmpfile"
    fi

    # Return temp file path
    echo "$_tmpfile"
}

# Install binary to target directory
install_binary() {
    _binary="$1"
    _install_dir="$2"
    _target="${_install_dir}/${APP_NAME}"

    say "Installing to ${_target}..."

    # Create install directory if needed
    if [ ! -d "$_install_dir" ]; then
        say_verbose "Creating directory: $_install_dir"
        if is_root; then
            ensure mkdir -p "$_install_dir"
        else
            ensure mkdir -p "$_install_dir"
        fi
    fi

    # Move binary and make executable
    ensure mv "$_binary" "$_target"
    ensure chmod +x "$_target"

    # Verify installation
    if ! "$_target" --version >/dev/null 2>&1; then
        warn "Binary installed but --version check failed"
    fi

    say "Successfully installed ${APP_NAME} to ${_target}"
}

# Install systemd service
install_systemd_service() {
    _service_file="/etc/systemd/system/led-matrix.service"

    say "Installing systemd service..."

    # Create service file
    cat >"$_service_file" <<'EOF'
[Unit]
Description=LED Matrix HTTP Server
After=network.target

[Service]
ExecStart=/usr/local/bin/led-matrix-rs --media-dir /home/pi/led-matrix --port 8080
WorkingDirectory=/home/pi
Restart=on-failure
RestartSec=5
User=root
Environment="RUST_LOG=info"
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

    # Reload systemd
    ensure systemctl daemon-reload

    say "Installed systemd service: $_service_file"
}

# Print success message with next steps
print_success() {
    _install_dir="$1"
    _installed_systemd="$2"

    echo ""
    say "Installation complete!"
    echo ""

    if [ "$_installed_systemd" = "yes" ]; then
        cat <<EOF
Successfully installed ${APP_NAME} to ${_install_dir}/${APP_NAME}
Installed systemd service: /etc/systemd/system/led-matrix.service

Next steps:
  1. Customize service (optional):
     sudo nano /etc/systemd/system/led-matrix.service
     # Edit --media-dir, --port, etc.

  2. Enable and start the service:
     sudo systemctl enable led-matrix
     sudo systemctl start led-matrix

  3. Check status:
     sudo systemctl status led-matrix

  4. View logs:
     journalctl -u led-matrix -f

EOF
    elif is_root; then
        cat <<EOF
Successfully installed ${APP_NAME} to ${_install_dir}/${APP_NAME}

Next steps:
  1. Run the server manually:
     sudo ${APP_NAME} --media-dir /path/to/media --port 8080

  2. Test the API:
     curl http://localhost:8080/api/v1/status

  3. To install systemd service later, re-run without --no-systemd:
     curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sudo sh

EOF
    else
        cat <<EOF
Successfully installed ${APP_NAME} to ${_install_dir}/${APP_NAME}

Next steps:
  1. Add to PATH (if not already):
     echo 'export PATH="\${HOME}/.local/bin:\${PATH}"' >> ~/.bashrc
     source ~/.bashrc

  2. Run the server (requires sudo for GPIO access):
     sudo ${_install_dir}/${APP_NAME} --media-dir /path/to/media --port 8080

  3. Test the API:
     curl http://localhost:8080/api/v1/status

  4. For system-wide install with systemd service, re-run with sudo:
     curl -fsSL https://brentwilkins.github.io/led-matrix-rs/install.sh | sudo sh

EOF
    fi
}

# Main installation
main() {
    _arch=""
    _install_dir=""
    _binary=""
    _installed_systemd="no"

    # Validate dependencies
    need_cmd uname
    need_cmd mktemp
    need_cmd chmod
    need_cmd mkdir
    need_cmd mv
    need_cmd rm

    # Detect architecture
    _arch="$(get_architecture)" || exit 1
    say_verbose "Architecture: $_arch"

    # Determine install directory
    _install_dir="$(get_install_dir)" || exit 1
    say_verbose "Install directory: $_install_dir"

    # Download binary
    _binary="$(download_binary "$_arch" "$LED_MATRIX_VERSION")" || exit 1

    # Install binary
    install_binary "$_binary" "$_install_dir"

    # Clean up temp directory
    ignore rm -rf "$(dirname "$_binary")"

    # Install systemd service if running as root and not disabled
    if is_root && [ "$INSTALL_SYSTEMD" != "no" ]; then
        install_systemd_service
        _installed_systemd="yes"
    fi

    # Print success message
    print_success "$_install_dir" "$_installed_systemd"
}

# Run main only if executed directly (not sourced)
case "${0}" in
*install.sh) main ;;
esac
