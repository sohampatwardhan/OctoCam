# HomeKit Secure Video (HKSV) Recording — Design

**Date:** 2026-07-08
**Status:** Approved for planning (hardened 2026-07-08)
**Primary use case:** Motion-triggered HKSV clips recorded to the Apple Home Hub (iCloud), so OctoCam behaves like a first-class HomeKit Secure Video camera.

## Summary

Add HKSV recording to the **existing** Node.js HomeKit bridge
([homekit/octocam-homekit.js](../../../homekit/octocam-homekit.js)) rather than
reimplementing the HomeKit Accessory Protocol natively in Rust.

The original Phase-3 idea was a native Rust HAP+HKSV stack in `octocam-web`. That
was reconsidered: `hap-nodejs` (already a dependency, pinned `0.14.3`) has shipped
full HKSV support for years. Its API surface was verified directly against the
`v0.14.3` git tag: `CameraControllerOptions.recording` (`{ options, delegate }`),
`CameraControllerOptions.sensors.motion`, and the `CameraRecordingDelegate`
(`updateRecordingActive`, `updateRecordingConfiguration`,
`handleRecordingStreamRequest`, `closeRecordingStream`) are all present. **No
version bump is required** — which matters because the Pi deploy explicitly must
not run `npm install` on-device. A from-scratch Rust reimplementation (SRP6a,
Ed25519, ChaCha20-Poly1305, mDNS, TLV-over-TCP, *and* the HDS binary protocol) is
a multi-month effort with no mature Rust HKSV crate, all on a 512 MB Pi Zero 2 W.
Extending the working bridge reuses everything Phase 1 (Rust motion detection) and
Phase 2 (Node bridge with camera + `MotionSensor`) already delivered and tested.

On motion, the bridge streams a fragmented-MP4 (fMP4) H.264 clip to the Apple
Home Hub over the existing HAP pairing session. The Home Hub stores clips
encrypted in iCloud. **No local clip storage. No prebuffer (pending hardware
verification). Video only.**

## How HKSV responsibilities split

| Concern | Owner |
|---|---|
| Is recording *armed*? | The Home app / Home Hub (its own "Record on Motion" HKSV toggle). Requires a Home Hub + an iCloud+ plan with HKSV storage. |
| What *triggers* a recording? | The `MotionSensor` service already shipped in Phase 2, fed by `motion.rs`'s SSE stream, now **linked to the recording management** via `sensors.motion`. |
| Producing the clip bytes | This feature: an `ffmpeg` process pulling from mediamtx, encoding fMP4 fragments **at the parameters the hub negotiated**, streamed back through hap-nodejs's `CameraRecordingDelegate`. |
| Choosing recording quality | The Home Hub, negotiated from a fixed list of Pi-safe configurations OctoCam advertises. |
| Storing / classifying clips (person, vehicle, package) | The Home Hub + iCloud. The Pi does no local AI. |

## Scope decisions

| Decision | Choice | Rationale |
|---|---|---|
| Native Rust HAP vs. extend Node bridge | **Extend the Node bridge** | `hap-nodejs` already implements HKSV/HDS (verified against v0.14.3). Native Rust is multi-month with no HKSV crate. Reuses tested Phase 1+2 work. |
| Local clip copy | **None** | Match stock HKSV: clips go to the Home Hub / iCloud only. No retention/cleanup logic, no extra disk I/O. |
| Prebuffer (pre-trigger lead-in) | **None (advertise 0 s), pending hardware check** | Avoids an always-on 24/7 encode. hap-nodejs docs say the prebuffer "must be at least 4000 ms"; no runtime enforcement was found, so 0 s is attempted first and **verified against a real Home Hub as the first gate** (see Testing). If rejected, revisit a rolling-buffer design. |
| Recording resolution / bitrate / fps | **Derived from the hub-negotiated configuration** | HKSV requires the fMP4 track parameters (resolution / profile / level / keyframe interval) to match what the hub selected from the advertised list; diverging causes silently-aborted recordings and an "unreliable" flag. `updateRecordingConfiguration()` drives ffmpeg encode params, as Homebridge camera plugins do. **Not admin-configurable.** |
| Advertised recording configurations | **Fixed, Pi-safe list in code** | OctoCam advertises a small set of resolutions/bitrates the Pi can software-encode within H.264 Level 4.0 (consistent with the existing live-view level cap). The hub can only pick a compliant option. |
| Concurrency (live view + recording) | **Both run, no application throttling; contained by cgroup limits** | Recording is the point of the feature — never skip it. Overlap is rare and short. Rather than app-level scheduling, `MemoryMax`/`CPUQuota` on the bridge's systemd unit prevent a runaway encode from OOM-killing mediamtx (the shared video source). |
| Audio | **Video only, with a placeholder audio codec config** | No microphone hardware. But `CameraRecordingOptions.audio` is a *required* field in hap-nodejs 0.14.3 (constructor throws if omitted/empty), so a minimal disabled/dummy audio config is supplied structurally. |
| Recording event triggers | **Motion only** | No doorbell/periodic hardware or use case. |

### Non-goals

- No native Rust HAP or HKSV implementation.
- No local storage / NVR of clips; no retention policy.
- No prebuffer / pre-roll footage (unless hardware verification forces it).
- No audio capture (placeholder codec config only).
- No admin-configurable recording quality — the hub negotiates it.
- No live-view/recording application-level scheduling (cgroup limits only).
- No changes to the Rust motion-detection pipeline (Phase 1 stays as-is).

## Components

### 1. `homekit/octocam-homekit.js` (primary change)

Extend the existing `CameraController` construction (currently around
[octocam-homekit.js:583](../../../homekit/octocam-homekit.js#L583)):

- Add **`sensors: { motion: motionService }`**, reusing the exact `Service`
  instance already created at [octocam-homekit.js:580](../../../homekit/octocam-homekit.js#L580).
  This links the existing `MotionSensor` to the `RecordingManagement` and
  auto-derives `EventTriggerOption.MOTION`. Passing `true` instead would make
  hap-nodejs create a *duplicate* internal sensor — must pass the instance.
- Add **`recording: { options, delegate }`** where `options` is a
  `CameraRecordingOptions`:
  - `prebufferLength: 0` (see Testing gate).
  - `mediaContainerConfiguration`: fragmented MP4.
  - `video`: a fixed list of Pi-safe H.264 configs (≤ Level 4.0).
  - `audio`: a **required** minimal placeholder codec config (constructor throws
    otherwise — this crashes the whole bridge at startup, not just HKSV).

Add a `CameraRecordingDelegate`:

- **`updateRecordingConfiguration(config)`** — store the hub-selected config; the
  encode parameters (resolution/bitrate/fps/profile/level) come from **here**,
  not from OctoCam settings.
- **`handleRecordingStreamRequest(streamId)`** — an `async` generator. Spawns an
  `ffmpeg` process pulling from mediamtx over RTSP (same source-selection pattern
  as the live-view `buildStreamArgs`), encoding to
  `-f mp4 -movflags frag_keyframe+empty_moov+default_base_moof` at the negotiated
  parameters. Parses the init segment (`ftyp`+`moov`) then successive `moof`+`mdat`
  fragments, `yield`ing each as hap-nodejs requests it. Must wrap spawn+kill in
  `try/finally` so the ffmpeg child is reaped on the generator's `.return()`/
  `.throw()` paths, not only via `closeRecordingStream`.
- **`closeRecordingStream(streamId, reason)`** — kill that ffmpeg child.
- **`updateRecordingActive(active)`** — store hub-provided armed state.

### 2. `rust/octocam-web/src/settings.rs`

Add a single field (quality is hub-negotiated, so no width/height/bitrate/fps):

- `hksv_enabled: bool` (default `false`)

Cross-field invariant: `hksv_enabled == true` requires `motion_enabled == true`.
Implement as a dedicated **`enforce_hksv_requires_motion(&mut Settings)`**
mirroring the existing `enforce_matter_requires_admin`, called from **both**
`update_settings` (main.rs, ~line 1535) and `parse_restore`
(backup.rs, ~line 194) — not only inside `validate_map`, which validates fields
independently and would miss a restored backup that flips the pairing.

### 3. `rust/octocam-web/src/backup.rs`

Add `hksv_enabled` to **`PORTABLE_FIELDS`**. This is mandatory, not automatic:
`field_lists_cover_all_settings` ([backup.rs:212](../../../rust/octocam-web/src/backup.rs#L212))
is a fail-closed allow-list — a new `Settings` field absent from both
`PORTABLE_FIELDS` and `EXCLUDED_FIELDS` makes `cargo test` **panic**. Also add the
`enforce_hksv_requires_motion` call in `parse_restore` (see Component 2).

### 4. `rust/octocam-web/src/mediamtx.rs`

Raise the RTSP reader budget. Today `maxReaders = rtsp_max_clients + reserve`
where `reserve = homekit_enabled + matter_enabled`
([mediamtx.rs:130-146](../../../rust/octocam-web/src/mediamtx.rs#L130)) — a flat
+1 per daemon. But the HomeKit bridge already advertises `cameraStreamCount: 2`
(two live-view pulls), `motion.rs` holds a persistent reader, and recording adds a
third bridge reader. With the default `rtsp_max_clients: 1`, a live view + a
concurrent recording exceeds the budget and mediamtx **refuses** the connection —
breaking the "both run" decision. Scale `reserve` to the real maximum concurrent
internal readers (motion + `cameraStreamCount` + 1 for recording).

### 5. `systemd/octocam-homekit.service`

Add `MemoryMax=` and `CPUQuota=` directives so a runaway/concurrent ffmpeg encode
is contained to the bridge unit rather than triggering the OOM killer against
mediamtx or octocam-web (a full camera outage). Values tuned for the Pi Zero 2 W's
512 MB budget, leaving headroom for mediamtx + octocam-web.

### 6. `rust/octocam-web/templates/stream_settings.html`

New "HomeKit Secure Video" panel: a single enable toggle (no quality inputs — the
hub negotiates quality), wired into the existing `_checkboxes`/form-submit
plumbing. Hint that a Home Hub + iCloud+ HKSV storage is required and that motion
detection must be enabled.

## Data flow (recording lifecycle)

1. Motion fires → `motion.rs` flips the atomic and broadcasts on SSE (**already
   built**) → the bridge's SSE client sets `MotionDetected = true` (**already
   built**).
2. If HKSV is armed in the Home app, the Home Hub opens an HDS recording stream,
   selecting a configuration from the advertised list → hap-nodejs invokes
   `updateRecordingConfiguration(config)` then `handleRecordingStreamRequest`.
3. The delegate spawns `ffmpeg` against mediamtx at the **negotiated** parameters
   and streams fMP4 fragments back through the async generator as hap-nodejs
   consumes them.
4. The hub closes the stream (motion cleared, max clip length, or user limit) →
   `closeRecordingStream` kills the ffmpeg child; the generator's `finally` also
   reaps it on any early exit. No fragments are buffered beyond what hap-nodejs
   actively pulls; nothing is written to disk.

## Error handling

- **ffmpeg fails to start or dies mid-recording:** the generator ends early (via
  `try/finally` reaping the child). The hub receives a short/incomplete clip —
  consistent with how transient camera errors already degrade in HKSV. **No retry
  loop**, so a stuck retry can't hold a stale ffmpeg process across a motion event.
- **HKSV enabled but motion disabled:** blocked at settings validation
  (`enforce_hksv_requires_motion`), so the bridge never advertises a recording
  capability it can't trigger.
- **Resource exhaustion under concurrency:** contained by the systemd cgroup
  limits (Component 5) — the bridge unit is throttled/capped rather than allowed
  to OOM-kill the shared mediamtx source.
- **Bridge can't read settings file:** `loadSettings()` is a synchronous local
  file read with try/catch; on failure it falls back to built-in defaults so
  `hksv_enabled` simply reads false and recording is not advertised.

## Testing / verification

- **Automated:** `cargo test` — the `hksv_enabled` addition must be reflected in
  `PORTABLE_FIELDS` (Component 3) or the existing coverage test panics; the
  `enforce_hksv_requires_motion` invariant gets a unit test alongside the existing
  `validates_*` tests (84 tests today, must stay green).
- **Manual, in order (the Home app can't be automated):**
  1. **Prebuffer gate (first):** deploy, enable HKSV, and confirm a real Home Hub
     accepts `prebufferLength: 0` and opens a recording stream. If it rejects 0 s,
     stop and revisit the prebuffer decision before building further.
  2. Trigger motion; confirm a clip appears in the camera's Home-app history,
     beginning at the trigger frame (no lead-in).
  3. Confirm the negotiated configuration produces a valid, playable clip (not
     silently aborted / camera not flagged "unreliable").
  4. With a live-view session active, trigger a recording and confirm both
     function and that the bridge stays within its cgroup limits (no OOM of
     mediamtx / octocam-web).
