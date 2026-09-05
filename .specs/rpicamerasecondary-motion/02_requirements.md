# Requirements: rpiCameraSecondary as the Motion-Detection Source

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Discovery](01_discovery.md) · [Requirements](02_requirements.md) · [Design](03_design.md) · [Tasks](04_tasks.md) · [Execution](05_execution.md)
<!-- spec-nav:end -->

## Introduction

Motion detection currently reads the full-resolution `main` H264 stream and pays a multi-second
decoder cold-start on every reconnect. Now that upstream mediamtx has fixed the crash that
previously blocked using its hardware `rpiCameraSecondary` MJPEG stream (see
[01_discovery.md](01_discovery.md)), this feature lets motion detection use that near-instant
stream instead, while never losing motion coverage and never regressing the stream's existing
consumers (the Matter/HomeKit bridge and the web UI's live view).

**Actors used below:** `Motion_Detector` (the motion-detection loop), `Mediamtx_Config_Generator`
(the code that renders `/etc/mediamtx.yml`), `Octocam_RTSP_Service` (the `mediamtx` process
managed as the `octocam-rtsp` systemd unit), `Octocam_Web` (the backend settings/API service),
`Settings_UI` (the frontend settings page).

> [!IMPORTANT]
> Approval gate: approve these requirements before work begins on [03_design.md](03_design.md).

## Requirements

### Requirement 1: Motion-detection source selection

**User Story:** As an OctoCam operator, I want motion detection to use the fast hardware
secondary stream whenever it's safely available, so that the camera recovers visibility faster
after a reconnect without ever losing motion coverage.

#### Acceptance Criteria

1. **R1.1** WHEN motion detection starts a new capture session AND `motion_use_secondary_stream` is enabled AND `sub_stream_enabled` is enabled AND `text_overlay_enabled` is disabled, THE Motion_Detector SHALL connect to the secondary (`sub`) RTSP path as its frame source.
2. **R1.2** IF `motion_use_secondary_stream` is disabled, OR `sub_stream_enabled` is disabled, OR `text_overlay_enabled` is enabled, THEN THE Motion_Detector SHALL connect to the `main` RTSP path as its frame source.
3. **R1.3** WHEN a settings change updates `motion_use_secondary_stream`, `sub_stream_enabled`, or `text_overlay_enabled` while motion detection is running, THE Motion_Detector SHALL apply the resulting source-selection outcome (R1.1 or R1.2) starting from its next reconnect.
4. **R1.4** IF the frame source selected for the current session disconnects or fails, THEN THE Motion_Detector SHALL apply its existing reconnect/backoff behavior against that same source, and SHALL NOT switch to the other source mid-session.
5. **R1.5** THE Motion_Detector SHALL always resolve to exactly one of `main` or the secondary (`sub`) path as its frame source; no code path SHALL leave the source unresolved.

### Requirement 2: Correct secondary-stream codec in generated mediamtx configuration

**User Story:** As an OctoCam operator, I want the generated mediamtx configuration to request
an MJPEG secondary stream (not hardware H264), so that the secondary stream actually delivers the
fast-reconnect, low-decode-cost behavior motion detection depends on.

#### Acceptance Criteria

1. **R2.1** THE Mediamtx_Config_Generator SHALL render any path configured with `rpiCameraSecondary: true` using the codec configuration that yields an MJPEG stream, never the hardware H264 codec.
2. **R2.2** THE Mediamtx_Config_Generator SHALL include an explicit MJPEG quality setting on any path configured with `rpiCameraSecondary: true`.
3. **R2.3** IF a path is the primary (non-secondary) `rpiCamera` path, THEN THE Mediamtx_Config_Generator SHALL continue to render it with the hardware H264 codec, unchanged from today's behavior.

### Requirement 3: Startup reliability with the secondary stream configured

**User Story:** As an OctoCam operator, I want the camera service to start reliably every time
the secondary stream is configured, so that enabling this feature can never turn into a boot-loop
on my device.

#### Acceptance Criteria

1. **R3.1** THE Octocam_RTSP_Service SHALL start successfully, with no startup crash, when its configuration includes a secondary `rpiCamera` path, across at least 10 consecutive restarts.
2. **R3.2** IF the running mediamtx instance does not reliably satisfy R3.1 with the corrected secondary-stream configuration in place, THEN THE Octocam_Web SHALL prevent `motion_use_secondary_stream` from taking effect and SHALL keep the Motion_Detector on the `main` source.

### Requirement 4: No regression for existing secondary-stream consumers

**User Story:** As an OctoCam operator, I want the Matter/HomeKit live view and the web UI's live
view to keep working exactly as they do today, so that adding motion detection as a new reader of
the secondary stream doesn't cost me an existing feature.

#### Acceptance Criteria

1. **R4.1** WHILE the Motion_Detector is reading the secondary stream, THE secondary path SHALL continue to serve the Matter/HomeKit bridge's live-view stream and JPEG snapshot capture without interruption attributable to the additional reader.
2. **R4.2** WHILE the Motion_Detector is reading the secondary stream, THE secondary path SHALL continue to serve the web UI's live-view stream without interruption attributable to the additional reader.

### Requirement 5: Diagnostics reflect the active source

**User Story:** As an OctoCam operator, I want to see which video source motion detection is
currently using and trust its existing health metrics, so that I can troubleshoot latency or
reliability issues without guessing.

#### Acceptance Criteria

1. **R5.1** WHEN the Motion_Detector connects to a frame source, THE Motion_Detector SHALL include which source (`main` or secondary) is currently active in its existing status/health output.
2. **R5.2** THE Motion_Detector's existing blind-time and availability measurements SHALL remain accurate regardless of whether `main` or the secondary path is the active source.

### Requirement 6: Configuration toggle

**User Story:** As an OctoCam operator, I want an explicit setting to control whether motion
detection may use the secondary stream, so that I can roll back to the known-good `main`-only
behavior without a code change if something looks wrong.

#### Acceptance Criteria

1. **R6.1** THE Settings_UI SHALL expose a user-configurable control for `motion_use_secondary_stream` in the motion-detection settings area.
2. **R6.2** THE Octocam_Web SHALL default `motion_use_secondary_stream` to disabled (false) for both new and existing installations, so behavior does not change until an operator explicitly opts in.

## Assumptions and Deferred Decisions

- **R6.2's conservative default is deliberate for this session:** the on-device feasibility test
  required by R3.1 has not yet been run (see [01_discovery.md](01_discovery.md), Open Decision 1).
  Shipping with the toggle defaulted off lets the feature merge safely; flipping the default to
  true is a separate, later decision once an operator has verified R3.1/R3.2 on the actual
  device, not part of this requirements set.
- **mediamtx version detection mechanism** (how `Octocam_Web` determines whether R3.1 is
  satisfied for R3.2) is deliberately left unspecified here — it's a design decision, not an
  observable product requirement.
- **No numeric performance target is set** for time-to-first-frame or CPU cost. The discovery
  found the previously quoted baseline numbers (~78% CPU, ~2.6s) unverifiable in this
  repository; this requirements set intentionally does not fabricate a threshold. A fresh,
  real measurement is a verification activity for the design/tasks phases, not a testable
  criterion here.

## Approval

Status: **Approved on 2026-09-05**.
