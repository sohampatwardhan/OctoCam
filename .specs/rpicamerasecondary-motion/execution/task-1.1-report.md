# Task 1.1 Report: Confirm device's secondary stream + mediamtx version

**Status:** verified (read-only reconnaissance, no device changes made)

## Findings

1. **mediamtx version on the device: `v1.19.2`** — this is *below* the `>= v1.20.1` floor
   (`MIN_SECONDARY_SAFE_VERSION`) and is exactly the version range affected by the original
   crash regression (bluenviron/mediamtx#6060; fixed in the v1.20.1 tag). Confirmed via
   `mediamtx --version`.
2. **Current live settings:** `sub_stream_enabled: true`, `text_overlay_enabled: true`,
   `rtsp_path: "main"`, `sub_rtsp_path: "sub"`, `motion_enabled: true`. Read from
   `/var/lib/octocam/settings.json` (not `~/.config/octocam/settings.json` — the systemd
   service's effective config path resolves differently than the crate's documented default;
   worth noting for later, not a blocker here).
3. **Current `/etc/mediamtx.yml`'s `sub` path is the *software* transcoder, not the hardware
   secondary stream** — confirmed directly: `"sub": { source: publisher, runOnDemand: "ffmpeg
   ... -c:v libx264 ..." }`. Because `text_overlay_enabled` is `true` on this device today,
   [`mediamtx.rs:165-192`](../../rust/octocam-web/src/mediamtx.rs) never reaches the
   `rpiCameraSecondary: true` branch task 1.3 will fix — so task 1.3's codec change has **zero
   effect on this device's currently-rendered config** as long as text overlay stays enabled.
4. **Environment:** binary at `/usr/local/bin/mediamtx` (62,164,043 bytes, dated 2026-06-28,
   matching the v1.19.2 release date), not apt-managed — a plain downloaded binary, managed
   only by the `octocam-rtsp` systemd unit (`ExecStart=/usr/local/bin/mediamtx /etc/mediamtx.yml`).
   Architecture `aarch64`. Device reaches `github.com` release assets directly (`HTTP 200`,
   confirmed for the exact `v1.20.1`/`linux_arm64` asset), and has 50GB free disk.

## Impact on remaining tasks

Finding 1 means **the restart test cannot proceed as originally scoped**: testing the corrected
secondary-stream config against the *currently installed* v1.19.2 binary would very likely
reproduce the original crash (the fix isn't in that binary yet), which is unacceptable on a live
device. `04_tasks.md` is repaired (Autonomous task-list repair) to insert a new task 2.1
("Upgrade device mediamtx binary to >= v1.20.1") ahead of the restart test, which shifts to
task 3.1 (and the former task 3.1, call-site wiring, shifts to 3.2) — see the Change Control
note below. This is not a new requirement: R3.2 and [01_discovery.md](01_discovery.md)'s Open
Decision 1 already anticipated needing to confirm/upgrade the device's mediamtx version; this
finding is what makes that concrete.

Finding 3 substantially de-risks task 1.3's deployment: shipping the corrected codec-generation
code to this device changes nothing observable today (text overlay is on), so task 1.3 can land
safely regardless of the on-device secondary-stream test's outcome — R6.2's default-off toggle
and this device's own `text_overlay_enabled: true` are two independent reasons the corrected
secondary path won't actually activate until an operator deliberately changes settings.
