# Matter 1.5 Camera Support — Design

Date: 2026-07-02
Status: Approved

## Goal

Expose the OctoCam as a Matter 1.5 camera device so its feed can be viewed
from other smart home ecosystems (Home Assistant, Amazon Alexa, Google Home,
SmartThings) as controller support for Matter cameras rolls out.

This is a **build-ahead-of-the-ecosystem** project, decided with full
knowledge of the July 2026 landscape:

- **SmartThings** is the only ecosystem that has shipped Matter camera
  viewing (live stream, snapshots, two-way audio).
- **Home Assistant** replaced python-matter-server with a matter.js-based
  Matter Server 9.0 (HA 2026.7) that speaks Matter 1.5.1. Camera live view
  is experimental in matterjs-server 0.7.0 and there is no HA camera entity
  yet.
- **Alexa** speaks the Matter 1.5 protocol on Echo devices but has no camera
  viewing in the app. **Google Home** has shipped nothing for Matter cameras.
- **Apple Home** has shipped nothing (WWDC 2026 made no Matter camera
  announcement); the existing HomeKit (HAP) daemon remains the Apple path.

Acceptance is therefore against the reference controller we fully control
(see Testing), with ecosystem runs as best-effort observation.

## Scope

**In scope (v1):**

- Matter **Camera device type** with:
  - Camera AV Stream Management (stream allocation/negotiation)
  - WebRTC Transport Provider (live view signaling)
  - Snapshot stream (JPEG)
- Video only: H.264, relayed from mediamtx without re-encoding.
- On-network (IP) commissioning with QR + manual pairing code shown in the
  web UI. Test VID `0xFFF1`.
- Coexistence with the HomeKit daemon (independent daemons, multi-admin).

**Out of scope (v1):**

- Audio (the kit has no microphone; OctoCam streams video-only today).
- Push AV Stream Transport (motion-triggered CMAF clip upload) and Zone
  Management — motion detection is only a settings placeholder in OctoCam.
- Thread/BLE commissioning (Wi-Fi is provisioned before Matter can be
  enabled).
- Certification. This ships on a test VID; Google/Alexa production onboarding
  is revisited when those ecosystems can view cameras at all.

## Stack decision

**Patched fork of connectedhomeip's Linux `camera-app` example (C++),**
running as a sidecar daemon.

Alternatives considered:

- **rs-matter (Rust)** — rejected for v1: targets Matter 1.3, has none of
  the camera clusters, no TCP transport story. Pure-Rust would be a months-
  long greenfield protocol effort.
- **matter.js device-side (Node)** — rejected: all 1.5.1 clusters exist but
  there is no proven device-side camera example, and Node + WebRTC media on
  a 512MB Pi alongside mediamtx is tight.
- **Stock camera-app + v4l2loopback** — rejected: kernel module plus a
  decode → software re-encode round trip the Pi Zero 2 W cannot afford.

CHIP's camera-app is the only working Matter 1.5 camera device
implementation, its documented reference platform is a Raspberry Pi, and it
uses GStreamer internally, which makes the RTSP-ingest patch tractable.

## Architecture

```
libcamera → mediamtx (owns camera, hardware H.264)
              ├─ RTSP :8554 ──→ octocam-homekit (existing, HAP/SRTP)
              ├─ RTSP :8554 ──→ octocam-matter ──→ Matter signaling + WebRTC media
              ├─ WebRTC/HLS  ──→ browser preview
              └─ snapshots   ──→ octocam-web
```

- mediamtx remains the **single camera owner** (libcamera single-consumer
  constraint). `octocam-matter` is just another local RTSP reader.
- Matter is the signaling plane only: the controller sends an SDP offer via
  WebRTC Transport Provider, camera-app answers, ICE establishes
  connectivity, and media flows peer-to-peer over WebRTC.
- The daemon follows the HomeKit sidecar pattern exactly: separate systemd
  service, status JSON file polled by octocam-web, settings toggle, own web
  UI page.

## Components

### 1. CHIP fork (`octocam-connectedhomeip`, separate repo)

Pinned at a release SHA, carrying a small patch series:

1. **RTSP ingest media source** — new source option for the camera-app
   GStreamer pipeline (`rtspsrc ! rtph264depay ! h264parse → appsink`)
   selected by CLI flag, relaying mediamtx's hardware-encoded H.264 and
   bypassing `x264enc` and camera ownership entirely.
2. **Snapshot via local fetch** — JPEG snapshots fetched from octocam-web's
   existing cached snapshot path (2s TTL, single-flight) over a
   loopback-only endpoint, instead of a second decode pipeline.
3. **Status file** — write `/var/lib/octocam/matter-status.json`
   (commissioned flag, fabric count, stream state, last error) on state
   changes, mirroring `homekit-status.json`.

Only the patch series, pin SHA, and build script live in the OctoCam repo.
Rebasing the patches onto newer CHIP releases is an accepted maintenance
cost; camera-app is example-quality code under active churn.

### 2. Build & deploy

- `scripts/build-matter.sh` cross-compiles an ARM64 binary on the Mac using
  CHIP's `chip-build-crosscompile` Docker image (documented flow: pull
  `ghcr.io/project-chip/chip-build-crosscompile`, build target
  `linux-arm64-camera-clang` via `build_examples.py` inside the container).
  **Never build on the Pi.**
- Identity is injected via the standard Linux-app flags (`--discriminator`,
  `--passcode`, `--secured-device-port`, `--KVS
  /var/lib/octocam/matter-storage/kvs`) — no CHIP patch needed for identity
  or storage paths.
- The binary deploys over rsync like the rest of OctoCam.
- `install.sh` adds runtime deps: GStreamer core/base/good/bad + RTSP
  plugins. No libav/ugly (we never decode video on-device).

### 3. octocam-web integration (Rust)

- `Settings` gains `matter_enabled: bool` (default false).
- Matter identity (passcode, discriminator, VID `0xFFF1`, PID) generated
  once on first enable, persisted at `/var/lib/octocam/matter-identity.json`
  (pattern: `homekit-identity.json`). PID is fixed at `0x8001`.
- New `matter.rs` module (pattern: `mediamtx.rs`): renders daemon CLI/env
  config, reads `matter-status.json`, and computes the onboarding payload —
  the QR payload is deterministic from VID/PID/discriminator/passcode, so
  octocam-web renders the QR and 11-digit manual code itself without
  parsing daemon output.
- `streams.rs`/`mediamtx.rs` reader reservation extends to Matter: an
  enabled Matter daemon reserves one reader slot, exactly like
  `homekit_reserve_adds_one_reader()`. Stream selection mirrors HomeKit's
  main/sub fallback.
- The loopback-only snapshot endpoint is added for the daemon's snapshot
  fetches (guarded to 127.0.0.1, no session auth required).

### 4. systemd

`octocam-matter.service`: `After=octocam-rtsp.service`, restart with
backoff, enabled/disabled through the same web settings flow as the HomeKit
service. Commissioning state lives in `/var/lib/octocam/matter-storage/`
(CHIP KVS) and survives reboots and disable/enable cycles.

### 5. Web UI

`/matter` page mirroring `/homekit`: enable toggle, service status, QR code
and manual pairing code, commissioned-fabric count, stream source, last
error, plus an honest note that ecosystem viewing support is still rolling
out (SmartThings shipped; HA experimental; Alexa/Google pending). One nav
link. A "reset Matter pairing" action wipes `matter-storage/`.

## Resource bounds (embedded rules)

- Daemon does not run unless `matter_enabled` is true.
- Concurrent WebRTC sessions capped at 2 via AV Stream Management.
- Video-only; no on-device decode or encode in the Matter path.
- Before the feature is called done, measure RSS/CPU on the Pi Zero 2 W
  under an active WebRTC stream and record the numbers.

## Error handling

- RTSP unavailable → daemon retries with backoff, reports via status file.
- Daemon crash → systemd restart with backoff.
- Errors surface on `/matter` and in `/logs` (journalctl).
- Disabling Matter stops the service but preserves commissioning state;
  only the explicit reset action wipes it.

## Testing & acceptance

- **Rust unit tests**: settings validation/merge, reader reservation,
  daemon config render, onboarding payload generation (QR + manual code
  vectors).
- **Build check**: cross-compile succeeds on the Mac.
- **Acceptance gate**: commission from CHIP's reference `camera-controller`
  and get live WebRTC view + a snapshot from the Pi. The controller's only
  documented build target is `linux-x64-camera-controller` (no macOS
  target), so it runs in a Linux environment on the LAN — a Linux VM or
  container on the Mac, or any Linux host. Documented flow:
  `pairing onnetwork 1 <passcode>` then
  `liveview start 1 --min-res-width 640 --min-res-height 480
  --min-framerate 30`.
- **Best-effort, non-gating**: commission into Home Assistant's matter.js
  server and document behavior. Known caveat: HA's new server blocks
  uncertified devices by default and needs a manual override — document it.

## Documentation

- Replace the README's "Matter is out of scope" paragraph with actual
  status and caveats.
- `docs/matter.md`: fork/patch/build process, commissioning walkthrough,
  ecosystem support matrix with dates.
- CHANGELOG entry.

## Risks

| Risk | Mitigation |
| --- | --- |
| camera-app is example-quality code under churn | Small pinned patch series; rebase cost accepted and documented |
| No published Pi Zero 2 W footprint numbers for camera-app | No encode/decode on-device; measure on the Pi before calling it done |
| HA end-to-end viewing may not work yet regardless of our code | Accepted: acceptance gates on CHIP camera-controller, not HA |
| GStreamer runtime deps add install footprint | Only core/base/good/bad + RTSP plugins; no libav/ugly |
