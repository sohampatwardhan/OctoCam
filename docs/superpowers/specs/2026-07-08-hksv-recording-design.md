# HomeKit Secure Video (HKSV) Recording — Design

**Date:** 2026-07-08
**Status:** Approved for planning
**Primary use case:** Motion-triggered HKSV clips recorded to the Apple Home Hub (iCloud), so OctoCam behaves like a first-class HomeKit Secure Video camera.

## Summary

Add HKSV recording to the **existing** Node.js HomeKit bridge
([homekit/octocam-homekit.js](../../../homekit/octocam-homekit.js)) rather than
reimplementing the HomeKit Accessory Protocol natively in Rust.

The original Phase-3 idea was a native Rust HAP+HKSV stack in `octocam-web`. That
was reconsidered: `hap-nodejs` (already a dependency, pinned `0.14.3`) has shipped
full HKSV support for years — `CameraController`'s `recording` option, the
`CameraRecordingDelegate`, and the HomeKit Data Stream (HDS) transport for clip
transfer are all built in and battle-tested by the Homebridge camera ecosystem. A
from-scratch Rust reimplementation (SRP6a pairing, Ed25519, ChaCha20-Poly1305,
mDNS advertisement, TLV-over-TCP, *and* the HDS binary protocol) is a multi-month
effort with no mature Rust HKSV crate to build on, all targeting a 512 MB Pi Zero
2 W. Extending the working bridge reuses everything Phase 1 (Rust motion
detection) and Phase 2 (Node bridge with camera + `MotionSensor`) already
delivered and tested.

On motion, the bridge streams a fragmented-MP4 (fMP4) H.264 clip to the Apple
Home Hub over the existing HAP pairing session. The Home Hub stores clips
encrypted in iCloud. **No local clip storage. No prebuffer. Video only.**

## How HKSV responsibilities split

| Concern | Owner |
|---|---|
| Is recording *armed*? | The Home app / Home Hub (its own "Record on Motion" HKSV toggle). Requires a Home Hub + an iCloud+ plan with HKSV storage. |
| What *triggers* a recording? | The `MotionSensor` service already shipped in Phase 2, fed by `motion.rs`'s SSE stream. |
| Producing the clip bytes | This feature: an `ffmpeg` process pulling from mediamtx, encoding fMP4 fragments streamed back through hap-nodejs's `CameraRecordingDelegate`. |
| Storing / classifying clips (person, vehicle, package) | The Home Hub + iCloud. The Pi does no local AI. |

## Scope decisions

| Decision | Choice | Rationale |
|---|---|---|
| Native Rust HAP vs. extend Node bridge | **Extend the Node bridge** | `hap-nodejs` already implements HKSV/HDS; native Rust is multi-month with no HKSV crate. Reuses tested Phase 1+2 work. |
| Local clip copy | **None** | Match stock HKSV: clips go to the Home Hub / iCloud only. No retention/cleanup logic, no extra disk I/O on the Pi. |
| Prebuffer (pre-trigger lead-in) | **None (advertise 0 s)** | A prebuffer requires an always-on recording-quality background encode 24/7. Skipping it means zero idle CPU/power cost; clips begin at the motion trigger instant. |
| Recording resolution / bitrate / fps | **Admin-configurable in settings** | Lets the operator tune quality against real-world Pi Zero 2 W CPU load rather than hardcoding. Not negotiated with the hub. |
| Concurrency (live view + recording) | **Both run, no throttling** | Recording is the point of the feature — never skip it. Overlap with live view is rare and short; accept possible frame drops over scheduling complexity. |
| Audio | **Video only** | No microphone hardware on the OctoCam. |
| Recording event triggers | **Motion only** | No doorbell/periodic hardware or use case. |

### Non-goals

- No native Rust HAP or HKSV implementation.
- No local storage / NVR of clips; no retention policy.
- No prebuffer / pre-roll footage.
- No audio capture.
- No hub-negotiated adaptive bitrate — quality comes from OctoCam settings.
- No live-view/recording CPU scheduling or throttling.
- No changes to the Rust motion-detection pipeline (Phase 1 stays as-is).

## Components

### 1. `homekit/octocam-homekit.js` (primary change)

Extend the existing `CameraController` construction (currently around
[octocam-homekit.js:583](../../../homekit/octocam-homekit.js#L583)) with a
`recording` configuration:

- `options.recording.prebufferLength: 0`
- Event triggers: **MOTION** only.
- `mediaContainerConfiguration`: fragmented MP4.
- Supported video codec parameters advertised to match what the Pi can encode
  (H.264, profile/level consistent with the existing live-view `ffmpeg` config).
- A `RecordingManagement` sensor association so the recording stream is bound to
  the existing `MotionSensor` service.

Add a `CameraRecordingDelegate` with:

- **`handleRecordingStreamRequest(streamId)`** — an `async` generator. Spawns an
  `ffmpeg` process pulling from mediamtx over RTSP (same source-selection pattern
  as the live-view `buildStreamArgs`), encoding to
  `-f mp4 -movflags frag_keyframe+empty_moov+default_base_moof` (or the
  hap-nodejs-required flags for HKSV fMP4). Reads ffmpeg stdout, parses the
  initialization segment (`ftyp`+`moov`) then successive `moof`+`mdat`
  fragments, and `yield`s each fragment as hap-nodejs requests it. Resolution /
  bitrate / fps come from the admin-configured HKSV settings (fetched via the
  existing `loadSettings()` / settings-fetch path the bridge already uses).
- **`closeRecordingStream(streamId, reason)`** — kills that ffmpeg child.
- **`updateRecordingActive(active)`** — store hub-provided armed state.
- **`updateRecordingConfiguration(config)`** — store hub-provided config; OctoCam
  does not derive encode parameters from it (quality is admin-set), but the
  callback must still be honored for the HDS handshake.

### 2. `rust/octocam-web/src/settings.rs`

Add fields following the existing `sub_bitrate_kbps` / `sub_framerate` pattern
(so [backup.rs](../../../rust/octocam-web/src/backup.rs) coverage test picks them
up automatically):

- `hksv_enabled: bool` (default `false`)
- `hksv_width: i32`, `hksv_height: i32`
- `hksv_bitrate_kbps: i32`
- `hksv_fps: i32`

Validation: `hksv_enabled == true` requires `motion_enabled == true` (recording is
motion-triggered; arming without motion is meaningless). Numeric fields validated
with the same bounds style as existing stream fields.

### 3. `rust/octocam-web/templates/stream_settings.html`

New "HomeKit Secure Video" panel following the existing settings-panel markup:
enable toggle plus resolution / bitrate / fps inputs, wired into the existing
`_checkboxes` / form-submit plumbing. Panel hints that a Home Hub + iCloud+ HKSV
storage is required, and that motion detection must be enabled.

## Data flow (recording lifecycle)

1. Motion fires → `motion.rs` flips the atomic and broadcasts on SSE (**already
   built**) → the bridge's SSE client sets `MotionDetected = true` (**already
   built**).
2. If HKSV is armed in the Home app, the Home Hub opens an HDS recording stream →
   hap-nodejs invokes `handleRecordingStreamRequest(streamId)`.
3. The delegate spawns `ffmpeg` against mediamtx at the admin-configured HKSV
   quality and streams fMP4 fragments back through the async generator as
   hap-nodejs consumes them.
4. The hub closes the stream (motion cleared, max clip length, or user limit) →
   `closeRecordingStream` kills the ffmpeg child. No fragments are buffered
   beyond what hap-nodejs actively pulls; nothing is written to disk.

## Error handling

- **ffmpeg fails to start or dies mid-recording:** the generator ends early. The
  hub receives a short/incomplete clip — consistent with how transient camera
  errors already degrade elsewhere in HKSV. **No retry loop**, since a stuck retry
  could hold a stale ffmpeg process across an entire motion event.
- **HKSV enabled but motion disabled:** blocked at settings validation, so the
  bridge never advertises a recording capability it can't trigger.
- **Bridge can't reach `octocam-web` settings:** fall back to conservative
  built-in defaults for encode parameters (mirrors the bridge's existing
  `loadSettings()` failure behavior) so recording still functions.

## Testing / verification

- **Automated:** `cargo test` — the settings-field additions are covered by the
  existing backup-coverage test (84 tests today; adding fields must keep it
  green). No new Rust control-flow beyond settings plumbing.
- **Manual (unavoidable — the Home app can't be automated):**
  1. Deploy to the Pi; enable motion detection and HKSV in the OctoCam UI.
  2. In the Home app, enable "Secure Video" recording for the camera (requires a
     configured Home Hub + iCloud+ HKSV storage).
  3. Trigger motion; confirm a clip appears in the camera's Home-app history with
     no lead-in before the trigger frame.
  4. Confirm live view still works during and after a recording.
