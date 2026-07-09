# HomeKit Secure Video (HKSV) Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add motion-triggered HomeKit Secure Video recording to OctoCam by extending the existing Node.js `hap-nodejs` bridge, so a HomePod-mini Home Hub records clips to iCloud on motion.

**Architecture:** The Rust `octocam-web` backend gains one `hksv_enabled` setting (quality is hub-negotiated, not admin-set), a cross-field invariant (`hksv_enabled ⇒ motion_enabled`), an extra mediamtx reader-slot reservation for the concurrent recording pull, and a settings-UI toggle. The Node bridge adds a `CameraRecordingDelegate` to the existing `CameraController`: on a hub recording request it spawns `ffmpeg` against the local mediamtx main stream, encodes fragmented-MP4 at the **hub-negotiated** parameters, and yields each fragment through hap-nodejs's HomeKit Data Stream transport. A systemd cgroup cap contains concurrent-encode resource use on the Pi Zero 2 W. Prebuffer is 0 (no always-on encode), gated on a hardware verification step.

**Tech Stack:** Rust (axum, serde) for `octocam-web`; Node.js + `hap-nodejs@0.14.3` (no version bump) for the bridge; `ffmpeg` (libx264) for encoding; systemd for process supervision.

**Spec:** `docs/superpowers/specs/2026-07-08-hksv-recording-design.md`

**Reference — verified facts this plan depends on:**
- `hap-nodejs@0.14.3` already exposes `CameraControllerOptions.recording` `{ options, delegate }`, `CameraControllerOptions.sensors.motion`, and the full `CameraRecordingDelegate` (verified in `homekit/node_modules/hap-nodejs/dist/lib/`). **No version bump / no npm-on-Pi.**
- `CameraRecordingOptions.audio` is **required**; an empty codec list throws `"CameraRecordingOptions.audio: At least one audio codec configuration must be specified!"` at construction (`RecordingManagement.js:422`). A placeholder AAC config is mandatory even though we send no audio.
- `prebufferLength: 0` is encoded without a runtime throw (only a doc-level "≥4000ms" note) — hence the hardware-verification gate in Task 8.
- Passing `sensors: { motion: <Service instance> }` reuses the sensor; passing `true` creates a **duplicate** internal sensor (`CameraController.js:164` vs `:167`).
- `handleRecordingStreamRequest(streamId, signal?)` returns `AsyncGenerator<RecordingPacket>`; `RecordingPacket = { data: Buffer, isLast: boolean }`; first packet must be the init segment (`ftyp`+`moov`), then `moof`+`mdat` fragments.

---

## File Structure

**Rust backend (`rust/octocam-web/src/`):**
- `settings.rs` — add `hksv_enabled` field, default, parse, and `enforce_hksv_requires_motion()`.
- `backup.rs` — add `hksv_enabled` to `PORTABLE_FIELDS`; call the new enforcement in `parse_restore`.
- `main.rs` — call the new enforcement in `update_settings`.
- `mediamtx.rs` — raise the RTSP reader reserve when HKSV can run concurrently.
- `templates/stream_settings.html` — HKSV enable toggle.

**Bridge (`homekit/`):**
- `octocam-homekit.js` — fragmented-MP4 box splitter, `CameraRecordingDelegate`, recording options, `sensors.motion` wiring.

**Ops:**
- `systemd/octocam-homekit.service` — `MemoryMax` / `CPUQuota` cgroup limits.

---

## Task 1: Add the `hksv_enabled` setting

**Files:**
- Modify: `rust/octocam-web/src/settings.rs` (struct ~line 61, defaults ~line 209, `validate_map` ~line 389)
- Modify: `rust/octocam-web/src/backup.rs` (`PORTABLE_FIELDS` ~line 53)
- Test: `rust/octocam-web/src/settings.rs` (tests module)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `settings.rs` (near `matter_requires_admin_password`, ~line 898):

```rust
    #[test]
    fn parses_hksv_enabled() {
        let mut map = Map::new();
        map.insert("hksv_enabled".into(), Value::String("true".into()));
        let s = validate_map(&map);
        assert!(s.hksv_enabled, "hksv_enabled should parse from the form map");

        // Absent key keeps the default (false).
        let s_default = validate_map(&Map::new());
        assert!(!s_default.hksv_enabled);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust/octocam-web && cargo test parses_hksv_enabled`
Expected: FAIL — compile error `no field hksv_enabled on type Settings`.

- [ ] **Step 3: Add the struct field**

In `settings.rs`, add to the `Settings` struct after `motion_zones: u64,` (~line 61):

```rust
    pub motion_zones: u64,
    pub hksv_enabled: bool,
```

- [ ] **Step 4: Add the default**

In the `impl Default for Settings` block, after `motion_zones: u64::MAX,` (~line 209):

```rust
            motion_zones: u64::MAX,
            hksv_enabled: false,
```

- [ ] **Step 5: Parse it in `validate_map`**

In `validate_map`, after the `settings.noir_mode = ...` line (~line 422), before `clamp_to_encoder_limits(&mut settings);`:

```rust
    settings.noir_mode = bool_value(&map, "noir_mode", settings.noir_mode);
    settings.hksv_enabled = bool_value(&map, "hksv_enabled", settings.hksv_enabled);
    clamp_to_encoder_limits(&mut settings);
```

- [ ] **Step 6: Run tests — expect the backup coverage test to fail**

Run: `cd rust/octocam-web && cargo test`
Expected: `parses_hksv_enabled` PASSES, but `backup::tests::field_lists_cover_all_settings` FAILS (the new field is in `Settings` but not classified in `PORTABLE_FIELDS`/`EXCLUDED_FIELDS`). This is the fail-closed allow-list working as designed.

- [ ] **Step 7: Add the field to `PORTABLE_FIELDS`**

In `backup.rs`, add to the `PORTABLE_FIELDS` array after `"motion_zones",` (~line 53):

```rust
    "motion_zones",
    "hksv_enabled",
```

- [ ] **Step 8: Run tests to verify all pass**

Run: `cd rust/octocam-web && cargo test`
Expected: PASS — `field_lists_cover_all_settings` green again, 85 tests pass.

- [ ] **Step 9: Commit**

```bash
git add rust/octocam-web/src/settings.rs rust/octocam-web/src/backup.rs
git commit -m "feat(hksv): add hksv_enabled setting and backup coverage"
```

---

## Task 2: Enforce `hksv_enabled ⇒ motion_enabled`

HKSV recording is motion-triggered; arming it with motion off is meaningless. Mirror the existing `enforce_matter_requires_admin` pattern (a second pass after `validate_map`, called from both the settings form and backup restore).

**Files:**
- Modify: `rust/octocam-web/src/settings.rs` (add fn after `enforce_matter_requires_admin`, ~line 434; add test)
- Modify: `rust/octocam-web/src/main.rs` (`update_settings`, ~line 1535)
- Modify: `rust/octocam-web/src/backup.rs` (`parse_restore`, ~line 194)

- [ ] **Step 1: Write the failing test**

Add to the `settings.rs` tests module:

```rust
    #[test]
    fn hksv_requires_motion() {
        // HKSV on + motion off  -> HKSV forced off.
        let mut s = Settings {
            hksv_enabled: true,
            motion_enabled: false,
            ..Settings::default()
        };
        enforce_hksv_requires_motion(&mut s);
        assert!(!s.hksv_enabled, "HKSV must not stay enabled without motion");

        // HKSV on + motion on -> stays on.
        let mut s2 = Settings {
            hksv_enabled: true,
            motion_enabled: true,
            ..Settings::default()
        };
        enforce_hksv_requires_motion(&mut s2);
        assert!(s2.hksv_enabled);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd rust/octocam-web && cargo test hksv_requires_motion`
Expected: FAIL — `cannot find function enforce_hksv_requires_motion`.

- [ ] **Step 3: Implement the enforcement function**

In `settings.rs`, add immediately after `enforce_matter_requires_admin` (~line 434):

```rust
/// HKSV recording is triggered by the motion sensor; without motion detection
/// there is nothing to start a recording. Force HKSV off when motion is off so
/// the bridge never advertises a recording capability it can't trigger.
pub fn enforce_hksv_requires_motion(settings: &mut Settings) {
    if !settings.motion_enabled {
        settings.hksv_enabled = false;
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd rust/octocam-web && cargo test hksv_requires_motion`
Expected: PASS.

- [ ] **Step 5: Wire into the settings form handler**

In `main.rs`, in `update_settings`, after `settings::enforce_matter_requires_admin(&mut validated);` (~line 1535):

```rust
    settings::enforce_matter_requires_admin(&mut validated);
    settings::enforce_hksv_requires_motion(&mut validated);
```

- [ ] **Step 6: Wire into backup restore**

In `backup.rs`, in `parse_restore`, after `settings::enforce_matter_requires_admin(&mut restored);` (~line 194):

```rust
    settings::enforce_matter_requires_admin(&mut restored);
    settings::enforce_hksv_requires_motion(&mut restored);
```

- [ ] **Step 7: Run the full suite to verify no regressions**

Run: `cd rust/octocam-web && cargo test`
Expected: PASS — all tests green (86 tests).

- [ ] **Step 8: Commit**

```bash
git add rust/octocam-web/src/settings.rs rust/octocam-web/src/main.rs rust/octocam-web/src/backup.rs
git commit -m "feat(hksv): enforce hksv_enabled requires motion_enabled"
```

---

## Task 3: Reserve a mediamtx reader slot for the concurrent recording pull

Today `maxReaders = rtsp_max_clients + reserve` where `reserve = homekit_enabled + matter_enabled` — one flat slot per daemon. When HKSV records while a live-view session is active, the bridge holds a second concurrent reader on the main path; without an extra reserved slot mediamtx refuses the recording connection. Add a slot only when HKSV can actually run concurrently (HomeKit enabled AND HKSV enabled).

**Files:**
- Modify: `rust/octocam-web/src/mediamtx.rs` (`reserve`, ~line 133; add test in tests module)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `mediamtx.rs` (near `homekit_reserve_adds_one_reader`, ~line 444):

```rust
    #[test]
    fn hksv_reserves_an_extra_reader_when_homekit_enabled() {
        let max_readers = |content: &str| -> Vec<i32> {
            content
                .lines()
                .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
                .map(|v| v.parse().unwrap())
                .collect()
        };
        let base = Settings {
            homekit_enabled: true,
            hksv_enabled: false,
            ..Default::default()
        };
        let with_hksv = Settings {
            hksv_enabled: true,
            ..base.clone()
        };
        let a = max_readers(&render_mediamtx_config(&base));
        let b = max_readers(&render_mediamtx_config(&with_hksv));
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(y - x, 1, "enabling HKSV must reserve one more reader per path");
        }
    }

    #[test]
    fn hksv_reserves_nothing_without_homekit() {
        let max_readers = |content: &str| -> Vec<i32> {
            content
                .lines()
                .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
                .map(|v| v.parse().unwrap())
                .collect()
        };
        let base = Settings {
            homekit_enabled: false,
            hksv_enabled: false,
            ..Default::default()
        };
        // hksv_enabled without homekit shouldn't reserve (the bridge isn't running).
        let hksv_no_homekit = Settings {
            hksv_enabled: true,
            ..base.clone()
        };
        let a = max_readers(&render_mediamtx_config(&base));
        let b = max_readers(&render_mediamtx_config(&hksv_no_homekit));
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(y - x, 0, "HKSV without HomeKit must not change the reader budget");
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd rust/octocam-web && cargo test hksv_reserves`
Expected: FAIL — `hksv_reserves_an_extra_reader_when_homekit_enabled` fails (`y - x == 0`, not 1), because `reserve` ignores HKSV.

- [ ] **Step 3: Update the reserve calculation**

In `mediamtx.rs`, replace the `reserve` line (~line 133):

```rust
    let reserve = i32::from(settings.homekit_enabled) + i32::from(settings.matter_enabled);
```

with:

```rust
    // Each enabled local daemon reserves one slot. HKSV recording opens a SECOND
    // concurrent reader (recording pull alongside a live-view session), so reserve
    // an extra slot when HomeKit + HKSV are both on.
    let reserve = i32::from(settings.homekit_enabled)
        + i32::from(settings.matter_enabled)
        + i32::from(settings.homekit_enabled && settings.hksv_enabled);
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd rust/octocam-web && cargo test hksv_reserves`
Expected: PASS. Also run `cargo test homekit_reserve_adds_one_reader matter_reserve_adds_one_reader` — both still PASS (defaults have `hksv_enabled: false`).

- [ ] **Step 5: Run the full suite**

Run: `cd rust/octocam-web && cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rust/octocam-web/src/mediamtx.rs
git commit -m "feat(hksv): reserve an extra mediamtx reader for concurrent recording"
```

---

## Task 4: Add systemd cgroup limits to the HomeKit bridge

Contain concurrent-encode resource use so a runaway/overlapping ffmpeg can't OOM-kill mediamtx or octocam-web (the shared video source) on the 512 MB Pi Zero 2 W.

**Files:**
- Modify: `systemd/octocam-homekit.service` (`[Service]` section)

- [ ] **Step 1: Add the resource-control directives**

In `systemd/octocam-homekit.service`, in the `[Service]` block, after `RestartSec=3` (line 16):

```ini
Restart=on-failure
RestartSec=3
# Contain concurrent ffmpeg encodes (live view + HKSV recording) so a resource
# spike is throttled/capped within this unit instead of OOM-killing mediamtx or
# octocam-web on the 512MB Pi Zero 2 W. octocam-web + mediamtx need the rest.
MemoryMax=220M
MemoryHigh=180M
CPUQuota=250%
```

- [ ] **Step 2: Verify the unit file parses**

Run: `systemd-analyze verify systemd/octocam-homekit.service 2>&1 | grep -v '__SERVICE_USER__\|__PROJECT_DIR__' || true`
Expected: No syntax errors reported (placeholder-substitution and unit-existence warnings for `__…__` tokens and cross-unit deps are expected in a non-deployed checkout and can be ignored).

If `systemd-analyze` is unavailable (macOS dev host), skip — the directives are validated on deploy in Task 8.

- [ ] **Step 3: Commit**

```bash
git add systemd/octocam-homekit.service
git commit -m "feat(hksv): cap HomeKit bridge memory/cpu via cgroup limits"
```

---

## Task 5: Add the HKSV enable toggle to the settings UI

**Files:**
- Modify: `rust/octocam-web/templates/stream_settings.html` (`_checkboxes` hidden input ~line 20; new panel after the Motion detection block ~line 94)

- [ ] **Step 1: Register the checkbox in the `_checkboxes` list**

In `stream_settings.html`, update the hidden input (line 20) to append `hksv_enabled`:

```html
            <input type="hidden" name="_checkboxes" value="camera_enabled,sub_stream_enabled,hflip,vflip,text_overlay_enabled,noir_mode,motion_enabled,hksv_enabled">
```

- [ ] **Step 2: Add the HKSV panel**

In `stream_settings.html`, after the motion-zones `</label>` block (after line 94, before the `<div class="subsection-heading"><h3>Overlay</h3></div>` at line 96), insert:

```html
              <div class="subsection-heading"><h3>HomeKit Secure Video</h3></div>
              <div class="toggle-row">
                <span>Record clips to HomeKit on motion</span>
                <label class="switch"><input type="checkbox" name="hksv_enabled" {% if settings.hksv_enabled %}checked{% endif %}><span></span></label>
              </div>
              <p class="field-hint">Requires motion detection (above), a Home Hub (HomePod/Apple TV), and an iCloud+ plan with HomeKit Secure Video storage. Recording quality is chosen by the Home app. If motion detection is off, this turns itself off on save.</p>
```

Note: if `field-hint` is not an existing class, use whatever hint/help class the template already uses; check the file for a sibling `<p>` hint pattern and match it. If none exists, a plain `<p>` is acceptable.

- [ ] **Step 3: Verify the template renders**

Run: `cd rust/octocam-web && cargo build`
Expected: PASS (Askama compiles templates at build time; a template syntax error fails the build).

- [ ] **Step 4: Manually verify the toggle round-trips (preview)**

Start the dev server, open the stream-settings page, toggle "Record clips to HomeKit on motion" on with motion enabled, save, and confirm it stays on after reload. Then turn motion off, save, and confirm HKSV auto-clears (Task 2 enforcement). Use the preview tooling per the repo's verification workflow.

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/templates/stream_settings.html
git commit -m "feat(hksv): add HKSV enable toggle to stream settings"
```

---

## Task 6: Add a fragmented-MP4 box splitter to the bridge

hap-nodejs needs the recording as discrete packets: first the init segment (`ftyp`+`moov`), then one packet per `moof`+`mdat` fragment. ffmpeg emits a continuous fMP4 byte stream on stdout; this helper parses MP4 boxes and groups them.

**Files:**
- Modify: `homekit/octocam-homekit.js` (add helpers near the other top-level functions, after `runProcess`, ~line 230)
- Test: `homekit/test-mp4-splitter.js` (standalone, temporary — removed after verification)

- [ ] **Step 1: Implement the box reader and fragment grouper**

In `octocam-homekit.js`, add after the `runProcess` function (~line 230):

```js
// Read a stream of MP4 boxes. Each box is a 4-byte big-endian size + 4-byte type
// + payload. Yields { type, data } where data is the complete box buffer.
async function* readMp4Boxes(stream) {
  const queue = [];
  let waiting = null;
  let ended = false;
  let streamError = null;

  stream.on("data", (chunk) => {
    queue.push(chunk);
    if (waiting) { const w = waiting; waiting = null; w(); }
  });
  stream.on("end", () => { ended = true; if (waiting) { const w = waiting; waiting = null; w(); } });
  stream.on("error", (err) => { streamError = err; if (waiting) { const w = waiting; waiting = null; w(); } });

  let pending = Buffer.alloc(0);
  const pull = () => new Promise((resolve) => { waiting = resolve; });

  const ensure = async (n) => {
    while (pending.length < n) {
      if (queue.length) {
        pending = Buffer.concat([pending, ...queue.splice(0)]);
      } else if (streamError) {
        throw streamError;
      } else if (ended) {
        return false;
      } else {
        await pull();
      }
    }
    return true;
  };

  while (true) {
    if (!(await ensure(8))) return;
    const size = pending.readUInt32BE(0);
    const type = pending.toString("ascii", 4, 8);
    if (size < 8) throw new Error(`invalid mp4 box size ${size} for type ${type}`);
    if (!(await ensure(size))) return;
    const data = pending.subarray(0, size);
    pending = pending.subarray(size);
    yield { type, data };
  }
}

// Group MP4 boxes into HKSV packets: the init segment (everything up to and
// including moov) first, then each moof+mdat pair as one fragment.
// Yields { kind: "init" | "fragment", data: Buffer }.
async function* readMp4Fragments(stream) {
  let initBoxes = [];
  let sentInit = false;
  let fragmentBoxes = [];

  for await (const box of readMp4Boxes(stream)) {
    if (!sentInit) {
      initBoxes.push(box.data);
      if (box.type === "moov") {
        yield { kind: "init", data: Buffer.concat(initBoxes) };
        sentInit = true;
        initBoxes = [];
      }
      continue;
    }
    fragmentBoxes.push(box.data);
    if (box.type === "mdat") {
      yield { kind: "fragment", data: Buffer.concat(fragmentBoxes) };
      fragmentBoxes = [];
    }
  }
}

module.exports = module.exports || {};
module.exports.readMp4Boxes = readMp4Boxes;
module.exports.readMp4Fragments = readMp4Fragments;
```

Note: the `module.exports` lines are only to allow the standalone test in Step 2 to import these helpers. If the file is run as the daemon entrypoint (`main()` at the bottom), exporting is harmless. Keep them.

- [ ] **Step 2: Write the standalone verification test**

Create `homekit/test-mp4-splitter.js`:

```js
"use strict";
const assert = require("assert");
const { Readable } = require("stream");
const { readMp4Fragments } = require("./octocam-homekit.js");

// Build a fake fMP4 byte stream: ftyp, moov, then two moof+mdat fragments.
function box(type, payloadLen) {
  const size = 8 + payloadLen;
  const b = Buffer.alloc(size);
  b.writeUInt32BE(size, 0);
  b.write(type, 4, "ascii");
  return b;
}

async function run() {
  const stream = Buffer.concat([
    box("ftyp", 8),
    box("moov", 40),
    box("moof", 16),
    box("mdat", 100),
    box("moof", 16),
    box("mdat", 120),
  ]);
  // Feed it in awkward chunk sizes to exercise the buffering.
  const readable = new Readable({ read() {} });
  for (let i = 0; i < stream.length; i += 7) {
    readable.push(stream.subarray(i, i + 7));
  }
  readable.push(null);

  const out = [];
  for await (const seg of readMp4Fragments(readable)) {
    out.push({ kind: seg.kind, len: seg.data.length });
  }

  assert.strictEqual(out.length, 3, "expected init + 2 fragments");
  assert.strictEqual(out[0].kind, "init");
  assert.strictEqual(out[0].len, 16 + 48, "init = ftyp(16) + moov(48)");
  assert.strictEqual(out[1].kind, "fragment");
  assert.strictEqual(out[1].len, 24 + 108, "frag1 = moof(24) + mdat(108)");
  assert.strictEqual(out[2].kind, "fragment");
  assert.strictEqual(out[2].len, 24 + 128, "frag2 = moof(24) + mdat(128)");
  console.log("mp4 splitter OK");
}

run().catch((err) => { console.error(err); process.exit(1); });
```

- [ ] **Step 3: Run the standalone test to verify it passes**

Run: `cd homekit && node test-mp4-splitter.js`
Expected: `mp4 splitter OK` and exit 0.

(If Node < 18 is the dev default, this still works — async generators and `subarray` are supported from Node 12+.)

- [ ] **Step 4: Remove the standalone test file**

The bridge has no persistent test harness; this was a one-shot verification.

```bash
rm homekit/test-mp4-splitter.js
```

- [ ] **Step 5: Commit**

```bash
git add homekit/octocam-homekit.js
git commit -m "feat(hksv): add fragmented-mp4 box splitter for recording"
```

---

## Task 7: Wire the CameraRecordingDelegate into the bridge

Add the recording delegate (spawns ffmpeg at the hub-negotiated params, streams fragments), the `recording` options (with the mandatory placeholder audio config and `prebufferLength: 0`), and `sensors: { motion: motionService }` so the existing MotionSensor triggers recording.

**Files:**
- Modify: `homekit/octocam-homekit.js` (imports ~line 10; new delegate class before `main()` ~line 505; `CameraController` construction ~line 583)

- [ ] **Step 1: Import the recording enums from hap-nodejs**

In `octocam-homekit.js`, extend the destructured `require("hap-nodejs")` (lines 10-18):

```js
const {
  Accessory,
  AudioRecordingCodecType,
  AudioRecordingSamplerate,
  CameraController,
  Categories,
  Characteristic,
  H264Level,
  H264Profile,
  HAPStorage,
  MediaContainerType,
  Service,
  VideoCodecType,
  uuid,
} = require("hap-nodejs");
```

- [ ] **Step 2: Add the recording delegate class**

In `octocam-homekit.js`, add before `function main()` (~line 569), after `startMotionListener`:

```js
// H264Profile/H264Level enum -> ffmpeg strings. Indices match hap-nodejs enums
// (BASELINE=0/MAIN=1/HIGH=2; LEVEL3_1=0/LEVEL3_2=1/LEVEL4_0=2).
const RECORDING_PROFILES = ["baseline", "main", "high"];
const RECORDING_LEVELS = ["3.1", "3.2", "4.0"];

class OctoCamRecordingDelegate {
  constructor() {
    this.selectedConfig = undefined;
    this.processes = new Map(); // streamId -> ChildProcess
  }

  updateRecordingActive(active) {
    console.log(`HKSV recording active: ${active}`);
  }

  updateRecordingConfiguration(configuration) {
    this.selectedConfig = configuration;
    if (configuration) {
      const v = configuration.videoCodec;
      console.log(
        `HKSV config selected: ${v.resolution[0]}x${v.resolution[1]}@${v.resolution[2]}fps ` +
        `profile=${v.parameters.profile} level=${v.parameters.level} ` +
        `bitrate=${v.parameters.bitRate}k iFrame=${v.parameters.iFrameInterval}ms ` +
        `frag=${configuration.mediaContainerConfiguration.fragmentLength}ms`
      );
    } else {
      console.log("HKSV config cleared");
    }
  }

  buildRecordingArgs(settings, config) {
    const v = config.videoCodec;
    const [width, height, fps] = v.resolution;
    const profile = RECORDING_PROFILES[v.parameters.profile] || "high";
    const level = RECORDING_LEVELS[v.parameters.level] || "4.0";
    const bitrate = Math.max(64, v.parameters.bitRate); // kbps
    const fragSec = Math.max(1, config.mediaContainerConfiguration.fragmentLength / 1000);
    // GOP aligned to fragment length so every fragment starts with a keyframe.
    return [
      "-hide_banner", "-nostdin",
      "-rtsp_transport", "tcp",
      "-fflags", "nobuffer", "-flags", "low_delay",
      "-i", rtspUrl(settings, "main"),
      "-an", "-sn", "-dn",
      "-map", "0:v:0",
      "-vf", `scale=${width}:${height}`,
      "-c:v", "libx264",
      "-preset", "ultrafast",
      "-tune", "zerolatency",
      "-pix_fmt", "yuv420p",
      "-profile:v", profile,
      "-level:v", level,
      "-r", String(fps),
      "-b:v", `${bitrate}k`,
      "-maxrate", `${bitrate}k`,
      "-bufsize", `${bitrate * 2}k`,
      "-force_key_frames", `expr:gte(t,n_forced*${fragSec})`,
      "-f", "mp4",
      "-movflags", "frag_keyframe+empty_moov+default_base_moof",
      "-",
    ];
  }

  killRecording(streamId) {
    const child = this.processes.get(streamId);
    if (child) {
      this.processes.delete(streamId);
      try { child.kill("SIGKILL"); } catch (_) {}
    }
  }

  async *handleRecordingStreamRequest(streamId) {
    const settings = loadSettings();
    const config = this.selectedConfig;
    if (!config) {
      console.error(`HKSV stream ${streamId} requested with no selected configuration`);
      return;
    }

    const args = this.buildRecordingArgs(settings, config);
    if (DEBUG_FFMPEG) console.log(`HKSV ffmpeg ${args.join(" ")}`);

    const child = spawn("ffmpeg", args, { env: process.env, stdio: ["ignore", "pipe", "pipe"] });
    this.processes.set(streamId, child);

    const stderrLines = [];
    child.stderr.on("data", (chunk) => {
      const text = chunk.toString();
      stderrLines.push(text);
      if (stderrLines.length > 40) stderrLines.shift();
    });
    child.on("error", (err) => {
      console.error(`HKSV ffmpeg spawn error (stream ${streamId}): ${err.message}`);
    });
    child.on("close", (code, signal) => {
      if (code && code !== 0) {
        console.error(`HKSV ffmpeg (stream ${streamId}) exited code=${code} signal=${signal}: ${stderrLines.join(" | ")}`);
      }
    });

    try {
      let prevFragment = null;
      let sentInit = false;
      for await (const seg of readMp4Fragments(child.stdout)) {
        if (seg.kind === "init") {
          yield { data: seg.data, isLast: false };
          sentInit = true;
          continue;
        }
        // One-fragment lookahead so the final fragment can be marked isLast.
        if (prevFragment !== null) {
          yield { data: prevFragment, isLast: false };
        }
        prevFragment = seg.data;
      }
      if (prevFragment !== null) {
        yield { data: prevFragment, isLast: true };
      } else if (!sentInit) {
        console.error(`HKSV stream ${streamId} produced no init segment`);
      }
    } finally {
      this.killRecording(streamId);
    }
  }

  closeRecordingStream(streamId, reason) {
    console.log(`HKSV stream ${streamId} closed (reason=${reason})`);
    this.killRecording(streamId);
  }
}
```

- [ ] **Step 3: Build the recording options and wire the controller**

In `octocam-homekit.js`, in `main()`, replace the `CameraController` construction (lines 582-597) with:

```js
  const delegate = new OctoCamStreamingDelegate();
  const recordingDelegate = new OctoCamRecordingDelegate();
  const cameraController = new CameraController({
    cameraStreamCount: 2,
    delegate,
    streamingOptions: {
      supportedCryptoSuites: [SRTP_AES_CM_128_HMAC_SHA1_80],
      video: {
        codec: {
          profiles: [0, 1, 2],
          levels: [0, 1, 2],
        },
        resolutions: supportedResolutions(settings),
      },
    },
    // HomeKit Secure Video. sensors.motion MUST be the existing Service instance
    // (passing `true` would create a duplicate internal MotionSensor).
    sensors: {
      motion: motionService,
    },
    recording: {
      options: {
        prebufferLength: 0, // no always-on encode; verified against the hub in deploy
        mediaContainerConfiguration: {
          type: MediaContainerType.FRAGMENTED_MP4,
          fragmentLength: 4000,
        },
        video: {
          type: VideoCodecType.H264,
          parameters: {
            profiles: [H264Profile.BASELINE, H264Profile.MAIN, H264Profile.HIGH],
            levels: [H264Level.LEVEL3_1, H264Level.LEVEL3_2, H264Level.LEVEL4_0],
          },
          resolutions: [
            [1280, 720, 30],
            [1280, 720, 24],
            [1280, 720, 15],
            [1920, 1080, 30],
            [1920, 1080, 24],
            [1920, 1080, 15],
            [640, 480, 30],
            [640, 360, 30],
          ],
        },
        // No microphone hardware, but hap-nodejs REQUIRES a non-empty audio codec
        // list or it throws at construction. Advertise AAC-LC; we never emit audio.
        audio: {
          codecs: [
            {
              type: AudioRecordingCodecType.AAC_LC,
              samplerate: [AudioRecordingSamplerate.KHZ_32],
              audioChannels: 1,
            },
          ],
        },
      },
      delegate: recordingDelegate,
    },
  });
  delegate.controller = cameraController;
```

Note: `motionService` is already created at line 580 (`accessory.getService(Service.MotionSensor) || accessory.addService(...)`), and `startMotionListener` (line 623) already updates its `MotionDetected` characteristic — no change needed there; it now doubles as the HKSV trigger.

- [ ] **Step 4: Verify the bridge loads without throwing**

The bridge needs the real `hap-nodejs` from `homekit/node_modules`. Verify construction doesn't throw (this catches the audio-required and enum-import errors) using a syntax+load check that stops before `main()` opens network ports:

Run: `cd homekit && node --check octocam-homekit.js && echo "syntax OK"`
Expected: `syntax OK` (no parse errors — catches typos in the added code).

Full construction is validated on-device in Task 8 (running `main()` here would try to bind the HAP port and connect to the motion SSE endpoint).

- [ ] **Step 5: Commit**

```bash
git add homekit/octocam-homekit.js
git commit -m "feat(hksv): add CameraRecordingDelegate and recording options"
```

---

## Task 8: Deploy and verify on the Pi + Home Hub

The Home app cannot be automated. This task is the required manual gate. The **prebuffer check comes first** — if the hub rejects `prebufferLength: 0`, stop and revisit the design (rolling-buffer prebuffer) before relying on the rest.

**Files:** none (deploy + verification).

- [ ] **Step 1: Deploy to the Pi**

Cross-build on the Mac and deploy per the repo workflow (do NOT build on the Pi):

```bash
./scripts/deploy-pi-web.sh
```

Confirm the deploy syncs the updated `homekit/` (excluding `node_modules`, so the existing `hap-nodejs@0.14.3` is reused) and installs the updated `octocam-homekit.service`.

- [ ] **Step 2: Confirm services are healthy**

```bash
ssh root@192.168.2.211 'systemctl is-active octocam-web.service octocam-homekit.service && systemctl show octocam-homekit.service -p MemoryMax -p CPUQuotaPerSecUSec'
```
Expected: both `active`; `MemoryMax=230686720` (220M) and a non-empty `CPUQuotaPerSecUSec` — confirms the cgroup limits applied.

- [ ] **Step 3: Enable HKSV in OctoCam and the Home app**

- In the OctoCam web UI: enable motion detection, then enable "Record clips to HomeKit on motion", save.
- In the Home app: open the camera → Settings → enable "Stream & Allow Recording" / "Record" for both Home and Away (this requires the HomePod-mini Home Hub and an iCloud+ HKSV plan).

- [ ] **Step 4: Prebuffer gate — confirm the hub accepts the recording config**

Watch the bridge log while enabling recording:

```bash
ssh root@192.168.2.211 'journalctl -u octocam-homekit.service -f'
```
Expected: an `HKSV config selected: …` line appears (proves `updateRecordingConfiguration` fired and the hub negotiated a configuration with `prebufferLength: 0` accepted). If NO config is ever selected, or the Home app reports the camera can't record, **STOP** — the 0 s prebuffer is likely rejected; revisit the prebuffer decision before continuing.

- [ ] **Step 5: Trigger motion and confirm a clip records**

Walk in front of the camera (or otherwise trigger motion). In the bridge log, expect a recording request and ffmpeg activity (no `HKSV ffmpeg … exited code=` errors). In the Home app camera history, confirm a clip appears, plays back, and begins at the trigger frame (no lead-in).

- [ ] **Step 6: Confirm concurrent live view + recording**

Open the live view in the Home app, then trigger motion so a recording runs concurrently. Confirm:
- Live view stays up (no mediamtx connection refusal — validates the Task 3 reader reserve).
- A clip still records.
- `journalctl -u octocam-homekit.service` shows no OOM / restart, and `systemctl status` shows the unit stayed within its memory cap.

- [ ] **Step 7: Confirm no orphaned ffmpeg processes**

After several motion events end:

```bash
ssh root@192.168.2.211 'pgrep -af "ffmpeg.*movflags frag_keyframe" || echo "no orphaned recording ffmpeg"'
```
Expected: `no orphaned recording ffmpeg` when idle (validates `closeRecordingStream`/`finally` cleanup).

- [ ] **Step 8: Final commit (if any deploy-script or tuning tweaks were needed)**

If Steps 2-7 required adjusting cgroup values or ffmpeg args, commit those:

```bash
git add -A
git commit -m "fix(hksv): tune recording after on-device verification"
```

Otherwise, no commit — the feature is verified.

---

## Self-Review

**Spec coverage:**
- Extend Node bridge (not native Rust) → Tasks 6, 7. ✓
- No local clip storage → design; nothing writes to disk (Task 7 streams through the generator). ✓
- Prebuffer 0s, hardware-gated → Task 7 (`prebufferLength: 0`) + Task 8 Step 4 gate. ✓
- Hub-negotiated quality (not admin-set) → Task 7 (`updateRecordingConfiguration` drives `buildRecordingArgs`); no admin quality fields added (Task 1 adds only `hksv_enabled`). ✓
- Fixed Pi-safe advertised config list → Task 7 Step 3 `resolutions`/`parameters`. ✓
- cgroup limits for concurrency → Task 4. ✓
- Placeholder audio config → Task 7 Step 3 `audio.codecs`. ✓
- Motion-only trigger → Task 7 `sensors.motion` (auto-derives `EventTriggerOption.MOTION`); no doorbell. ✓
- `hksv_enabled` requires motion → Task 2. ✓
- backup `PORTABLE_FIELDS` → Task 1 Steps 6-7. ✓
- mediamtx reader budget → Task 3. ✓
- `sensors: { motion: motionService }` instance reuse → Task 7 Step 3. ✓
- Settings UI toggle → Task 5. ✓
- try/finally ffmpeg reaping → Task 7 Step 2 (`finally { this.killRecording }`). ✓
- Error handling: no retry loop → Task 7 (generator returns on error, no re-spawn). ✓
- Testing: cargo test green + manual hub verification → Tasks 1-3 (tests), Task 8 (manual). ✓

**Placeholder scan:** No TBD/TODO. Every code step shows complete code. The one conditional ("if `field-hint` isn't an existing class") gives an explicit fallback.

**Type/name consistency:** `hksv_enabled` used identically across settings.rs, backup.rs, mediamtx.rs, template, and the `_checkboxes` list. `enforce_hksv_requires_motion` defined in Task 2 Step 3, called in Steps 5-6. `readMp4Fragments`/`readMp4Boxes` defined in Task 6, consumed in Task 7. `OctoCamRecordingDelegate`, `selectedConfig`, `buildRecordingArgs`, `killRecording`, `processes` all internally consistent. `RECORDING_PROFILES`/`RECORDING_LEVELS` indices match the hap-nodejs `H264Profile`/`H264Level` enums verified in the reference section.
