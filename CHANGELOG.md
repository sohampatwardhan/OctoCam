# Changelog

## Unreleased - 2026-07-03

### Added

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
