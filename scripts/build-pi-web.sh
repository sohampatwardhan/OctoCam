#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_WEB_DIR="$PROJECT_DIR/rust/octocam-web"
PI_TARGET="${OCTOCAM_PI_TARGET:-aarch64-unknown-linux-gnu}"
DOCKER_IMAGE="${OCTOCAM_RUST_DOCKER_IMAGE:-rust:1-bookworm}"
DIST_DIR="${OCTOCAM_PI_DIST_DIR:-$PROJECT_DIR/dist/pi}"
CARGO_CACHE="${OCTOCAM_PI_CARGO_CACHE:-$HOME/Library/Caches/octocam-pi-cargo}"
ARTIFACT="$DIST_DIR/octocam-web"

usage() {
  cat <<USAGE
Usage: scripts/build-pi-web.sh

Builds the Rust web UI for Raspberry Pi OS 64-bit in a Linux ARM64 Docker
container and writes:

  $ARTIFACT

Environment overrides:
  OCTOCAM_RUST_DOCKER_IMAGE  Docker Rust image, default: $DOCKER_IMAGE
  OCTOCAM_PI_CARGO_CACHE     Cargo cache directory, default: $CARGO_CACHE
  OCTOCAM_PI_DIST_DIR        Artifact directory, default: $DIST_DIR
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ ! -f "$RUST_WEB_DIR/Cargo.toml" ]]; then
  echo "Missing Rust web app: $RUST_WEB_DIR/Cargo.toml" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "Docker is required for Mac-to-Pi builds. Install Docker Desktop and try again." >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "Docker is installed but the daemon is not running. Start Docker Desktop and try again." >&2
  exit 1
fi

mkdir -p "$DIST_DIR" "$CARGO_CACHE/registry" "$CARGO_CACHE/git"

echo "Building OctoCam web UI for $PI_TARGET with $DOCKER_IMAGE..."
docker run --rm \
  --platform linux/arm64/v8 \
  -e CARGO_TARGET_DIR=/work/rust/octocam-web/target \
  -e HOST_GID="$(id -g)" \
  -e HOST_UID="$(id -u)" \
  -e PI_TARGET="$PI_TARGET" \
  -v "$PROJECT_DIR":/work \
  -v "$CARGO_CACHE/registry":/usr/local/cargo/registry \
  -v "$CARGO_CACHE/git":/usr/local/cargo/git \
  -w /work/rust/octocam-web \
  "$DOCKER_IMAGE" \
  bash -lc 'set -euo pipefail
    export PATH="/usr/local/cargo/bin:$PATH"
    artifact_dir="/work/rust/octocam-web/target/$PI_TARGET/release"
    cargo_target_args=(--target "$PI_TARGET")
    if command -v rustup >/dev/null 2>&1; then
      rustup target add "$PI_TARGET" >/dev/null
    else
      host="$(rustc -vV | awk "/^host:/ { print \$2 }")"
      if [[ "$host" != "$PI_TARGET" ]]; then
        echo "Rust image host target is $host, but $PI_TARGET is required and rustup is unavailable." >&2
        exit 1
      fi
      artifact_dir="/work/rust/octocam-web/target/release"
      cargo_target_args=()
    fi
    cargo build --release --locked "${cargo_target_args[@]}"
    mkdir -p /work/dist/pi
    cp "$artifact_dir/octocam-web" /work/dist/pi/octocam-web
    chmod 0755 /work/dist/pi/octocam-web
    chown -R "$HOST_UID:$HOST_GID" /work/dist /work/rust/octocam-web/target /usr/local/cargo/registry /usr/local/cargo/git 2>/dev/null || true
  '

echo "Built $ARTIFACT"
