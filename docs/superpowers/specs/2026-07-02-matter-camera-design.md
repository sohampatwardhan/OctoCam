# Matter 1.5 Camera Support — Design

Date: 2026-07-02
Status: Approved (hardened via /plan-harden thorough — 3-reviewer pass, 20 fixes folded in)

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
  web UI. Test VID `0xFFF1`, PID `0x8001`.
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

**DNS-SD backend decision:** the fork builds with CHIP's **platform (Avahi)
DNS-SD backend**, not minimal-mDNS. The Pi already runs `avahi-daemon` for
`octocam.local` and the HomeKit daemon runs its own ciao responder; CHIP's
minimal-mDNS has documented misbehavior alongside another :5353 responder.
Consequences: `avahi-daemon` + D-Bus become hard runtime deps,
`octocam-matter.service` gets `Wants=avahi-daemon.service`, and
`scripts/minimize-os.sh --disable-mdns` **breaks Matter commissioning** —
its help text and README note must say so.
(Open verification: CHECK-3 — confirm backend gn args and :5353 coexistence
on the Pi.)

**IPv6 prerequisite:** Matter operational communication requires IPv6 (at
minimum link-local) on wlan0. Nothing in the NetworkManager flows guarantees
this today. `matter.rs` performs a preflight (a `fe80::` address present on
the wireless interface) and surfaces "IPv6 disabled — Matter requires IPv6"
on `/matter` when absent. NM client connections must not set
`ipv6.method=disabled`. (Open verification: CHECK-4.)

## Architecture

```
libcamera → mediamtx (owns camera, hardware H.264)
              ├─ RTSP :8554 ──→ octocam-homekit (existing, HAP/SRTP)
              ├─ RTSP :8554 ──→ octocam-matter ──→ Matter signaling + WebRTC media
              ├─ WebRTC/HLS  ──→ browser preview
              └─ snapshots   ──→ octocam-web (127.0.0.1 internal listener → Matter)
```

- mediamtx remains the **single camera owner** (libcamera single-consumer
  constraint). `octocam-matter` is just another local RTSP reader.
- Matter is the signaling plane only: the controller sends an SDP offer via
  WebRTC Transport Provider, camera-app answers, ICE establishes
  connectivity, and media flows peer-to-peer over WebRTC.
- The daemon follows the HomeKit sidecar pattern **with two deliberate
  divergences** (see Components §3): the service restarts only on rendered-
  config change (HomeKit restarts on every settings save), and stream
  parameters are pushed by octocam-web rather than read live from
  settings.json (the C++ daemon is configured at exec).

### mediamtx lifecycle coupling (load-bearing)

`rtsp_enabled=false` today **stops the whole `octocam-rtsp` unit**
(`mediamtx.rs` `set_service_enabled`), not just LAN RTSP exposure. That
would permanently kill the Matter daemon's only video source. Therefore:

- The mediamtx service runs whenever
  `rtsp_enabled || homekit_enabled || matter_enabled` is true. The
  `rtsp_enabled` toggle governs only LAN-facing exposure semantics in the
  UI and generated config; loopback paths for daemons remain served.
  (This also fixes the same latent hole for HomeKit.)
- `/matter` surfaces "no video source" in `matter-status.json` if mediamtx
  is down anyway.

## Components

### 1. CHIP fork (`octocam-connectedhomeip`, separate repo)

Pinned at a release SHA, carrying a small patch series:

1. **RTSP ingest media source** — new source option for the camera-app
   GStreamer pipeline (`rtspsrc ! rtph264depay ! h264parse → appsink`)
   selected by CLI flag, relaying mediamtx's hardware-encoded H.264 and
   bypassing `x264enc` and camera ownership entirely.
   **Reconnect is a hard requirement of this patch, not generic error
   handling:** octocam-web's boot-time reconcile restarts mediamtx ~10s into
   boot — provably *after* octocam-matter is up — and `rtspsrc` errors
   terminally on connection loss. The source bin must watch the GStreamer
   bus for EOS/error and rebuild with backoff, keeping live Matter WebRTC
   sessions alive across brief source gaps (or ending them cleanly via AV
   Stream Management if the gap exceeds a timeout). "mediamtx restart
   mid-stream" is a mandatory test.
2. **Snapshot via local fetch** — JPEG snapshots fetched from octocam-web
   over a new loopback-only listener (see §3). Rate-limited on the daemon
   side: minimum 5s between cold fetches, so continuous controller polling
   cannot turn octocam-web's ffmpeg cold-capture into a sustained load.
3. **Status file** — write `/var/lib/octocam/matter-status.json`
   (commissioned flag, fabric list, stream state, IPv6/source preflight
   results, last error) on state changes, mirroring `homekit-status.json`.

Only the patch series, pin SHA, and build script live in the OctoCam repo.
**Rebase cadence:** rebase onto upstream on CHIP security advisories and at
least quarterly — this is a LAN-facing daemon parsing untrusted pre-auth
input; a frozen pin means security fixes never arrive. Keep the RTSP-source
patch upstreamable to shrink the series.

### 2. Build & deploy

- `scripts/build-matter.sh` cross-compiles an ARM64 binary on the Mac using
  CHIP's `chip-build-crosscompile` Docker image, **pinned by digest**
  matching the pinned CHIP SHA's `integrations/docker/.../version` file
  (both recorded in the script). Target: `linux-arm64-camera-clang` via
  `build_examples.py`. **Never build on the Pi.**
- **ABI requirement:** the produced binary must link against ≤ the Pi's
  Raspberry Pi OS Bookworm glibc (2.36) and Bookworm GStreamer ABI. The
  Ubuntu-based cross image's sysroot likely exceeds this (CHECK-1 gates
  this). Fallback build path if it does: native `debian:bookworm` **arm64**
  container on the Apple Silicon Mac (runs natively via Rosetta-free arm64
  emulation, exact glibc + Bookworm GStreamer dev packages).
- Binary is `llvm-strip`ped; deploys use `rsync -z --partial` (CHIP binaries
  are large; deploys traverse 2.4GHz Wi-Fi to an SD card). Stripped size is
  recorded in the measurement checklist.
- Identity is injected via the standard Linux-app flags (`--discriminator`,
  `--passcode`, `--secured-device-port`, `--KVS
  /var/lib/octocam/matter-storage/kvs`) — no CHIP patch needed for identity
  or storage paths.
- Runtime deps: GStreamer core/base/good/bad + RTSP plugins, **plus
  `avahi-daemon` + D-Bus (platform DNS-SD)**, plus whatever the WebRTC path
  actually needs — on Debian, `gstreamer1.0-nice` is packaged separately
  and DTLS/SRTP elements in `-bad` need their crypto deps. **The final dep
  list is derived empirically** (`ldd` the built binary + `GST_DEBUG=3` run
  on a clean minimal image — CHECK-2), recorded in `docs/matter.md`, and
  only then written into `install.sh`.

### 3. octocam-web integration (Rust)

- `Settings` gains `matter_enabled: bool` (default false). **Enabling
  Matter requires an admin password to be set** — `require_admin_login`
  no-ops when the password hash is empty, which would otherwise expose the
  pairing QR (a durable commission-this-camera credential) to anyone on the
  LAN. The `/matter` page and all its actions sit behind
  `require_admin_login`.
- Matter identity (passcode, discriminator, VID `0xFFF1`, PID `0x8001`)
  generated once on first enable, persisted at
  `/var/lib/octocam/matter-identity.json` **with mode 0600**. Persisting
  the passcode (rather than only a SPAKE2+ verifier) deviates from the
  Matter spec's recommendation and from the README's no-plaintext-secrets
  posture; this deviation is documented in `docs/matter.md` and the
  passcode is **rotated on every "reset Matter pairing"** rather than
  reused forever.
- New `matter.rs` module (pattern: `mediamtx.rs`): renders daemon CLI/env
  config, reads `matter-status.json`, runs preflights (IPv6 link-local,
  mediamtx source up), and computes the onboarding payload — the QR payload
  is deterministic from VID/PID/discriminator/passcode, so octocam-web
  renders the QR and 11-digit manual code itself without parsing daemon
  output. **QR rendering is a new Rust dependency** (a `qrcode`-class crate
  + SVG output), to be vetted for build impact — nothing QR-capable exists
  in `Cargo.toml` today (the only QR code in the repo is the Node daemon's
  `qrcode` npm package).
- **Service reconfiguration diverges from the HomeKit flow deliberately.**
  `configure_homekit_service` restarts its daemon on *every* settings save;
  copying that would drop live Matter WebRTC sessions when the user changes
  brightness. Instead `configure_matter_service()` re-renders the daemon
  config on save and restarts `octocam-matter` **only when the rendered
  config changed** (reuse the `write_mediamtx_config` changed-detection
  pattern). Stream-parameter changes (resolution/fps/bitrate/path renames,
  sub-stream disable) flow through this render so AV Stream Management
  advertises parameters consistent with what mediamtx emits.
- **Reader reservation becomes additive**: `reserve = homekit_term +
  matter_term` in `render_mediamtx_config` (today a single scalar), with a
  `matter_reserve_adds_one_reader`-style test beside the HomeKit one.
- **Viewer classification generalizes**: `streams.rs` currently buckets
  *any* loopback RTSP reader into the `homekit` counter. `PathViewers`
  gains a distinct Matter bucket (or a generic local-daemons bucket with
  per-daemon attribution joined on each daemon's status-file stream
  source), so `/stream` and `/api/status` label viewers correctly.
- **Internal snapshot listener**: a second axum listener bound to
  `127.0.0.1:8081` (structurally unspoofable — no header checks, no
  `ConnectInfo` parsing on the shared 0.0.0.0 listener) serving
  `/internal/snapshot.jpg` without session auth but **keeping the
  `camera_enabled` check** (409 when the camera is off). Error contract for
  the daemon: 409 → snapshot stream unavailable; 503/timeout → transient
  error to the controller; cold capture may take up to 8s (serialized) —
  the daemon's fetch timeout must exceed it. Note: a cold capture spawns a
  transient ffmpeg RTSP reader that consumes a sub-path reader slot; the
  reservation math accounts for one transient capture slot whenever any
  daemon is enabled.

### 4. systemd — sandboxed, non-root

`octocam-matter.service` runs an example-quality C++ daemon parsing
untrusted pre-auth traffic from the whole LAN on TCP/UDP 5540. It does
**not** follow the existing root default (`deploy-pi-web.sh`
`SERVICE_USER=root`):

- Dedicated `octocam-matter` system user; needs only loopback RTSP read,
  loopback snapshot fetch, and write to `/var/lib/octocam/matter-storage/`
  + `matter-status.json`.
- Unit directives: `NoNewPrivileges=yes`, `ProtectSystem=strict` +
  `ReadWritePaths=/var/lib/octocam`, `ProtectHome=yes`,
  `CapabilityBoundingSet=`, `RestrictAddressFamilies=AF_INET AF_INET6
  AF_UNIX`, `MemoryMax=` (initial 150M, tuned after CHECK-5 measurement),
  `OOMScoreAdjust=500` (the optional feature dies first — never mediamtx or
  octocam-web), `LogRateLimitIntervalSec=30s` + `LogRateLimitBurst=1000`
  (CHIP example code is chatty; README requires bounded logs), reduced CHIP
  log level in production.
- `After=octocam-rtsp.service`, `Wants=avahi-daemon.service`, restart with
  backoff.

**Install/upgrade story (currently missing from all scripts):** the unit
template lands in `systemd/`; `install.sh` installs it, adds the runtime
deps, and reconciles enable-state from settings.json (pattern: the existing
`grep '"homekit_enabled": true'` block); `scripts/deploy-pi-web.sh` (or a
one-time `scripts/setup-matter-pi.sh`) is extended so the unit + deps reach
the already-installed reference Pi, which only ever receives rsync deploys.

### 5. Web UI

`/matter` page (behind admin login, see §3): enable toggle, service status,
QR code and manual pairing code, **commissioned-fabric list with per-fabric
remove** (Operational Credentials cluster supports fabric removal), stream
source, preflight warnings, last error. One nav link.

- **Support matrix next to the QR**, per ecosystem: works (SmartThings) /
  experimental + override needed (Home Assistant) / commissions but no
  video (Alexa) / not supported (Google, Apple) — and the device labeled
  "uncertified / test VID" so HA's default certification block reads as
  expected behavior rather than breakage.
- **Commissioning window policy**: the window opens only on explicit user
  action (enable, or a "open pairing window" button) and auto-closes after
  the standard 15 minutes — never indefinitely on boot. A status line
  distinguishes "commissioned, no viewer yet" so commissions-but-no-video
  ecosystems read correctly. Fabric-count increases surface prominently.
- **Disable ≠ revoke**, and the UI says so: disabling Matter stops the
  service but previously commissioned fabrics regain access on re-enable.
  The disable confirmation warns about this. A warning also shows when
  `matter-status.json` reports fabrics > 0 while `matter_enabled` is false
  (e.g., after a settings.json wipe/reimage leaves orphaned fabric state).
- **"Reset Matter pairing"** executes stop → wipe `matter-storage/` →
  rotate passcode → start (wiping the KVS under a live daemon is racy; the
  daemon holds fabric state in memory and rewrites the KVS).
- A device-wide **factory reset** (for resale) wipes Matter storage, HomeKit
  pairing, and settings together; documented that `rm` doesn't erase flash
  blocks (accepted residual risk).

## Resource bounds (embedded rules)

- Daemon does not run unless `matter_enabled` is true.
- Concurrent WebRTC sessions capped at 2 via AV Stream Management (verify
  the example enforces the cap in-protocol — CHECK-7; scope down to 1 if
  CHECK-5 measurements demand it).
- No on-device video encode in the Matter path, and no *added* decode:
  note that snapshots already software-decode today —
  `camera::capture_jpeg_via_rtsp` spawns ffmpeg on every 2s-TTL cache miss.
  The Matter daemon's 5s snapshot rate limit bounds how often controllers
  can trigger that.
- `MemoryMax` + `OOMScoreAdjust` bound the blast radius (see §4).
- Before the feature is called done, measure on the Pi Zero 2 W: RSS/CPU
  under 2 active WebRTC sessions **and under sustained snapshot polling**,
  KVS write frequency during a soak (`fatrace` — SD wear), and stripped
  binary size. Record the numbers.

## Error handling

- RTSP source lost (including the predictable boot-time mediamtx
  reconcile restart) → source-bin rebuild with backoff (§1.1), state in
  status file.
- Daemon crash → systemd restart with backoff; OOM kills land on this
  daemon first by design.
- Snapshot endpoint 409/503/timeout → mapped to Matter snapshot errors
  (§3).
- Preflight failures (no IPv6 link-local, mediamtx down) → surfaced on
  `/matter` with specific remediation text.
- Errors visible on `/matter` and in `/logs` (journalctl).

## Testing & acceptance

- **Rust unit tests**: settings validation/merge (incl. matter requires
  admin password), additive reader reservation, viewer classification with
  two local daemons, daemon config render + changed-detection restart
  logic, onboarding payload generation (QR + manual code vectors),
  loopback listener rejects non-loopback and enforces `camera_enabled`.
- **Build check**: cross-compile succeeds; `readelf` confirms glibc ≤ 2.36
  (CHECK-1).
- **Acceptance gate**: commission from CHIP's reference `camera-controller`
  and get live WebRTC view + a snapshot from the Pi. The controller's only
  documented build target is `linux-x64-camera-controller` (no macOS
  target), and `pairing onnetwork` requires receiving the device's mDNS
  multicast — **Docker-on-Mac containers cannot see LAN multicast (NAT'd
  networking), so the controller runs on a bridged-network Linux VM or a
  physical Linux host on the same L2 segment.** Documented flow:
  `pairing onnetwork 1 <passcode>` then
  `liveview start 1 --min-res-width 640 --min-res-height 480
  --min-framerate 30`.
- **Lifecycle tests on the Pi**: mediamtx restart mid-stream (reconnect);
  settings save that doesn't change Matter config (daemon must NOT
  restart); stream-parameter change (daemon restarts once, parameters
  consistent); reset-pairing while commissioned.
- **Best-effort, non-gating**: commission into Home Assistant's matter.js
  server and document behavior, including the uncertified-device override
  flow (CHECK-9).

## Open verifications (from hardening; resolve during implementation)

1. CHECK-1: glibc/GLIBCXX of the cross image sysroot vs Pi Bookworm
   (`readelf -V`; gates the debian:bookworm fallback).
2. CHECK-2: camera-app's actual WebRTC stack + GStreamer plugin set
   (`ldd` + `GST_DEBUG=3` on clean image; `gstreamer1.0-nice`?).
3. CHECK-3: DNS-SD backend gn args + :5353 coexistence (avahi + ciao +
   CHIP) on the Pi.
4. CHECK-4: IPv6 link-local actually present on wlan0 on this image.
5. CHECK-5: RSS/CPU under 2 WebRTC sessions + sustained snapshot polling →
   final `MemoryMax`.
6. CHECK-6: CHIP default commissioning-window behavior when
   enabled-but-uncommissioned.
7. CHECK-7: does camera-app enforce the 2-session cap in-protocol?
8. CHECK-8: KVS write frequency during soak (SD wear).
9. CHECK-9: HA Matter Server 9.0 test-VID override flow, empirically.

## Documentation

- Replace the README's "Matter is out of scope" paragraph with actual
  status and caveats; note `--disable-mdns` breaks Matter.
- `docs/matter.md`: fork/patch/build process (image digest + SHA pins),
  empirically-derived dependency list, commissioning walkthrough, ecosystem
  support matrix with dates, security-model deviation note (persisted
  passcode), factory-reset/resale guidance.
- CHANGELOG entry.

## Risks

| Risk | Likelihood | Impact | Posture |
| --- | --- | --- | --- |
| CHIP camera-cluster churn breaks the patch series at rebase | High | Medium | Small pinned patch series; upstream the RTSP source; quarterly + advisory-driven rebase cadence |
| Ecosystems never ship viewing / enforcement locks out test VIDs | Medium | High (product) | Accepted per goal decision; honest UI support matrix |
| Memory ceiling on 512MB makes 2 sessions infeasible | Medium | High | `MemoryMax` + `OOMScoreAdjust=500` bound blast radius; CHECK-5 decides scope-down to 1 session |
| Toolchain/image drift breaks reproducibility | Medium | Medium | Digest + SHA pinning recorded in build script |
| Root-RCE via pre-auth parsing bug in example code | Low | Critical | Non-root sandboxed unit reduces to service-user compromise |
