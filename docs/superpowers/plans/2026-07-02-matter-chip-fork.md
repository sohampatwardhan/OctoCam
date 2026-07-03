# Matter CHIP Fork & Acceptance Plan (Plan 2 of 2 — hardware-gated draft)

> **Status: DRAFT / GATED.** This plan cannot be executed to completion without
> (a) the multi-GB CHIP cross-compile toolchain, (b) the Pi Zero 2 W at
> 192.168.2.211, and (c) a bridged-network Linux host for the reference
> camera-controller. Task details marked ⚠ depend on CHECK results and the
> pinned upstream tree; flesh them out at execution time against the checkout.
> Plan 1 (`2026-07-02-matter-control-plane.md`) fixed the daemon contract this
> plan implements.

**Goal:** Produce the `dist/chip-camera-app` ARM64 binary (patched CHIP camera-app), deploy it to the Pi, and pass the acceptance gate: commission + live WebRTC view + snapshot from CHIP's reference camera-controller.

## Task 1: Fork setup

- Fork `project-chip/connectedhomeip` → `octocam-connectedhomeip` (separate repo).
- Pin to the newest release SHA whose `examples/camera-app/linux` builds green upstream; record the SHA in `scripts/build-matter.sh`.
- Record the matching Docker image digest: read `integrations/docker/images/base/chip-build/version` at that SHA, pin `ghcr.io/project-chip/chip-build-crosscompile@sha256:…` accordingly.

## Task 2: Patch series (⚠ shapes depend on the pinned tree)

1. **`--rtsp-source <url>`**: new GStreamer source path in `examples/camera-app/linux/src/camera-device.cpp` — `rtspsrc location=<url> latency=200 ! rtph264depay ! h264parse config-interval=-1 ! appsink`, bypassing `x264enc`/camera ownership. Must watch the pipeline bus for ERROR/EOS and rebuild the source bin with exponential backoff (1s→30s), keeping WebRTC sessions alive across gaps ≤ 30s (octocam-web restarts mediamtx once per boot ~10s in — this is the predictable mid-stream drop). Advertised AV Stream Management parameters come from the negotiated stream, not hardcoded 640×480.
2. **`--snapshot-url <url>`**: snapshot provider fetches JPEG via libcurl from the loopback endpooint; ≥5s min interval between cold fetches; HTTP 409 → snapshot stream unavailable, 503/timeout (>10s) → transient error; never crash on fetch failure.
3. **`--status-file <path>`**: write `{status, commissioned, fabric_count, stream_state, error}` JSON atomically (tmp+rename) on state transitions only (SD wear).

Keep each patch a single commit; upstream the rtsp-source patch when stable.

## Task 3: `scripts/build-matter.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
# Pins (update together; see docs/matter.md)
CHIP_REPO="git@github.com:<user>/octocam-connectedhomeip.git"
CHIP_SHA="<pinned>"
IMAGE="ghcr.io/project-chip/chip-build-crosscompile@sha256:<pinned>"
WORK="${OCTOCAM_CHIP_DIR:-$HOME/octocam-chip}"
[ -d "$WORK" ] || git clone "$CHIP_REPO" "$WORK"
git -C "$WORK" fetch && git -C "$WORK" checkout "$CHIP_SHA"
docker run --rm -v "$WORK":/var/connectedhomeip "$IMAGE" bash -lc '
  cd /var/connectedhomeip
  git config --global --add safe.directory "*"
  ./scripts/run_in_build_env.sh \
    "./scripts/build/build_examples.py --target linux-arm64-camera-clang build"
'
BIN="$WORK/out/linux-arm64-camera-clang/chip-camera-app"
llvm-strip "$BIN" -o dist/chip-camera-app 2>/dev/null || cp "$BIN" dist/chip-camera-app
echo "stripped size: $(du -h dist/chip-camera-app | cut -f1)"
```

- **CHECK-1 gate (run FIRST):** `readelf -V dist/chip-camera-app | grep -o 'GLIBC_2\.[0-9]*' | sort -Vu | tail -1` must be ≤ 2.36. If not, switch to a native `debian:bookworm` arm64 container build with Bookworm GStreamer dev packages (document in this script).

## Task 4: Pi runtime deps (CHECK-2/3)

- `ldd dist/chip-camera-app` on the Pi + first run with `GST_DEBUG=3`: derive the exact GStreamer package set (expect core/base/good/bad, `gstreamer1.0-nice`, avahi-daemon + dbus for platform DNS-SD). Record in `docs/matter.md`, then add to `install.sh`'s apt line and install on the reference Pi.
- Confirm the DNS-SD backend gn arg (`chip_mdns="platform"`) in the build; verify :5353 coexistence with avahi + the HomeKit daemon's ciao (CHECK-3).
- CHECK-4: `ip -6 addr show wlan0 | grep fe80` on the Pi.

## Task 5: Acceptance (bridged-L2 Linux host — NOT Docker-on-Mac)

1. Build controller on the Linux host: `./scripts/build/build_examples.py --target linux-x64-camera-controller build`.
2. On the Pi: enable Matter on `/matter`, note passcode.
3. Controller: `pairing onnetwork 1 <passcode>` → success; `/matter` fabric count = 1.
4. `liveview start 1 --min-res-width 640 --min-res-height 480 --min-framerate 30` → live video renders.
5. Snapshot request → JPEG.
6. Cross-check onboarding payload: `chip-tool payload parse-setup-payload <MT:… from /matter>` decodes to VID 0xFFF1 / PID 0x8001 / our discriminator+passcode (validates Plan-1 Task 5 encoder end-to-end).
7. Lifecycle: restart mediamtx mid-stream (reconnect ≤30s); settings save w/o Matter change (daemon must NOT restart); resolution change (one restart); reset pairing (fabric gone, passcode rotated).
8. Measurements (CHECK-5/7/8): RSS/CPU under 2 sessions + sustained snapshot polling → tune `MemoryMax`; 3rd-session rejection; `fatrace` KVS write soak. Record numbers in docs/matter.md.
9. Best-effort: HA Matter Server 9.0 commissioning with test-VID override (CHECK-9), documented.

## Open items carried from the spec

CHECK-1..9 (spec §Open verifications), commissioning-window behavior (CHECK-6) → adjust `/matter` copy if CHIP's default differs from open-on-boot assumption.
