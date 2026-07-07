#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_SCRIPT="$PROJECT_DIR/scripts/build-pi-web.sh"
DIST_DIR="${OCTOCAM_PI_DIST_DIR:-$PROJECT_DIR/dist/pi}"
ARTIFACT="$DIST_DIR/octocam-web"
SSH_TARGET="${OCTOCAM_PI_SSH:-dietpi@192.168.2.211}"
REMOTE_DIR="${OCTOCAM_REMOTE_DIR:-/root/OctoCam}"
SERVICE_USER="${OCTOCAM_SERVICE_USER:-root}"
NGINX_SITE_FILE="/etc/nginx/sites-available/octocam-web"
NGINX_SITE_LINK="/etc/nginx/sites-enabled/octocam-web"
REMOTE_TMP="/tmp/octocam-web-deploy-$$"
SKIP_BUILD=0
LOCAL_TMP=""

cleanup() {
  if [[ -n "$LOCAL_TMP" ]]; then
    rm -rf "$LOCAL_TMP"
  fi
}
trap cleanup EXIT

usage() {
  cat <<USAGE
Usage: scripts/deploy-pi-web.sh [--skip-build] [user@host]

Builds the Raspberry Pi web binary on this Mac, copies it to the Pi, installs
it at /usr/local/bin/octocam-web, syncs static assets and systemd unit files,
then restarts octocam-web.

Arguments:
  user@host      SSH target, default: $SSH_TARGET

Options:
  --skip-build   Deploy the existing artifact at $ARTIFACT

Environment overrides:
  OCTOCAM_PI_SSH       SSH target, default: $SSH_TARGET
  OCTOCAM_REMOTE_DIR   Remote checkout, default: $REMOTE_DIR
  OCTOCAM_SERVICE_USER Remote systemd service user, default: $SERVICE_USER
  OCTOCAM_PI_DIST_DIR  Artifact directory, default: $DIST_DIR
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      SSH_TARGET="$1"
      shift
      ;;
  esac
done

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  "$BUILD_SCRIPT"
fi

if [[ ! -x "$ARTIFACT" ]]; then
  echo "Missing built Pi artifact: $ARTIFACT" >&2
  echo "Run scripts/build-pi-web.sh first, or omit --skip-build." >&2
  exit 1
fi

echo "Uploading OctoCam web UI to $SSH_TARGET..."
LOCAL_TMP="$(mktemp -d)"
sed \
  -e "s|__PROJECT_DIR__|$REMOTE_DIR|g" \
  -e "s|__SERVICE_USER__|$SERVICE_USER|g" \
  "$PROJECT_DIR/systemd/octocam-web.service" > "$LOCAL_TMP/octocam-web.service"
cp "$PROJECT_DIR/systemd/octocam-wifi-setup.service" "$LOCAL_TMP/octocam-wifi-setup.service"
cp "$PROJECT_DIR/nginx/octocam-web.conf" "$LOCAL_TMP/octocam-web.nginx.conf"
sed \
  -e "s|__PROJECT_DIR__|$REMOTE_DIR|g" \
  "$PROJECT_DIR/systemd/octocam-matter.service" > "$LOCAL_TMP/octocam-matter.service"

ssh "$SSH_TARGET" "mkdir -p '$REMOTE_TMP'"
rsync -az "$ARTIFACT" "$SSH_TARGET:$REMOTE_TMP/octocam-web"
rsync -az "$LOCAL_TMP/octocam-web.service" "$LOCAL_TMP/octocam-wifi-setup.service" "$LOCAL_TMP/octocam-matter.service" "$LOCAL_TMP/octocam-web.nginx.conf" "$SSH_TARGET:$REMOTE_TMP/"

ssh "$SSH_TARGET" "sudo -n mkdir -p '$REMOTE_DIR/static'"
rsync -az --delete \
  --exclude '._*' \
  --exclude '.DS_Store' \
  --rsync-path='sudo -n rsync' \
  "$PROJECT_DIR/static/" "$SSH_TARGET:$REMOTE_DIR/static/"

HEALTH_PORT="${OCTOCAM_PORT:-8080}"
ssh "$SSH_TARGET" "bash -lc 'set -euo pipefail
  # Back up the current binary so we can roll back if the new one is unhealthy.
  if [ -x /usr/local/bin/octocam-web ]; then
    sudo -n cp -f /usr/local/bin/octocam-web /usr/local/bin/octocam-web.bak
  fi
  sudo -n install -m 0755 '$REMOTE_TMP/octocam-web' /usr/local/bin/octocam-web
  sudo -n install -m 0644 '$REMOTE_TMP/octocam-web.service' /etc/systemd/system/octocam-web.service
  sudo -n install -m 0644 '$REMOTE_TMP/octocam-wifi-setup.service' /etc/systemd/system/octocam-wifi-setup.service
  sudo -n install -m 0644 '$REMOTE_TMP/octocam-matter.service' /etc/systemd/system/octocam-matter.service
  if ! command -v nginx >/dev/null 2>&1 || ! command -v openssl >/dev/null 2>&1; then
    sudo -n apt-get update
    sudo -n apt-get install -y --no-install-recommends nginx openssl
  fi
  sudo -n install -d -m 0755 /etc/octocam
  sudo -n install -d -m 0750 /etc/octocam/tls
  if [ ! -f /etc/octocam/tls/octocam.crt ] || [ ! -f /etc/octocam/tls/octocam.key ]; then
    sudo -n openssl req -x509 -newkey rsa:2048 -sha256 -days 825 -nodes \
      -keyout /etc/octocam/tls/octocam.key \
      -out /etc/octocam/tls/octocam.crt \
      -subj \"/CN=\$(hostname).local\" \
      -addext \"subjectAltName=DNS:\$(hostname).local,DNS:octocam.local,IP:127.0.0.1\"
  fi
  sudo -n chmod 0644 /etc/octocam/tls/octocam.crt
  sudo -n chmod 0600 /etc/octocam/tls/octocam.key
  sudo -n install -m 0644 '$REMOTE_TMP/octocam-web.nginx.conf' '$NGINX_SITE_FILE'
  sudo -n rm -f /etc/nginx/sites-enabled/default
  sudo -n ln -sfn '$NGINX_SITE_FILE' '$NGINX_SITE_LINK'
  sudo -n nginx -t
  # Matter daemon runs sandboxed as its own user (never root: parses untrusted
  # LAN traffic). Storage dir owns the CHIP KVS + status file.
  if ! id -u octocam-matter >/dev/null 2>&1; then
    sudo -n useradd --system --no-create-home --shell /usr/sbin/nologin octocam-matter
  fi
  # octocam-web (running as the service user) must be able to wipe the KVS on
  # \"Reset Matter pairing\"; grant it group access to the daemon storage dir.
  if [ '$SERVICE_USER' != root ]; then
    sudo -n usermod -aG octocam-matter '$SERVICE_USER'
  fi
  sudo -n install -d -o octocam-matter -g octocam-matter -m 770 /var/lib/octocam/matter-storage
  sudo -n systemctl daemon-reload
  sudo -n systemctl enable octocam-wifi-setup.service >/dev/null
  sudo -n systemctl restart octocam-web.service
  sudo -n systemctl enable --now nginx.service >/dev/null
  sudo -n systemctl reload nginx.service
  rm -rf '$REMOTE_TMP'
  # Health gate: the UI must actually serve, not just be \"active\". Roll back on failure.
  # Poll up to ~30s: on the first boot after an upgrade the startup mediamtx
  # reconcile can restart octocam-rtsp (bounded ~10s) BEFORE the listener binds.
  healthy=false
  for _ in \$(seq 1 15); do
    if curl -fsS -m 4 -o /dev/null \"http://127.0.0.1:$HEALTH_PORT/login\"; then
      healthy=true
      break
    fi
    sleep 2
  done
  if [ \"\$healthy\" != true ]; then
    echo \"health check FAILED — rolling back\" >&2
    if [ -x /usr/local/bin/octocam-web.bak ]; then
      sudo -n cp -f /usr/local/bin/octocam-web.bak /usr/local/bin/octocam-web
      sudo -n systemctl restart octocam-web.service
    fi
    exit 1
  fi
  curl -fsS -m 4 -o /dev/null http://127.0.0.1/login
  curl -kfsS -m 4 -o /dev/null https://127.0.0.1/login
  systemctl is-active octocam-web.service
  echo \"health check OK\"
'"
