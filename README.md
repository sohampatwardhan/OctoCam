# OctoCam

A Raspberry Pi Zero 2 W refresh of the Pimoroni OctoCam kit.

This repo contains a lightweight local web interface for setting up and managing
an OctoCam built with a Raspberry Pi Zero 2 W. The original kit shipped for a Pi
Zero W; the Zero 2 W keeps the same small form factor while adding enough CPU
headroom for a 64-bit Raspberry Pi OS install, local preview, a minimal HomeKit
accessory daemon, and a separate RTSP camera service.

The product shape is intentionally appliance-like:

1. First boot opens a setup flow.
2. The user names the camera, picks a room, and confirms stream defaults.
3. Optional HomeKit pairing is exposed without turning the UI into a full home
   automation stack.
4. After setup, the UI collapses into a compact settings/status page.

Matter 1.5 camera support is implemented as an optional sidecar daemon (a
patched build of connectedhomeip's camera-app) that relays the mediamtx H.264
stream over Matter/WebRTC. As of mid-2026 only SmartThings has shipped Matter
camera viewing; Home Assistant support is experimental, and Alexa/Google/Apple
have not shipped it — the /matter page in the web UI shows an honest
per-ecosystem support matrix. See docs/matter.md. Note: disabling mDNS
(scripts/minimize-os.sh --disable-mdns) breaks Matter commissioning.

## Embedded Constraints

OctoCam should be treated as embedded software, not a tiny general-purpose web
server. The Pi Zero 2 W is capable, but memory, thermals, SD card wear, and boot
time still matter.

Target hardware:

- 1GHz quad-core 64-bit Arm Cortex-A53 CPU
- 512MB RAM
- Built-in 2.4GHz Wi-Fi 4 (802.11n)
- Bluetooth 4.2

Development preferences:

- Prefer C, C++, or Rust for long-running camera, streaming, pairing, and device
  control daemons.
- Use Rust for the long-running web control plane and device orchestration
  helpers.
- Avoid heavy SPAs, Node build chains, large dependency trees, and background
  polling loops.
- Bound memory, log size, subprocess use, network retries, and write frequency.
- Keep the camera/RTSP path separate from the web UI so video work can run in a
  lower-level process.
- Measure on the Pi Zero 2 W before adding always-on features.

## Security Model

Passwords must never be stored in plaintext by OctoCam.

- OctoCam admin passwords are stored only as salted PBKDF2-HMAC-SHA256 hashes.
- Password hashes are not returned by the settings API.
- The web session signing key is generated at install time and stored at
  `/var/lib/octocam/secret-key`.
- Wi-Fi passwords are not stored in OctoCam settings or scan caches.
- Wi-Fi credentials are submitted directly to NetworkManager because the device
  needs a recoverable Wi-Fi secret to reconnect; OctoCam does not keep its own
  copy.
- Root SSH keys are managed through `/root/.ssh/authorized_keys`. The control
  panel validates a pasted key as a single well-formed line before authorizing
  it (rejecting multi-line, control-character, options-prefixed, and oversized
  input), rewrites the file atomically after verifying the staged contents, and
  never writes an empty key file except on a confirmed last-key revoke. Adding
  or revoking a key requires an authenticated admin session and a same-origin
  request. Full key material is never placed in a URL or log line.

## OS Baseline

Target Debian 12 `bookworm` for the first OctoCam appliance builds. Bookworm is
available both as Raspberry Pi OS Legacy Lite 64-bit and as DietPi for Raspberry
Pi Zero 2 W, and it gives the project a better-tested, lower-footprint base than
the newer Debian 13 `trixie` images.

Use a minimal image, not a desktop image. The device should boot to
`multi-user.target`, run only the services needed for Wi-Fi, SSH, camera,
OctoCam, RTSP, and optional HomeKit accessory support, and avoid background desktop or
developer conveniences.

Raspberry Pi OS Legacy Lite is the reference baseline because it is the most
boring path for Raspberry Pi camera, libcamera/rpicam, and first-party Pi
support.

DietPi is the leading low-overhead candidate because it is Debian with reduced
bloat, supports the Raspberry Pi Zero 2 W, and supports the Raspberry Pi camera.
If it preserves the camera and Wi-Fi setup behavior, it may become the preferred
OctoCam image.

Trixie remains the forward-looking target once it has more Raspberry Pi field
time or when a required camera, NetworkManager, HomeKit, or security feature
depends on it.

DietPi acceptance criteria:

- Use the ARMv8 Raspberry Pi Zero 2 W image, preferably Debian 12 `bookworm`
  while Bookworm remains the OctoCam baseline.
- Verify Raspberry Pi camera support, rpicam/libcamera packages, and the
  chosen RTSP service.
- Verify NetworkManager, `nmcli`, setup AP mode, and captive portal behavior.
- Verify HomeKit accessory runtime memory on a 512MB board.
- Measure idle RAM, boot time, write rate, and thermal behavior against
  Raspberry Pi OS Legacy Lite.

DietPi should become the preferred image if it preserves camera and Wi-Fi
behavior while giving meaningful headroom on the Pi Zero 2 W.

Conservative minimization is available during install:

```bash
sudo ./install.sh --minimal-os
```

Or run it directly:

```bash
sudo ./scripts/minimize-os.sh
```

The default minimal profile:

- Disables apt recommended/suggested package installs.
- Caps journald disk and memory use.
- Ensures the system boots to `multi-user.target`.
- Disables common unused timers and services when present.
- Cleans apt caches after package operations.

Optional flags:

```bash
sudo ./scripts/minimize-os.sh --disable-bluetooth
sudo ./scripts/minimize-os.sh --disable-mdns
sudo ./scripts/minimize-os.sh --disable-apt-timers
```

Tradeoffs:

- `--disable-bluetooth` is appropriate if setup will never use BLE.
- `--disable-mdns` saves a small service, but `octocam.local` will stop working.
- `--disable-apt-timers` reduces background wakeups, but you must handle updates
  manually.

## Wi-Fi Setup Flow

OctoCam uses NetworkManager for Wi-Fi setup on Raspberry Pi OS Lite.
The boot-time setup service runs the Rust helper command
`octocam-web --wifi-setup`.

Expected first-boot behavior:

1. If the Pi is already connected to a real saved Wi-Fi network, setup AP mode
   is skipped.
2. Otherwise, saved Wi-Fi profiles are tried in order of most recently
   connected, using NetworkManager's `connection.timestamp`.
3. If a saved profile connects successfully, setup AP mode is skipped.
4. If there are no saved profiles, or all saved profiles fail, OctoCam scans
   nearby networks.
5. Scan results are cached at `/var/lib/octocam/wifi-networks.json`.
6. Cached entries include SSID, security class, raw security text, and signal.
7. Security is normalized as `open`, `wep`, `wpa`, `wpa2`, `wpa3`, or `unknown`.
8. OctoCam starts an open setup AP named `OctoCam-Setup`.
9. The setup portal shows cached SSIDs and asks for a password when the cached
   network is not open.
10. Submitted credentials go directly to NetworkManager and are not stored in
   OctoCam's JSON settings.
11. After a successful join, the setup AP autoconnect is disabled.

The current setup AP serves the portal on the OctoCam web UI port. Full
captive-portal auto-popup behavior may need a tiny port-80 redirect or DNS/HTTP
intercept layer after we test on iOS, macOS, Android, and Windows.

## What This Installs

- A Rust-based settings UI on port `8080`
- A first-run setup flow at `/setup`
- A systemd service named `octocam-web`
- A first-boot Wi-Fi setup service named `octocam-wifi-setup`, backed by
  `octocam-web --wifi-setup`
- Persistent settings at `/var/lib/octocam/settings.json`
- Cached Wi-Fi scan results at `/var/lib/octocam/wifi-networks.json`
- Raspberry Pi camera CLI packages for the current libcamera/rpicam stack
- Diagnostics for IP address, uptime, service health, and OctoCam logs

The web UI is intentionally local-network oriented. Put the device behind a
trusted network or add authentication before exposing it outside your LAN.

## Control Panel

The Rust control panel is server-rendered and designed for repeated appliance
maintenance from a phone, tablet, or desktop browser.

- The top bar keeps the OctoCam brand, mobile menu button, power options, and
  logout action visible without a large hero pane.
- Sidebar sections are ordered by use: View, Basic Settings, and Advanced
  Settings. Each navigation item uses an inline Lucide icon.
- On mobile viewports, the sidebar collapses behind the top-bar menu button.
- Live status fields refresh every 5 seconds from `/api/settings` and
  `/api/status`, including camera state, services, system metrics, Wi-Fi
  details, and recent logs.
- The Wi-Fi page focuses on the connected network, signal strength, local
  Wi-Fi IP addresses, and saved profiles. Its add-network dialog supports scan
  or manual entry, security selection, and open networks without passwords.
- The System info page shows CPU, memory, and swap meters plus a Wi-Fi signal
  indicator derived from RSSI when available.
- The System logs page lets the log panel fill the available content area.
- The power button opens a modal dialog with options to restart the OctoCam
  service, restart the Raspberry Pi, or shut down the Raspberry Pi. A shutdown
  requires unplugging and re-plugging power to turn the device back on.
- The SSH keys page (Advanced Settings) lists the public keys authorized for
  root SSH login, shows each key's type, comment, and SHA256 fingerprint, and
  lets you revoke a key or authorize a new one. Removing the last remaining key
  requires an explicit confirmation, since it ends root SSH access to the
  device.

## Quick Start On The Pi

Flash Raspberry Pi OS Lite 64-bit, enable SSH/Wi-Fi, then run:

```bash
sudo ./install.sh
```

After installation, open one of these from a browser on the same network:

```text
http://octocam.local:8080
http://<device-ip>:8080
```

Useful service commands:

```bash
sudo systemctl status octocam-web
sudo systemctl restart octocam-web
journalctl -u octocam-web -f
```

## Development On A Non-Pi Machine

The Rust app can run without Pi camera hardware. It will show host status and
fall back cleanly when rpicam/libcamera CLI tools are unavailable.

```bash
cd rust/octocam-web
cargo run
```

Then open:

```text
http://127.0.0.1:8080
```

## Build And Deploy From A Mac

On Apple Silicon Macs, Docker can build the Raspberry Pi Linux ARM64 web binary
much faster than compiling on the Pi itself.

Start Docker Desktop, then run:

```bash
scripts/build-pi-web.sh
scripts/deploy-pi-web.sh dietpi@192.168.2.211
```

The build artifact is written to `dist/pi/octocam-web`. The deploy script copies
that binary to `/usr/local/bin/octocam-web`, syncs `static/`, installs the
systemd unit files, enables `octocam-wifi-setup`, and restarts `octocam-web`.
It renders the systemd service for `/root/OctoCam` and user `root` by default;
override those with `OCTOCAM_REMOTE_DIR` and `OCTOCAM_SERVICE_USER`.

To redeploy an already-built binary:

```bash
scripts/deploy-pi-web.sh --skip-build dietpi@192.168.2.211
```

## Release Notes

See [CHANGELOG.md](CHANGELOG.md) for the current migration notes and user-facing
changes.

## Web UI Design System

The web UI uses server-rendered Rust templates, a local Pico-inspired CSS
foundation, and a tiny amount of plain JavaScript for copy buttons and stream
preview controls. The goal is an appliance UI that feels polished without a
client-side framework, CDN dependency, or Node build chain.

Design rules:

- Keep layout, forms, sidebar navigation, status rows, and stream preview in
  local CSS.
- Prefer semantic HTML controls and local CSS tokens before adding component
  dependencies.
- If richer components are needed later, evaluate locally vendored Web
  Components only for specific controls such as dialogs, toasts, tabs, switches,
  and tooltips.
- Avoid full Material/React/Vue-style application frameworks unless a future
  feature clearly needs that cost.

## Next Hardware-Facing Steps

- Wire the saved stream settings into a long-running RTSP service.
- Test captive portal behavior and add a minimal redirect/intercept layer if
  client OSes do not auto-open the setup portal reliably.
- Add post-setup network switching through NetworkManager.
- Tune HomeKit camera streaming compatibility and latency after pairing on Apple Home.
- Add authenticated admin access if the UI will leave a trusted LAN.
- Add motion detection and recording controls once the desired camera pipeline is
  chosen.
- Track the CHIP camera-app fork (`docs/superpowers/plans/2026-07-02-matter-chip-fork.md`)
  to build and deploy the Matter daemon binary, then revisit certification and
  additional ecosystem onboarding as Matter camera viewing support rolls out
  beyond SmartThings.
