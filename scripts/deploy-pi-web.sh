#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_SCRIPT="$PROJECT_DIR/scripts/build-pi-web.sh"
DIST_DIR="${OCTOCAM_PI_DIST_DIR:-$PROJECT_DIR/dist/pi}"
ARTIFACT="$DIST_DIR/octocam-web"
SSH_TARGET="${OCTOCAM_PI_SSH:-dietpi@192.168.2.211}"
REMOTE_DIR="${OCTOCAM_REMOTE_DIR:-/root/OctoCam}"
SERVICE_USER="${OCTOCAM_SERVICE_USER:-root}"
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

ssh "$SSH_TARGET" "mkdir -p '$REMOTE_TMP'"
rsync -az "$ARTIFACT" "$SSH_TARGET:$REMOTE_TMP/octocam-web"
rsync -az "$LOCAL_TMP/octocam-web.service" "$LOCAL_TMP/octocam-wifi-setup.service" "$SSH_TARGET:$REMOTE_TMP/"

ssh "$SSH_TARGET" "sudo -n mkdir -p '$REMOTE_DIR/static'"
rsync -az --delete \
  --exclude '._*' \
  --exclude '.DS_Store' \
  --rsync-path='sudo -n rsync' \
  "$PROJECT_DIR/static/" "$SSH_TARGET:$REMOTE_DIR/static/"

ssh "$SSH_TARGET" "bash -lc 'set -euo pipefail
  sudo -n install -m 0755 '$REMOTE_TMP/octocam-web' /usr/local/bin/octocam-web
  sudo -n install -m 0644 '$REMOTE_TMP/octocam-web.service' /etc/systemd/system/octocam-web.service
  sudo -n install -m 0644 '$REMOTE_TMP/octocam-wifi-setup.service' /etc/systemd/system/octocam-wifi-setup.service
  sudo -n systemctl daemon-reload
  sudo -n systemctl enable octocam-wifi-setup.service >/dev/null
  sudo -n systemctl restart octocam-web.service
  rm -rf '$REMOTE_TMP'
  /usr/local/bin/octocam-web --help
  systemctl is-active octocam-web.service
'"
