#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVICE_TEMPLATE="$PROJECT_DIR/systemd/octocam-web.service"
SERVICE_FILE="/etc/systemd/system/octocam-web.service"
WIFI_SERVICE_TEMPLATE="$PROJECT_DIR/systemd/octocam-wifi-setup.service"
WIFI_SERVICE_FILE="/etc/systemd/system/octocam-wifi-setup.service"
MINIMIZE_SCRIPT="$PROJECT_DIR/scripts/minimize-os.sh"
SERVICE_USER="${SUDO_USER:-${USER}}"
SERVICE_GROUP="$(id -gn "$SERVICE_USER")"
STATE_DIR="/var/lib/octocam"
LOG_DIR="/var/log/octocam"
SECRET_KEY_FILE="$STATE_DIR/secret-key"
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

if [[ "$MINIMAL_OS" -eq 1 ]]; then
  if [[ ! -x "$MINIMIZE_SCRIPT" ]]; then
    echo "Missing executable minimization script: $MINIMIZE_SCRIPT"
    exit 1
  fi

  "$MINIMIZE_SCRIPT"
fi

echo "Installing OctoCam dependencies..."
apt-get update
apt-get install -y --no-install-recommends network-manager python3-flask python3-picamera2 python3-libcamera

if ! apt-get install -y --no-install-recommends rpicam-apps; then
  echo "rpicam-apps was unavailable; trying the older libcamera-apps package..."
  apt-get install -y --no-install-recommends libcamera-apps
fi

echo "Preparing OctoCam state directories..."
install -d -o "$SERVICE_USER" -g "$SERVICE_GROUP" "$STATE_DIR" "$LOG_DIR"

if [[ ! -f "$SECRET_KEY_FILE" ]]; then
  umask 077
  /usr/bin/python3 -c "import secrets; print(secrets.token_urlsafe(48))" > "$SECRET_KEY_FILE"
  chown "$SERVICE_USER:$SERVICE_GROUP" "$SECRET_KEY_FILE"
fi

if [[ ! -f "$STATE_DIR/settings.json" ]]; then
  cd "$PROJECT_DIR"
  sudo -u "$SERVICE_USER" PYTHONPATH="$PROJECT_DIR" OCTOCAM_CONFIG_PATH="$STATE_DIR/settings.json" \
    /usr/bin/python3 -c "from octocam.settings import DEFAULT_SETTINGS, save_settings; save_settings(DEFAULT_SETTINGS)"
fi

echo "Installing systemd service..."
sed \
  -e "s|__PROJECT_DIR__|$PROJECT_DIR|g" \
  -e "s|__SERVICE_USER__|$SERVICE_USER|g" \
  "$SERVICE_TEMPLATE" > "$SERVICE_FILE"
chmod 0644 "$SERVICE_FILE"

sed \
  -e "s|__PROJECT_DIR__|$PROJECT_DIR|g" \
  "$WIFI_SERVICE_TEMPLATE" > "$WIFI_SERVICE_FILE"
chmod 0644 "$WIFI_SERVICE_FILE"

for group in video gpio i2c; do
  if getent group "$group" >/dev/null; then
    usermod -aG "$group" "$SERVICE_USER"
  fi
done

systemctl daemon-reload
systemctl enable --now octocam-wifi-setup.service
systemctl enable --now octocam-web.service

echo "OctoCam web UI is installed."
echo "Open http://$(hostname).local:8080 or http://<device-ip>:8080"
