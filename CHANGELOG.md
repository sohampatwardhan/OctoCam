# Changelog

## Unreleased - 2026-07-09

### Added

- feat(web, homekit): HomeKit Secure Video (HKSV) recording. When motion
  detection and HKSV are enabled, the Node HomeKit bridge advertises a recording
  capability paired to the camera's motion sensor; on motion the Apple Home Hub
  negotiates a configuration and the bridge produces a fragmented-MP4 H.264 clip
  with ffmpeg at the hub-negotiated resolution/bitrate, streamed back over the
  HomeKit Data Stream transport (no local storage, no prebuffer, video only).
  Advertised recording resolutions cap HD frame rate for the Pi Zero 2 W, and the
  encode preserves the source aspect ratio. `hksv_enabled` gates both the
  advertised capability and an extra mediamtx reader reservation, and requires
  motion detection to be on. The bridge runs under systemd memory/CPU limits so a
  concurrent recording encode cannot starve mediamtx.
- feat(web, homekit): software motion detection. A zero-dependency Rust detector
  pipes a low-resolution grayscale stream from mediamtx via ffmpeg and flags
  motion by frame differencing, with an 8x8 zone-selection grid and
  global-lighting-change suppression to reduce false triggers. Motion state is
  published as a Server-Sent Events stream (`/api/motion/events`) and drives a
  HomeKit MotionSensor service on the camera accessory. Configurable from the
  stream settings page: enable toggle, sensitivity, and an interactive zone
  editor.
- feat(web): configuration backup & restore on the System page. `GET /backup`
  downloads a versioned JSON envelope of the portable settings (camera, stream,
  RTSP, image, motion, feature toggles) plus authorized SSH public keys; the
  admin password hash and Wi-Fi credentials are never included. `POST /restore`
  imports a backup onto an already-set-up device: portable fields are overlaid
  on the current settings (device-bound fields — admin hash, `setup_complete`,
  HomeKit pairing, Wi-Fi SSID — are preserved, not taken from the file), all
  values are re-validated/clamped, downstream services are reconfigured, and
  SSH keys are merged (union, deduped by fingerprint) in one atomic write. Both
  routes require setup to be complete and an admin session; restore also
  requires a same-origin request and caps the upload at 256 KB.
- feat(web): SSH keys page (Advanced Settings) to view, revoke, and authorize
  the public keys in root's `/root/.ssh/authorized_keys`. Keys are shown with
  type, comment, and SHA256 fingerprint. Added keys are validated as a single
  well-formed line (multi-line, control-char, options-prefixed, and oversized
  input rejected); the file is rewritten atomically after verifying staged
  contents, and never emptied except on a confirmed last-key revoke, which
  warns that root SSH access will be lost. POST routes require an admin session
  and a same-origin request; only enumerated status codes travel in redirects.
- feat(matter): Matter 1.5 camera control plane — matter_enabled setting,
  onboarding QR/manual code generated locally, sandboxed octocam-matter
  systemd unit, loopback snapshot endpoint, additive reader reservation,
  /matter settings page. Daemon binary (patched CHIP camera-app) tracked
  separately; see docs/matter.md.

### Changed

- Migrated the local web control panel from the legacy Python/Flask app to the
  Rust `octocam-web` service.
- Replaced the boot-time Wi-Fi setup shell/Python flow with
  `octocam-web --wifi-setup`, keeping NetworkManager as the source of truth for
  saved credentials.
- Added Docker-based Mac-to-Raspberry Pi builds with `scripts/build-pi-web.sh`
  and deployment with `scripts/deploy-pi-web.sh`.
- Updated the control panel layout with a compact top bar, collapsible mobile
  navigation, Lucide icons, View-first navigation order, and denser content
  panes.
- Added dynamic status refresh for settings, system status, Wi-Fi details, and
  logs through `/api/settings` and `/api/status`.
- Added a power dialog for restarting the OctoCam service, rebooting the device,
  or shutting the device down.
- Added system metric meters for CPU, memory, and swap, plus a Wi-Fi signal
  indicator based on RSSI when available.
- Streamlined the Wi-Fi page around the connected network and saved profiles,
  with an add-network dialog for scanned or manual networks and explicit
  security selection.

### Removed

- Removed the legacy Python package, Flask templates, `requirements.txt`, and
  old Wi-Fi setup shell script now covered by the Rust service.
