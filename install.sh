#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_WEB_DIR="$PROJECT_DIR/rust/octocam-web"
HOMEKIT_DIR="$PROJECT_DIR/homekit"
SERVICE_TEMPLATE="$PROJECT_DIR/systemd/octocam-web.service"
SERVICE_FILE="/etc/systemd/system/octocam-web.service"
HOMEKIT_SERVICE_TEMPLATE="$PROJECT_DIR/systemd/octocam-homekit.service"
HOMEKIT_SERVICE_FILE="/etc/systemd/system/octocam-homekit.service"
WIFI_SERVICE_TEMPLATE="$PROJECT_DIR/systemd/octocam-wifi-setup.service"
WIFI_SERVICE_FILE="/etc/systemd/system/octocam-wifi-setup.service"
MATTER_SERVICE_TEMPLATE="$PROJECT_DIR/systemd/octocam-matter.service"
MATTER_SERVICE_FILE="/etc/systemd/system/octocam-matter.service"
MINIMIZE_SCRIPT="$PROJECT_DIR/scripts/minimize-os.sh"
SERVICE_USER="${SUDO_USER:-${USER}}"
SERVICE_GROUP="$(id -gn "$SERVICE_USER")"
STATE_DIR="/var/lib/octocam"
LOG_DIR="/var/log/octocam"
MATTER_STORAGE_DIR="$STATE_DIR/matter-storage"
SECRET_KEY_FILE="$STATE_DIR/secret-key"
WEB_BINARY="/usr/local/bin/octocam-web"
MINIMAL_OS=0

for arg in "$@"; do
  case "$arg" in
    --minimal-os)
      MINIMAL_OS=1
      ;;
    -h|--help)
      echo "Usage: sudo ./install.sh [--minimal-os]"
      exit 0
      ;;
    *)
      echo "Unknown option: $arg"
      echo "Usage: sudo ./install.sh [--minimal-os]"
      exit 1
      ;;
  esac
done

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run this installer with sudo: sudo ./install.sh"
  exit 1
fi

if [[ ! -f "$SERVICE_TEMPLATE" ]]; then
  echo "Missing service template: $SERVICE_TEMPLATE"
  exit 1
fi

if [[ ! -f "$WIFI_SERVICE_TEMPLATE" ]]; then
  echo "Missing Wi-Fi service template: $WIFI_SERVICE_TEMPLATE"
  exit 1
fi

if [[ ! -f "$MATTER_SERVICE_TEMPLATE" ]]; then
  echo "Missing Matter service template: $MATTER_SERVICE_TEMPLATE"
  exit 1
fi

if [[ "$MINIMAL_OS" -eq 1 ]]; then
  if [[ ! -x "$MINIMIZE_SCRIPT" ]]; then
    echo "Missing executable minimization script: $MINIMIZE_SCRIPT"
    exit 1
  fi

  "$MINIMIZE_SCRIPT"
fi

echo "Installing OctoCam dependencies..."
apt-get update
# octocam-matter runtime deps (GStreamer/avahi) land with the Plan-2 daemon build; see docs/matter.md
apt-get install -y --no-install-recommends ca-certificates curl build-essential pkg-config network-manager nodejs npm ffmpeg

if ! apt-get install -y --no-install-recommends rpicam-apps; then
  echo "rpicam-apps was unavailable; trying the older libcamera-apps package..."
  apt-get install -y --no-install-recommends libcamera-apps
fi

if [[ ! -f "$RUST_WEB_DIR/Cargo.toml" ]]; then
  echo "Missing Rust web app: $RUST_WEB_DIR/Cargo.toml"
  exit 1
fi

if [[ ! -f "$HOMEKIT_DIR/package.json" ]]; then
  echo "Missing HomeKit app: $HOMEKIT_DIR/package.json"
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Installing a minimal Rust toolchain..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

echo "Building OctoCam Rust web UI..."
cargo build --manifest-path "$RUST_WEB_DIR/Cargo.toml" --release --locked
install -m 0755 "$RUST_WEB_DIR/target/release/octocam-web" "$WEB_BINARY"

echo "Installing OctoCam HomeKit dependencies..."
npm ci --omit=dev --prefix "$HOMEKIT_DIR"

echo "Preparing OctoCam state directories..."
install -d -o "$SERVICE_USER" -g "$SERVICE_GROUP" "$STATE_DIR" "$LOG_DIR"

# Matter daemon runs sandboxed as its own user (never root: parses untrusted
# LAN traffic). Storage dir owns the CHIP KVS + status file.
if ! id -u octocam-matter >/dev/null 2>&1; then
  useradd --system --no-create-home --shell /usr/sbin/nologin octocam-matter
fi
install -d -o octocam-matter -g octocam-matter -m 750 "$MATTER_STORAGE_DIR"

if [[ ! -f "$SECRET_KEY_FILE" ]]; then
  umask 077
  SECRET_KEY="$(od -An -tx1 -N32 /dev/urandom)"
  SECRET_KEY="${SECRET_KEY//[[:space:]]/}"
  printf '%s\n' "$SECRET_KEY" > "$SECRET_KEY_FILE"
  chown "$SERVICE_USER:$SERVICE_GROUP" "$SECRET_KEY_FILE"
fi

if [[ ! -f "$STATE_DIR/settings.json" ]]; then
  cat > "$STATE_DIR/settings.json" <<'JSON'
{
  "admin_password_hash": "",
  "bitrate_kbps": 2500,
  "brightness": 0,
  "camera_enabled": true,
  "camera_label": "OctoCam",
  "contrast": 1.0,
  "device_name": "OctoCam",
  "framerate": 15,
  "hflip": false,
  "homekit_enabled": false,
  "homekit_paired": false,
  "motion_enabled": false,
  "motion_sensitivity": 50,
  "resolution_height": 720,
  "resolution_width": 1280,
  "room": "Living Room",
  "rotation": 0,
  "rtsp_enabled": true,
  "rtsp_max_clients": 1,
  "rtsp_path": "main",
  "setup_complete": false,
  "sub_bitrate_kbps": 600,
  "sub_framerate": 10,
  "sub_resolution_height": 480,
  "sub_resolution_width": 640,
  "sub_rtsp_max_clients": 2,
  "sub_rtsp_path": "sub",
  "sub_stream_enabled": true,
  "vflip": false,
  "wifi_ssid": ""
}
JSON
  chown "$SERVICE_USER:$SERVICE_GROUP" "$STATE_DIR/settings.json"
fi

echo "Installing systemd service..."
sed \
  -e "s|__PROJECT_DIR__|$PROJECT_DIR|g" \
  -e "s|__SERVICE_USER__|$SERVICE_USER|g" \
  "$SERVICE_TEMPLATE" > "$SERVICE_FILE"
chmod 0644 "$SERVICE_FILE"

sed \
  -e "s|__PROJECT_DIR__|$PROJECT_DIR|g" \
  -e "s|__SERVICE_USER__|$SERVICE_USER|g" \
  "$HOMEKIT_SERVICE_TEMPLATE" > "$HOMEKIT_SERVICE_FILE"
chmod 0644 "$HOMEKIT_SERVICE_FILE"

sed \
  -e "s|__PROJECT_DIR__|$PROJECT_DIR|g" \
  "$WIFI_SERVICE_TEMPLATE" > "$WIFI_SERVICE_FILE"
chmod 0644 "$WIFI_SERVICE_FILE"

sed \
  -e "s|__PROJECT_DIR__|$PROJECT_DIR|g" \
  "$MATTER_SERVICE_TEMPLATE" > "$MATTER_SERVICE_FILE"
chmod 0644 "$MATTER_SERVICE_FILE"

for group in video gpio i2c; do
  if getent group "$group" >/dev/null; then
    usermod -aG "$group" "$SERVICE_USER"
  fi
done

systemctl daemon-reload
systemctl enable octocam-wifi-setup.service
if ! systemctl start octocam-wifi-setup.service; then
  echo "OctoCam Wi-Fi setup AP did not start; continuing because it is only needed for headless fallback setup."
  echo "Run 'journalctl -xeu octocam-wifi-setup.service' on the Pi for details."
fi
systemctl enable --now octocam-web.service
if grep -q '"homekit_enabled": true' "$STATE_DIR/settings.json"; then
  systemctl enable --now octocam-homekit.service
else
  systemctl disable --now octocam-homekit.service >/dev/null 2>&1 || true
fi

if grep -q '"matter_enabled": true' /var/lib/octocam/settings.json 2>/dev/null; then
  systemctl enable octocam-matter.service
fi

echo "OctoCam web UI is installed."
echo "Open http://$(hostname).local:8080 or http://<device-ip>:8080"
