# Matter 1.5 Camera Support

This document describes OctoCam's Matter camera control plane: what it is,
what each smart home ecosystem actually supports today, how to enable and
commission it, its security model, the contract between `octocam-web` and
the daemon binary, how the daemon is built, and what remains to be verified
empirically on real hardware.

The authoritative design is
`docs/superpowers/specs/2026-07-02-matter-camera-design.md`. The
implementation plans are
`docs/superpowers/plans/2026-07-02-matter-control-plane.md` (control plane,
implemented — this document describes what that plan built) and
`docs/superpowers/plans/2026-07-02-matter-chip-fork.md` (daemon binary, not
yet built).

## 1. What it is

OctoCam exposes its camera to Matter 1.5 controllers through an optional
sidecar daemon: a patched build of connectedhomeip's (CHIP) `camera-app`
example. mediamtx remains the single owner of the camera (the libcamera
pipeline only tolerates one consumer); the Matter daemon is just another
local RTSP reader, exactly like the existing HomeKit daemon. Matter itself
carries only signaling — a controller sends a WebRTC SDP offer through the
Camera AV Stream Management and WebRTC Transport Provider clusters, the
daemon answers, ICE negotiates connectivity, and the H.264 video then flows
peer-to-peer over WebRTC without any re-encoding on the Pi. Snapshots (JPEG)
are fetched by the daemon from a loopback-only endpoint on `octocam-web`.

```
libcamera → mediamtx (owns camera, hardware H.264)
              ├─ RTSP :8554 ──→ octocam-homekit (existing, HAP/SRTP)
              ├─ RTSP :8554 ──→ octocam-matter ──→ Matter signaling + WebRTC media
              ├─ WebRTC/HLS  ──→ browser preview
              └─ snapshots   ──→ octocam-web (127.0.0.1 internal listener → Matter)
```

`octocam-web` (Rust) owns everything on the OctoCam side of this: the
`matter_enabled` setting, generating and persisting the Matter identity
(passcode, discriminator, vendor/product ID), rendering the QR code and
11-digit manual pairing code locally (no dependency on the daemon to print
them), writing the daemon's environment file, reading its status file, and
managing the `octocam-matter` systemd unit. The daemon itself — the patched
CHIP `camera-app` binary — is tracked in a separate fork and build pipeline
(see §6); as of this writing that binary does not exist yet, so enabling
Matter today configures and (attempts to) start a unit whose `ExecStart`
target is absent. systemd reports the unit failed, and the `/matter` page
surfaces that as an error/starting state rather than pretending the feature
is live end-to-end.

mediamtx's `octocam-rtsp` service now stays running whenever
`rtsp_enabled || homekit_enabled || matter_enabled` is true, rather than
stopping outright when LAN RTSP exposure is turned off. Previously,
disabling `rtsp_enabled` stopped the whole RTSP unit and would have silently
cut off any daemon's only video source; `rtsp_enabled` now governs LAN-facing
exposure semantics only, and loopback readers for local daemons stay served
regardless.

## 2. Ecosystem support (July 2026)

Matter 1.5 camera support is brand new across the industry, and adoption is
uneven. OctoCam's `/matter` page renders this table directly in the UI next
to the pairing QR so users know what to expect before they commission
anything.

| Ecosystem | Status | Notes |
| --- | --- | --- |
| SmartThings | Shipped | Live view, snapshots, and two-way audio for Matter cameras are live. OctoCam is video-only (the kit has no microphone), so only the video path applies. |
| Home Assistant | Experimental | HA 2026.7 ships the matter.js-based Matter Server 9.0, which speaks Matter 1.5.1. Camera live view is experimental in the underlying matter.js server and there is no dedicated HA camera entity yet. HA also blocks uncertified/test-VID devices by default — OctoCam ships on the CSA test vendor ID `0xFFF1`, so a manual override in HA's Matter integration is required before it will commission at all. |
| Alexa | Commissions, no video | Echo devices speak the Matter 1.5 protocol and will commission an OctoCam as a Matter device, but the Alexa app has no camera-viewing surface for Matter cameras yet. |
| Google Home | Not supported | Nothing shipped for Matter cameras as of mid-2026. |
| Apple Home | Not supported | No Matter camera support was announced at WWDC 2026. The existing HomeKit (HAP) daemon remains the supported Apple path for viewing the OctoCam feed; it runs independently of and alongside the Matter daemon. |

Because of this landscape, OctoCam treats Matter as a **build-ahead-of-the-
ecosystem** feature: acceptance is validated against a reference controller
(CHIP's own `camera-controller`) that the project fully controls, with
real-ecosystem commissioning (SmartThings, Home Assistant) treated as
best-effort observation rather than a hard gate. The `/matter` page is
deliberately honest about this rather than implying broad smart-home
support that doesn't exist yet.

## 3. Enabling & commissioning

Matter can only be turned on from `/matter` once an admin password has been
set. This is a hard requirement, not a suggestion: the Matter pairing QR
code and manual code are a durable, reusable credential that lets any device
commission (and therefore view) the camera. `require_admin_login` is a
no-op while the admin password hash is empty, which would otherwise expose
that credential to anyone on the LAN who loads the page. The `matter_enabled`
toggle on `/matter` is disabled in the UI until a password exists, and
`enforce_matter_requires_admin()` forces `matter_enabled` back to `false`
server-side if the password hash is ever cleared while Matter is on.

Once enabled, `/matter` shows a QR code and an 11-digit manual pairing code,
both computed locally from the persisted identity (vendor ID, product ID,
discriminator, passcode) — `octocam-web` does not need to shell out to or
parse output from the daemon to produce them. Commissioning is on-network
(IP) only; there is no BLE commissioning path, since Wi-Fi has to already be
provisioned before Matter can be enabled at all. A commissioning window
opens only on explicit user action and follows the standard Matter timeout
behavior rather than staying open indefinitely.

Matter's operational communication requires IPv6, at minimum a link-local
address on the wireless interface. `/matter` performs a preflight check for
this and shows a warning ("IPv6 appears disabled on this device... Matter
requires IPv6") if no `fe80::` address is present, since nothing in the
NetworkManager configuration flow otherwise guarantees IPv6 stays enabled.

Disabling mDNS via `scripts/minimize-os.sh --disable-mdns` breaks Matter
commissioning. The Matter daemon build uses CHIP's platform (Avahi) DNS-SD
backend rather than CHIP's minimal-mDNS implementation, specifically because
the Pi already runs `avahi-daemon` for `octocam.local` and the HomeKit
daemon's own mDNS responder — running CHIP's minimal-mDNS alongside another
:5353 responder has documented misbehavior. That means Matter commissioning
depends on `avahi-daemon` being present and running, and turning mDNS off
for a locked-down deployment removes Matter's only discovery path.

## 4. Security model

`octocam-matter.service` runs a daemon built from example-quality upstream
code that parses untrusted, pre-authentication Matter traffic from the
entire LAN on TCP/UDP 5540. It is deliberately **not** run as the shared
service user (root, the default for other OctoCam services in
`deploy-pi-web.sh`). Instead it runs as a dedicated, unprivileged
`octocam-matter` system user under a systemd sandbox. The key directives in
`systemd/octocam-matter.service`:

- `User=octocam-matter` / `Group=octocam-matter` — a dedicated system user
  created by `install.sh`, not root and not the shared web/service account.
- `NoNewPrivileges=yes` — the process can never gain privileges beyond what
  it starts with.
- `ProtectSystem=strict` with `ReadWritePaths=/var/lib/octocam/matter-storage`
  — the entire filesystem is read-only to the daemon except this one
  directory (its KVS and status file live here). This is narrower than the
  daemon strictly needs read access to elsewhere in `/var/lib/octocam`
  (identity and env files it consumes are handed to it via
  `EnvironmentFile`, not read directly from disk by the daemon), so the
  writable surface is kept to exactly the one directory it must write to.
- `ProtectHome=yes` — no access to any user home directory.
- `CapabilityBoundingSet=` — all Linux capabilities dropped.
- `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX AF_NETLINK` — the
  daemon can only use the socket families Matter, DNS-SD, and standard
  networking actually require.
- `MemoryMax=150M` — bounds the daemon's memory footprint (initial budget;
  tuned pending on-device measurement).
- `OOMScoreAdjust=500` — under memory pressure on the 512MB Pi, this
  optional daemon is killed by the OOM killer before mediamtx or
  `octocam-web`, since the core camera and control plane must survive.
- `LogRateLimitIntervalSec=30` / `LogRateLimitBurst=1000` — CHIP example
  code logs verbosely; this bounds log volume so a chatty or misbehaving
  daemon can't fill the disk or journal.
- `Restart=on-failure` with `RestartSec=5` — the daemon restarts with a
  backoff on crash rather than staying down.

**Persisted-passcode deviation.** OctoCam's general posture is to avoid
storing plaintext secrets on disk. The Matter identity file
(`/var/lib/octocam/matter-identity.json`) is a deliberate, documented
exception: it persists the actual Matter passcode rather than only a
SPAKE2+ verifier, because `octocam-web` needs the raw passcode to render the
QR code and manual pairing code on every page load without depending on the
daemon being up. Both the identity file and the daemon's environment file
(`/var/lib/octocam/matter-env`, which also carries the passcode so systemd
can inject it into the daemon's environment) are written with mode `0600`,
and the identity file is created with `O_CREAT`-safe permissions from the
moment it exists (an existing looser-mode file is deleted and recreated
rather than reused, so there's no window where it's briefly more permissive
than 0600). To limit the blast radius of this deviation, the passcode is
**rotated every time a user resets Matter pairing** rather than being a
permanent secret for the life of the device.

**Disable is not revoke.** Turning `matter_enabled` off stops the
`octocam-matter` service, but any ecosystem fabrics that were previously
commissioned remain valid in the daemon's persisted KVS. Re-enabling Matter
restores their access immediately — nothing about disabling the toggle
revokes prior pairings. The `/matter` page states this explicitly next to
the toggle, and shows a separate warning whenever the daemon's status file
reports `fabric_count > 0` while `matter_enabled` is `false` (for example,
after a `settings.json` wipe or a re-image leaves orphaned fabric state
behind). The only way to actually revoke access is **"Reset Matter
pairing,"** which stops the daemon, wipes the storage directory, rotates the
passcode, and restarts the daemon (if still enabled) with a fresh identity.

**Storage wipe preserves the directory.** "Reset Matter pairing" clears the
*contents* of `/var/lib/octocam/matter-storage` rather than removing and
recreating the directory itself. The directory is owned by the sandboxed
`octocam-matter` user (created by `install.sh` with `-o octocam-matter -m
750`); if `octocam-web` (running as a different user) deleted and recreated
it, ownership would flip to `octocam-web`'s user and lock the daemon out of
its own storage. If the directory is missing entirely (a fresh install, or
Matter was never enabled), reset creates it, but ownership repair in that
case is `install.sh`'s job, not the reset action's.

**Factory reset / resale guidance.** A device-wide factory reset wipes
Matter storage, HomeKit pairing state, and `settings.json` together, so a
resold or re-provisioned OctoCam doesn't carry over prior owners'
commissioned fabrics or credentials. This is file-level deletion; it does
not securely erase the underlying flash blocks on the SD card. Data
recoverable via raw flash/SD forensics after deletion is an accepted
residual risk for this class of device — anyone doing a genuinely
security-sensitive resale should treat the SD card itself as needing
physical destruction or a full low-level wipe, not just a settings reset.

## 5. Daemon contract

`octocam-web` and the Matter daemon binary are built and shipped
separately (see §6), so the interface between them is a fixed contract.
This is the same contract defined in the header of
`docs/superpowers/plans/2026-07-02-matter-control-plane.md`, reproduced here
as the reference for anyone building or debugging either side.

| Item | Value |
| --- | --- |
| Env file | `/var/lib/octocam/matter-env` (mode 0600, read by systemd via `EnvironmentFile=`) |
| Env keys | `OCTOCAM_MATTER_DISCRIMINATOR`, `OCTOCAM_MATTER_PASSCODE`, `OCTOCAM_MATTER_VENDOR_ID`, `OCTOCAM_MATTER_PRODUCT_ID`, `OCTOCAM_MATTER_RTSP_URL`, `OCTOCAM_MATTER_SNAPSHOT_URL` |
| KVS | `/var/lib/octocam/matter-storage/kvs` |
| Status file (daemon writes) | `/var/lib/octocam/matter-storage/status.json` with keys `status` (string), `commissioned` (bool), `fabric_count` (u32), `stream_state` (string), `error` (string) |
| Snapshot endpoint | `GET http://127.0.0.1:8081/internal/snapshot.jpg` — `200 image/jpeg` on success, `409` when the camera is disabled in settings, `503` when capture fails/times out; a cold capture (cache miss) can take up to 8 seconds, so the daemon's fetch timeout must exceed that |
| Identity | Vendor ID `0xFFF1` (65521, CSA test VID), Product ID `0x8001` (32769), 8-digit passcode (27-bit, spec-valid range, excluding the Matter spec's disallowed passcode list), 12-bit discriminator |

`octocam-web` renders the env file and restarts `octocam-matter` only when
the rendered content actually changes (mirroring the change-detection
pattern already used for mediamtx's own config), so routine settings saves
that don't touch Matter-relevant fields — resolution, path names, HomeKit
toggles, brightness, etc. — do not interrupt a live Matter WebRTC session.
The one exception is the daemon's `ExecStart` invocation: it also takes
`--secured-device-port 5540` and `--KVS /var/lib/octocam/matter-storage/kvs`
directly as CLI flags (not through the env file), since those never change
at runtime.

## 6. Build & deploy (Plan 2)

The daemon binary referenced by `systemd/octocam-matter.service` —
`dist/chip-camera-app` — **does not exist yet.** Everything described in
§§1–5 orchestrates a systemd unit and a contract file format; the actual
Matter protocol implementation is a separate, not-yet-completed piece of
work tracked in `docs/superpowers/plans/2026-07-02-matter-chip-fork.md`.
Until that binary is built and deployed, enabling `matter_enabled` will
configure and attempt to start a unit whose `ExecStart` target is missing;
systemd reports the unit as failed, and this is expected, accurate behavior
given the current state of the project, not a bug in the control plane.

At a high level, the plan for that binary is:

- A pinned fork of connectedhomeip (`octocam-connectedhomeip`, tracked in a
  separate repository) carries a small, deliberately upstreamable patch
  series on top of CHIP's Linux `camera-app` example: an RTSP-ingest media
  source (with reconnect/backoff, since mediamtx gets restarted during
  boot-time reconcile and `rtspsrc` otherwise errors out terminally), a
  snapshot-fetch path that pulls JPEGs from `octocam-web`'s loopback
  listener, and a status-file writer.
- The binary is cross-compiled on the Mac — never on the Pi — using CHIP's
  pinned `chip-build-crosscompile` Docker image, targeting
  `linux-arm64-camera-clang` via `build_examples.py`.
- The resulting binary must link against no more than the Raspberry Pi OS
  Bookworm glibc (2.36) and Bookworm's GStreamer ABI, since that's what
  ships on the target hardware. If the cross-compile image's sysroot
  exceeds that, the fallback is a native `debian:bookworm` arm64 container
  run on Apple Silicon.
- The stripped binary is deployed via `rsync -z --partial`, following the
  same cross-build-on-Mac-then-rsync-to-Pi workflow used for the rest of
  OctoCam.

See `docs/superpowers/plans/2026-07-02-matter-chip-fork.md` for the full
plan, including the empirically-derived runtime dependency list (GStreamer
plugins, `avahi-daemon` + D-Bus) that has to land in `install.sh` before the
daemon can actually run.

## 7. Open verifications

The following items are called out in the design spec as needing empirical
confirmation during (or before) the daemon build lands; none of them are
resolved by the control-plane work this document otherwise describes.

1. **CHECK-1**: Confirm the glibc/GLIBCXX version of the cross-compile
   image's sysroot against the Pi's Bookworm glibc (`readelf -V`); this
   gates whether the `debian:bookworm` fallback build path is needed.
2. **CHECK-2**: Determine camera-app's actual WebRTC stack and required
   GStreamer plugin set empirically (`ldd` the binary, run with
   `GST_DEBUG=3` on a clean image) — in particular whether
   `gstreamer1.0-nice` and additional DTLS/SRTP crypto dependencies are
   needed.
3. **CHECK-3**: Confirm the DNS-SD backend's gn build args and verify
   :5353 coexistence between `avahi-daemon`, the HomeKit daemon's ciao
   responder, and CHIP's DNS-SD client on the actual Pi.
4. **CHECK-4**: Confirm an IPv6 link-local address is actually present on
   `wlan0` on the current OctoCam OS image (not just assumed by the
   preflight check).
5. **CHECK-5**: Measure RSS/CPU under 2 concurrent WebRTC sessions and
   under sustained snapshot polling, to set the final `MemoryMax` (and
   decide whether the session cap needs to drop to 1).
6. **CHECK-6**: Confirm CHIP's default commissioning-window behavior when
   the daemon is enabled but not yet commissioned.
7. **CHECK-7**: Confirm whether camera-app enforces the 2-concurrent-
   session cap in-protocol, or whether OctoCam needs to enforce it itself.
8. **CHECK-8**: Measure KVS write frequency during a soak test
   (`fatrace`) to assess SD card wear impact.
9. **CHECK-9**: Empirically document the Home Assistant Matter Server 9.0
   uncertified/test-VID override flow needed to commission an OctoCam
   (test-VID `0xFFF1`) into Home Assistant at all.
