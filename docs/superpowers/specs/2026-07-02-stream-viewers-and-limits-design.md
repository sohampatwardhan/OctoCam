# Stream Viewers, Limits, and Main-Stream Fix — Design

**Date:** 2026-07-02
**Status:** Approved (brainstorming complete)
**Scope:** octocam-web (Rust), mediamtx config generation, octocam-homekit (Node), static/app.js, stream templates

## Problem

1. **Main stream is broken on the shipped device.** `main` is configured at 1640×1232, which exceeds the Pi Zero 2 W hardware H.264 encoder ceiling (1920×1080 — 1232 lines > 1080). mediamtx logs `encoder_hardware_h264_encode(): ioctl(VIDIOC_QBUF) failed` per frame and readers of `main` get `400 Bad Request`. Only `sub` works. (Verified live on 192.168.2.211: `ffprobe rtsp://127.0.0.1:8554/main` → 400; journalctl shows QBUF failures; `1640x1232` is a selectable preset.)
2. **Nobody can see who is streaming.** All viewers (external RTSP, dashboard browser via WebRTC/HLS iframe, HomeKit via local ffmpeg→RTSP) funnel through mediamtx, but no counts are surfaced anywhere.
3. **Limits exist but are blunt and HomeKit-blind.** `maxReaders` (from `rtsp_max_clients`=1 main / `sub_rtsp_max_clients`=2 sub) rejects excess readers, but HomeKit's local reader silently competes for the same slots, and a rejected browser viewer just sees a dead player instead of being steered to the sub stream.
4. **Snapshot paths are unthrottled.** The Home app tile polls a snapshot every ~15s while open; each poll spawns ffmpeg + a full RTSP session on `sub` for ~4s (verified in mediamtx logs). `/snapshot.jpg` spawns `rpicam-still` per request.

## Decisions (made during brainstorming)

| Decision | Choice |
|---|---|
| At-capacity policy | **Reject the newcomer** (keep mediamtx `maxReaders` behavior); never interrupt existing viewers |
| HomeKit slot | **Soft reserved slot**: `maxReaders = configured cap + 1` on both paths when HomeKit is enabled. Soft = an external client can occupy the spare slot while HomeKit is idle; no kicking |
| Browser at-capacity UX | **Reroute to sub**: server picks the initial stream by live availability; client falls back main→sub with a visible note |
| External RTSP at capacity | Rejected by mediamtx (protocol has no usable redirect); both URLs shown in UI |
| Viewer-count visibility | **Stream page + `/api/status`** (existing 5s poll); no topbar badge |
| Rate-limit scope | **Concurrent-viewer caps + snapshot throttling** (both `/snapshot.jpg` and HomeKit snapshots); no per-IP connection-attempt limiting |
| HomeKit local vs remote | **Requested-quality heuristic** (see §5) — network address alone can't distinguish (hub relays); height ≥720 or maxBitrate ≥500kbps → main, else sub |
| BLE Wi-Fi provisioning | **Dropped.** WAC needs MFi (unavailable to hap-nodejs); iOS Safari lacks Web Bluetooth; AP flow already serves all phones. Replaced with captive-portal polish (§6). Revisit only with Matter |

## Architecture

mediamtx remains the single enforcement point (per-path `maxReaders`) and becomes the single **counting** point via its local HTTP API. octocam-web reads counts (never video). The HomeKit daemon chooses main/sub per session. No video packets ever transit octocam-web.

```
external RTSP ──┐
browser WebRTC ─┼──► mediamtx :8554/:8888/:8889  ── rpiCamera ──► camera
HomeKit ffmpeg ─┘        │ enforces maxReaders
                         │ API 127.0.0.1:9997 (new)
octocam-web ◄────────────┘ GET /v3/paths/list + /v3/rtspsessions/list
  └─ counts → /api/status → stream page (5s poll)
```

## Components

### 1. Settings fix (`settings.rs`)

- Remove the `1640x1232` preset from `RESOLUTION_PRESETS`; add `1536x864` (16:9). Top 4:3 preset becomes `1296x972`.
- `validate_form` clamps main and sub resolutions to `width ≤ 1920 && height ≤ 1080` (hardware H.264 encoder limit) regardless of submitted values.
- Migration on load (same pattern as existing stream-path migration): if stored resolution exceeds the limit, clamp to `1296x972` and persist. Fixes the currently-deployed device on next start.

### 2. mediamtx config (`mediamtx.rs`)

- Emit `api: yes` and `apiAddress: 127.0.0.1:9997` (localhost-only; not reachable from LAN).
- `maxReaders` per path = configured cap `+ 1` when `settings.homekit_enabled` (HomeKit may read either path: main for local viewers, sub for remote viewers and snapshots).
- Existing config test extended: asserts `api:` lines and the `+1` math.

### 3. Viewer counting (`streams.rs`, new module in octocam-web)

- `pub async fn viewer_report() -> Option<ViewerReport>`: two HTTP GETs to `127.0.0.1:9997` (`/v3/paths/list`, `/v3/rtspsessions/list`) over `tokio::net::TcpStream` with a minimal HTTP/1.1 client (~60 lines; no new dependencies; `serde_json` for parsing; 2s overall `tokio::time::timeout`).
- Classification per path (`main`, `sub`): `webrtcSession`/`hlsMuxer` reader → **browser**; `rtspSession` whose id joins to a session with `remoteAddr` starting `127.0.0.1` → **homekit** (local); other `rtspSession` → **rtsp**. Join by reader `id`.
- `ViewerReport { main: PathViewers, sub: PathViewers }`; `PathViewers { browser, rtsp, homekit, total, capacity }` where `capacity` is the *user-facing* cap (excluding the HomeKit reserve). `main_available = total_non_local < capacity`.
- Failure mode: any error → `None`; callers render "viewer counts unavailable" and reroute logic falls back to static defaults. Errors logged at most once per minute (rate-limited log guard), not per poll.
- HLS caveat (accepted): mediamtx reports one `hlsMuxer` per path regardless of HLS client count; HLS viewers therefore count as ≥1. The dashboard uses WebRTC (exact). Documented in UI copy as approximate only if HLS is in use.

### 4. Reroute + UI (main.rs, `stream.html`, `app.js`)

- `/api/status` response gains a `viewers` field (`ViewerReport` or `null`). Handler runs `run_blocking(system::status)` and `streams::viewer_report()` concurrently (`tokio::join!`).
- `/stream` page render: initial iframe source = `main` if `viewers.main_available`, else `sub` (with a visible "Main stream is at capacity — showing sub stream" note). If report is `None`: current behavior (sub if enabled else main).
- `app.js`: on Main button click when the latest 5s payload says main is full, switch to sub + show note instead of loading a dead player; also fall back to sub if the main iframe errors. Stream page's RTSP card shows per-path breakdown, e.g. `Main: 1/1 viewers (1 HomeKit) · Sub: 2/2 (1 browser, 1 RTSP)`, refreshed by the existing 5s poll.
- Graceful close: no new machinery — mediamtx frees slots on session teardown (verified in logs); Stop button already unloads the iframe; counts self-correct within one 5s poll.

### 5. HomeKit per-session stream choice (`octocam-homekit.js`)

- Replace static `sourceStream(settings)` (currently: always sub when sub enabled) with per-session logic in `handleStreamRequest`:
  - requested video height ≥ 720 **or** `maxBitrate` ≥ 500 kbps → **main**
  - else → **sub**
  - if `targetAddress` is outside the camera's own /24 → **sub** regardless (belt and suspenders; hub-relayed remote sessions usually present LAN addresses, which is why quality is the primary signal).
  - if the chosen main ffmpeg fails to produce output within its existing start timeout → retry once with sub before failing the session.
- Snapshots always use sub.

### 6. Snapshot throttling

- **octocam-web `/snapshot.jpg`**: `AppState` gains `snapshot_cache: tokio::sync::Mutex<Option<(Instant, Vec<u8>)>>`. Serve cached JPEG if < 2s old. Cold path holds the mutex across capture (single-flight: concurrent cold requests coalesce; capture already bounded by `CAPTURE_TIMEOUT`).
- **HomeKit `handleSnapshotRequest`**: cache last JPEG buffer + timestamp; TTL 5s. Home-app tile polling then costs ≤1 ffmpeg/RTSP session per 5s per household instead of one per request.

### 7. Captive-portal polish (setup AP mode)

- When `setup_complete == false`, octocam-web additionally binds `0.0.0.0:80` and serves: iOS/macOS probe (`/hotspot-detect.html`), Android probe (`/generate_204`), and a catch-all — all `302` to `http://<device-ip>:8080/setup`. Phones joining `OctoCam-Setup` then auto-open the setup page. Listener is not started once setup is complete (port 80 stays closed in normal operation). Bind failure (e.g. port in use) logs a warning and continues — setup still works by manual URL.

## Error handling summary

- mediamtx API down/unparseable → counts `null`, UI note, static reroute defaults; never blocks a page render (2s timeout, async).
- All new subprocess/network I/O is bounded (existing `proc::run` timeouts; 2s API timeout; ffmpeg timeouts already present in the Node daemon).
- Snapshot cache lock is held briefly (memcpy) on hot path; capture on cold path is bounded; a failed capture doesn't poison the cache (old frame kept, error returned).

## Testing

**Rust unit tests:** mediamtx JSON fixture → classifier counts (browser/rtsp/homekit, main/sub, join-by-id); reroute decision table (available/full/None); resolution clamp + migration (1640×1232 → 1296×972; ≤limit untouched); mediamtx config contains `api:` + correct `maxReaders` with/without HomeKit; snapshot cache TTL (fresh hit, stale miss).
**Node:** decision-table cases for the stream chooser (720p/500kbps thresholds, subnet override, fallback) — as a small pure function extracted for testability; manual matrix documented if no Node test runner is added.
**On-hardware (192.168.2.211):** main plays via ffprobe after deploy; counts match a staged mix (1 browser + 1 RTSP + Home app); second browser viewer lands on sub while main occupied; closing the Home app frees slots within ~5s; captive portal sheet appears on iPhone joining the setup AP (requires a factory-ish reset or temporary `setup_complete=false`).

## Out of scope

- Kicking sessions / priority eviction (rejected in favor of reject-newcomer)
- Per-IP connection-attempt rate limiting
- Exact HLS client counting (mediamtx reports one muxer per path)
- BLE Wi-Fi provisioning and Matter commissioning (revisit with Matter camera support)
- Hard HomeKit slot reservation (requires kicking; soft reservation accepted)
