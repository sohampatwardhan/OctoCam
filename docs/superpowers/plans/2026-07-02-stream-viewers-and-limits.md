# Stream Viewers, Limits, and Main-Stream Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the broken main stream (hardware-encoder limit), count live viewers across RTSP/browser/HomeKit via the mediamtx API, reroute excess browser viewers to the sub stream, choose HomeKit main/sub per session, and throttle both snapshot paths.

**Architecture:** mediamtx stays the sole video/enforcement plane; octocam-web gains a read-only localhost API client (`streams.rs`) for counting and rerouting decisions; the HomeKit daemon picks its RTSP input per session by requested quality. Settings gain an encoder-limit clamp with startup config reconciliation so the fix reaches already-deployed devices.

**Tech Stack:** Rust (axum 0.8, tokio, serde_json — no new crates), mediamtx v3 API, Node (hap-nodejs), vanilla JS.

**Spec:** `docs/superpowers/specs/2026-07-02-stream-viewers-and-limits-design.md`

---

## File Structure

- Modify `rust/octocam-web/src/settings.rs` — presets, encoder clamp, tests
- Modify `rust/octocam-web/src/mediamtx.rs` — API lines, `maxReaders` reserve, change detection, tests
- Create `rust/octocam-web/src/streams.rs` — mediamtx API client + classifier + tests
- Modify `rust/octocam-web/src/main.rs` — module decl, startup reconcile + captive portal listener, `/api/status` viewers, stream reroute, snapshot cache
- Modify `rust/octocam-web/src/camera.rs` — snapshot cache freshness helper + test
- Modify `rust/octocam-web/templates/stream.html` — initial source, busy note, viewer rows
- Modify `static/app.js` — client-side fallback + live viewer rendering
- Modify `homekit/octocam-homekit.js` — per-session stream chooser, snapshot cache, sub-fallback

All Rust commands run from `rust/octocam-web/`. Full suite currently: 19 tests passing.

---

## Task 1: Encoder-limit clamp in settings

**Files:** Modify `rust/octocam-web/src/settings.rs`

- [ ] **Step 1: Write failing tests** (append inside `mod tests`):

```rust
    #[test]
    fn clamps_resolution_to_encoder_limit() {
        let mut map = Map::new();
        map.insert("resolution_width".into(), Value::from(1640));
        map.insert("resolution_height".into(), Value::from(1232));
        let settings = validate_map(&map);
        assert_eq!(settings.resolution_width, 1296);
        assert_eq!(settings.resolution_height, 972);
    }

    #[test]
    fn keeps_legal_resolution_unchanged() {
        let mut map = Map::new();
        map.insert("resolution_width".into(), Value::from(1536));
        map.insert("resolution_height".into(), Value::from(864));
        let settings = validate_map(&map);
        assert_eq!(settings.resolution_width, 1536);
        assert_eq!(settings.resolution_height, 864);
    }

    #[test]
    fn presets_exclude_oversize_modes() {
        assert!(RESOLUTION_PRESETS
            .iter()
            .all(|p| p.width <= MAX_ENCODER_WIDTH && p.height <= MAX_ENCODER_HEIGHT));
        assert!(RESOLUTION_PRESETS.iter().any(|p| p.value == "1536x864"));
        assert!(!RESOLUTION_PRESETS.iter().any(|p| p.value == "1640x1232"));
    }
```

- [ ] **Step 2:** Run `cargo test settings::` — expect the 3 new tests FAIL (missing `MAX_ENCODER_WIDTH`, preset assertions).

- [ ] **Step 3: Implement.** In `settings.rs`:

(a) Add constants near the top (below the `use` lines):

```rust
/// Pi hardware H.264 encoder limits. 1640x1232 is a valid IMX219 sensor mode but
/// exceeds 1080 encode lines; mediamtx then fails every frame with
/// `encoder_hardware_h264_encode(): ioctl(VIDIOC_QBUF) failed` and readers get 400.
pub const MAX_ENCODER_WIDTH: i32 = 1920;
pub const MAX_ENCODER_HEIGHT: i32 = 1080;
```

(b) In `RESOLUTION_PRESETS`, delete the `1640x1232` entry and add after `1296x972`:

```rust
    ResolutionPreset {
        value: "1536x864",
        label: "1536 x 864 (16:9)",
        width: 1536,
        height: 864,
    },
```

(c) At the END of `validate_map` (just before it returns `settings`), add:

```rust
    clamp_to_encoder_limits(&mut settings);
```

and define below `validate_map`:

```rust
/// Snap any resolution the hardware encoder cannot handle to the largest safe 4:3 mode.
fn clamp_to_encoder_limits(settings: &mut Settings) {
    if settings.resolution_width > MAX_ENCODER_WIDTH
        || settings.resolution_height > MAX_ENCODER_HEIGHT
    {
        settings.resolution_width = 1296;
        settings.resolution_height = 972;
    }
    if settings.sub_resolution_width > MAX_ENCODER_WIDTH
        || settings.sub_resolution_height > MAX_ENCODER_HEIGHT
    {
        settings.sub_resolution_width = 640;
        settings.sub_resolution_height = 480;
    }
}
```

NOTE: first confirm `load_settings` routes stored JSON through `validate_map` (read `settings.rs:211-228`); it does for the existing migrations — if it did not, the clamp must also be called there. `validate_map` builds `settings` as a `let settings = ...` chain; make the binding `let mut settings`.

- [ ] **Step 4:** `cargo test settings::` — all pass (including the existing `applies_resolution_preset_and_bounds`; if it asserted `1640x1232`, update it to a legal preset).
- [ ] **Step 5:** Commit: `git add -A rust/octocam-web/src/settings.rs && git commit -m "fix(web): clamp stream resolutions to hardware H.264 encoder limits"`

---

## Task 2: mediamtx API + reader reserve + change detection

**Files:** Modify `rust/octocam-web/src/mediamtx.rs`

- [ ] **Step 1: Write failing tests** (append inside `mod tests`):

```rust
    #[test]
    fn config_enables_localhost_api() {
        let settings = Settings::default();
        let content = render_mediamtx_config(&settings);
        assert!(content.contains("api: yes"));
        assert!(content.contains("apiAddress: 127.0.0.1:9997"));
    }

    #[test]
    fn homekit_reserve_adds_one_reader() {
        let mut settings = Settings::default();
        settings.homekit_enabled = false;
        let without = render_mediamtx_config(&settings);
        settings.homekit_enabled = true;
        let with = render_mediamtx_config(&settings);
        let max_readers = |content: &str| -> Vec<i32> {
            content
                .lines()
                .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
                .map(|v| v.parse().unwrap())
                .collect()
        };
        let a = max_readers(&without);
        let b = max_readers(&with);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(y - x, 1, "homekit reserve must add exactly one reader per path");
        }
    }
```

- [ ] **Step 2:** `cargo test mediamtx::` — FAIL (`render_mediamtx_config` does not exist).

- [ ] **Step 3: Implement.** Refactor `write_mediamtx_config` so rendering is separate and change-aware:

```rust
pub fn render_mediamtx_config(settings: &Settings) -> String {
    let reserve = if settings.homekit_enabled { 1 } else { 0 };
    let mut path_sections = vec![mediamtx_camera_path(
        &settings.rtsp_path,
        false,
        settings.resolution_width,
        settings.resolution_height,
        settings.framerate,
        settings.bitrate_kbps,
        settings.rtsp_max_clients + reserve,
    )];

    if settings.sub_stream_enabled {
        path_sections.push(mediamtx_camera_path(
            &settings.sub_rtsp_path,
            true,
            settings.sub_resolution_width,
            settings.sub_resolution_height,
            settings.sub_framerate,
            settings.sub_bitrate_kbps,
            settings.sub_rtsp_max_clients + reserve,
        ));
    }

    let mut content = vec![
        "logLevel: info".to_string(),
        String::new(),
        "api: yes".to_string(),
        "apiAddress: 127.0.0.1:9997".to_string(),
        String::new(),
        // ... keep every existing line (rtsp/hls/webrtc blocks) unchanged ...
        "paths:".to_string(),
    ];
    content.extend(path_sections);
    content.push(String::new());
    content.join("\n")
}

/// Writes the config; returns Ok(true) if the file content changed.
pub fn write_mediamtx_config(settings: &Settings, path: &PathBuf) -> Result<bool, String> {
    let next = render_mediamtx_config(settings);
    let current = fs::read_to_string(path).unwrap_or_default();
    if current == next {
        return Ok(false);
    }
    fs::write(path, next).map_err(|error| error.to_string())?;
    Ok(true)
}
```

Keep the full existing middle block (rtsp/hls/webrtc/srt lines) verbatim inside `content` — only `api:`/`apiAddress:` are new. Update `configure_rtsp_service` for the new return type: `Ok(changed)` sets `changed` in its `ActionResult` accordingly (`Ok(true) → changed: true, "ok"`, `Ok(false) → changed: false, "unchanged"`).

- [ ] **Step 4:** `cargo test mediamtx::` (and `cargo build` for the `configure_rtsp_service` fallout) — all pass.
- [ ] **Step 5:** Commit: `git commit -am "feat(web): mediamtx localhost API, HomeKit reader reserve, config change detection"`

---

## Task 3: `streams.rs` — mediamtx API client and classifier

**Files:** Create `rust/octocam-web/src/streams.rs`; modify `rust/octocam-web/src/main.rs:1-8` (add `mod streams;`).

- [ ] **Step 1:** Add `mod streams;` to the module list in `main.rs`.

- [ ] **Step 2: Create `streams.rs`** with classifier + tests first, HTTP after:

```rust
use crate::settings::Settings;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const API_PORT: u16 = 9997;
const API_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PathViewers {
    pub browser: u32,
    pub rtsp: u32,
    pub homekit: u32,
    pub total: u32,
    /// User-facing cap (excludes the HomeKit reserve slot).
    pub capacity: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ViewerReport {
    pub main: PathViewers,
    pub sub: PathViewers,
}

impl ViewerReport {
    /// Main has room for another NON-local viewer (HomeKit's reserve not counted).
    pub fn main_available(&self) -> bool {
        self.main.browser + self.main.rtsp < self.main.capacity
    }
}

/// Query the local mediamtx API and classify every reader. None on any failure.
pub async fn viewer_report(settings: &Settings) -> Option<ViewerReport> {
    let paths = http_get_local("/v3/paths/list").await?;
    let sessions = http_get_local("/v3/rtspsessions/list").await?;
    classify(
        &paths,
        &sessions,
        &settings.rtsp_path,
        &settings.sub_rtsp_path,
        settings.rtsp_max_clients.max(0) as u32,
        settings.sub_rtsp_max_clients.max(0) as u32,
    )
}

fn classify(
    paths_json: &str,
    sessions_json: &str,
    main_path: &str,
    sub_path: &str,
    main_cap: u32,
    sub_cap: u32,
) -> Option<ViewerReport> {
    let paths: Value = serde_json::from_str(paths_json).ok()?;
    let sessions: Value = serde_json::from_str(sessions_json).ok()?;

    // rtsp session id -> is the reader local (HomeKit's ffmpeg)?
    let mut local_session = std::collections::HashMap::new();
    for item in sessions.get("items")?.as_array()? {
        let id = item.get("id")?.as_str()?.to_string();
        let remote = item.get("remoteAddr").and_then(Value::as_str).unwrap_or("");
        local_session.insert(id, remote.starts_with("127.0.0.1"));
    }

    let mut report = ViewerReport {
        main: PathViewers { capacity: main_cap, ..Default::default() },
        sub: PathViewers { capacity: sub_cap, ..Default::default() },
    };
    for item in paths.get("items")?.as_array()? {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        let target = if name == main_path {
            &mut report.main
        } else if name == sub_path {
            &mut report.sub
        } else {
            continue;
        };
        let Some(readers) = item.get("readers").and_then(Value::as_array) else {
            continue;
        };
        for reader in readers {
            let kind = reader.get("type").and_then(Value::as_str).unwrap_or("");
            let id = reader.get("id").and_then(Value::as_str).unwrap_or("");
            match kind {
                "webrtcSession" | "hlsMuxer" => target.browser += 1,
                "rtspSession" | "rtspsSession" => {
                    if local_session.get(id).copied().unwrap_or(false) {
                        target.homekit += 1;
                    } else {
                        target.rtsp += 1;
                    }
                }
                _ => target.rtsp += 1,
            }
            target.total += 1;
        }
    }
    Some(report)
}

/// Minimal HTTP/1.0 GET to the localhost mediamtx API. HTTP/1.0 forces
/// connection-close and forbids chunked bodies, so "read to EOF, split on
/// the blank line" is a complete client. Returns the body.
async fn http_get_local(path: &str) -> Option<String> {
    tokio::time::timeout(API_TIMEOUT, async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", API_PORT))
            .await
            .ok()?;
        let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.ok()?;
        let text = String::from_utf8(raw).ok()?;
        let (head, body) = text.split_once("\r\n\r\n")?;
        if !head.starts_with("HTTP/1.0 200") && !head.starts_with("HTTP/1.1 200") {
            return None;
        }
        Some(body.to_string())
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATHS: &str = r#"{"itemCount":2,"pageCount":1,"items":[
        {"name":"main","readers":[{"type":"webrtcSession","id":"w1"},{"type":"rtspSession","id":"r1"}]},
        {"name":"sub","readers":[{"type":"rtspSession","id":"r2"},{"type":"hlsMuxer","id":"h1"}]}
    ]}"#;
    const SESSIONS: &str = r#"{"itemCount":2,"pageCount":1,"items":[
        {"id":"r1","remoteAddr":"192.168.2.50:61044"},
        {"id":"r2","remoteAddr":"127.0.0.1:44064"}
    ]}"#;

    #[test]
    fn classifies_readers_by_type_and_locality() {
        let report = classify(PATHS, SESSIONS, "main", "sub", 1, 2).unwrap();
        assert_eq!(report.main, PathViewers { browser: 1, rtsp: 1, homekit: 0, total: 2, capacity: 1 });
        assert_eq!(report.sub, PathViewers { browser: 1, rtsp: 0, homekit: 1, total: 2, capacity: 2 });
    }

    #[test]
    fn main_availability_ignores_homekit() {
        let paths = r#"{"items":[{"name":"main","readers":[{"type":"rtspSession","id":"r2"}]},{"name":"sub","readers":[]}]}"#;
        let report = classify(paths, SESSIONS, "main", "sub", 1, 2).unwrap();
        assert_eq!(report.main.homekit, 1);
        assert!(report.main_available(), "a HomeKit reader must not consume user capacity");
    }

    #[test]
    fn malformed_json_yields_none() {
        assert!(classify("not json", SESSIONS, "main", "sub", 1, 2).is_none());
        assert!(classify(PATHS, "{}", "main", "sub", 1, 2).is_none());
    }
}
```

- [ ] **Step 3:** `cargo test streams::` — 3 tests pass. `cargo build` clean.
- [ ] **Step 4:** Commit: `git add rust/octocam-web/src/streams.rs rust/octocam-web/src/main.rs && git commit -m "feat(web): mediamtx viewer counting via localhost API"`

---

## Task 4: Startup reconcile (config + service restart)

**Files:** Modify `rust/octocam-web/src/main.rs` (`async_main`)

The deployed Pi's `/etc/mediamtx.yml` still holds 1640x1232 and only regenerates on a settings save. Reconcile at startup.

- [ ] **Step 1:** In `async_main`, right after `let state = Arc::new(AppState::from_env());`, add:

```rust
    // Reconcile the mediamtx config with (possibly migrated) settings at startup,
    // restarting the RTSP service only when the rendered config actually changed.
    {
        let settings = settings::load_settings(&state.config_path);
        let config_path = state.mediamtx_config_path.clone();
        let _ = run_blocking(move || {
            match mediamtx::write_mediamtx_config(&settings, &config_path) {
                Ok(true) => {
                    let _ = system::restart_service("octocam-rtsp");
                }
                Ok(false) => {}
                Err(error) => tracing::warn!("mediamtx config reconcile failed: {error}"),
            }
        })
        .await;
    }
```

If `tracing` is not already imported in `main.rs`, use `eprintln!` in the same shape as neighboring code instead.

- [ ] **Step 2:** `cargo build && cargo test` — clean, all pass.
- [ ] **Step 3:** Commit: `git commit -am "fix(web): reconcile mediamtx config at startup so settings fixes reach deployed devices"`

---

## Task 5: `/api/status` viewers + stream page reroute

**Files:** Modify `rust/octocam-web/src/main.rs` (api_status, stream handler, StreamTemplate), `rust/octocam-web/templates/stream.html`

- [ ] **Step 1: api_status.** Replace the current body (keeps auth guard):

```rust
async fn api_status(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    let (status, viewers) = tokio::join!(
        run_blocking(system::status),
        streams::viewer_report(&settings)
    );
    #[derive(Serialize)]
    struct StatusResponse {
        #[serde(flatten)]
        status: system::SystemStatus,
        viewers: Option<streams::ViewerReport>,
    }
    Ok(Json(StatusResponse { status: status?, viewers }).into_response())
}
```

(`serde::Serialize` is already imported in main.rs via `serde`; if not, `use serde::Serialize;`.) The `flatten` keeps every existing key so current `data-live-status` consumers are unaffected.

- [ ] **Step 2: StreamTemplate.** Add fields to the struct (find `struct StreamTemplate`, ~line 200):

```rust
    initial_stream: String,   // "main" | "sub"
    main_busy: bool,          // true when reroute chose sub because main is full
    viewers: Option<streams::ViewerReport>,
```

- [ ] **Step 3: stream handler.** In `async fn stream(...)`, after settings/status are loaded and before `render(StreamTemplate { ... })`:

```rust
    let viewers = streams::viewer_report(&settings).await;
    let (initial_stream, main_busy) = match &viewers {
        Some(report) if !report.main_available() && settings.sub_stream_enabled => {
            ("sub".to_string(), true)
        }
        Some(_) => ("main".to_string(), false),
        None => (
            if settings.sub_stream_enabled { "sub" } else { "main" }.to_string(),
            false,
        ),
    };
```

and pass `initial_stream`, `main_busy`, `viewers` in the struct literal.

- [ ] **Step 4: stream.html.** Replace the hardcoded initial-source logic:

```html
            <div
              class="stream-preview"
              data-stream-preview
              data-main-src="{{ browser_stream_urls.main }}"
              data-sub-src="{{ browser_stream_urls.sub }}"
              data-initial-stream="{{ initial_stream }}"
            >
```

Update the two `aria-pressed` expressions to compare against `initial_stream` (`{% if initial_stream == "main" %}true{% else %}false{% endif %}` and the inverse), the iframe `src` to `{% if initial_stream == "sub" %}{{ browser_stream_urls.sub }}{% else %}{{ browser_stream_urls.main }}{% endif %}`, and add below the toolbar:

```html
              <p class="stream-note" data-stream-note {% if !main_busy %}hidden{% endif %}>
                Main stream is at capacity — showing the sub stream.
              </p>
```

In the RTSP card's `<dl>`, add viewer rows:

```html
              {% if viewers.is_some() %}
                {% let report = viewers.as_ref().unwrap() %}
                <div><dt>Main viewers</dt><dd data-viewers-main>{{ report.main.total }} / {{ report.main.capacity }}</dd></div>
                <div><dt>Sub viewers</dt><dd data-viewers-sub>{{ report.sub.total }} / {{ report.sub.capacity }}</dd></div>
              {% else %}
                <div><dt>Viewers</dt><dd data-viewers-main>unavailable</dd></div>
              {% endif %}
```

(Askama supports `is_some()`/`let`; if the installed askama 0.12 rejects `{% let %}`, precompute display strings in the handler as `viewers_main_text`/`viewers_sub_text` `String` fields instead — choose whichever compiles.)

- [ ] **Step 5:** `cargo build && cargo test` — clean. `cargo clippy -- -D warnings` — clean.
- [ ] **Step 6:** Commit: `git commit -am "feat(web): live viewer counts in /api/status and stream page; reroute full main to sub"`

---

## Task 6: app.js — live counts + main-full fallback

**Files:** Modify `static/app.js` (the `[data-stream-preview]` block, ~line 687)

- [ ] **Step 1:** Inside the existing `if (streamPreview) { ... }` block, add state + helpers (complete code):

```js
  const note = streamPreview.querySelector("[data-stream-note]");
  let latestViewers = null;

  function mainIsFull() {
    if (!latestViewers || !latestViewers.main) return false;
    const m = latestViewers.main;
    return m.browser + m.rtsp >= m.capacity;
  }

  function showBusyNote(show) {
    if (note) note.hidden = !show;
  }

  window.addEventListener("octocam:status", (event) => {
    latestViewers = (event.detail && event.detail.viewers) || null;
    const mainCell = document.querySelector("[data-viewers-main]");
    const subCell = document.querySelector("[data-viewers-sub]");
    if (latestViewers) {
      if (mainCell) mainCell.textContent = `${latestViewers.main.total} / ${latestViewers.main.capacity}`;
      if (subCell) subCell.textContent = `${latestViewers.sub.total} / ${latestViewers.sub.capacity}`;
    } else if (mainCell) {
      mainCell.textContent = "unavailable";
    }
  });
```

- [ ] **Step 2:** In the existing choice-click handler (where `activeStream = choice.dataset.streamChoice || "main"` is set), guard main selection:

```js
      let requested = choice.dataset.streamChoice || "main";
      if (requested === "main" && mainIsFull() && sources.sub) {
        requested = "sub";
        showBusyNote(true);
      } else {
        showBusyNote(false);
      }
      activeStream = requested;
```

- [ ] **Step 3:** Find where app.js fetches `/api/status` on its 5s cycle (search `api/status`). After the JSON is parsed there, dispatch the event so the stream page block stays decoupled:

```js
      window.dispatchEvent(new CustomEvent("octocam:status", { detail: data }));
```

- [ ] **Step 4:** Manual check via preview (`.claude/launch.json` bridge): stream page renders, counts update within 5s, no console errors (`preview_console_logs`).
- [ ] **Step 5:** Commit: `git commit -am "feat(web): stream page live viewer counts and main-full fallback"`

---

## Task 7: `/snapshot.jpg` cache (single-flight, 2s TTL)

**Files:** Modify `rust/octocam-web/src/camera.rs`, `rust/octocam-web/src/main.rs`

- [ ] **Step 1: Failing test** in `camera.rs` (new `mod tests`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn snapshot_freshness_window_is_two_seconds() {
        let now = Instant::now();
        assert!(snapshot_is_fresh(now, now + Duration::from_millis(1900)));
        assert!(!snapshot_is_fresh(now, now + Duration::from_millis(2100)));
    }
}
```

- [ ] **Step 2:** `cargo test camera::` — FAIL (missing fn).

- [ ] **Step 3: Implement.** In `camera.rs`:

```rust
use std::time::{Duration, Instant};

pub const SNAPSHOT_TTL: Duration = Duration::from_secs(2);

pub fn snapshot_is_fresh(captured: Instant, now: Instant) -> bool {
    now.duration_since(captured) < SNAPSHOT_TTL
}
```

In `main.rs`: add to `AppState`:

```rust
    snapshot_cache: Arc<tokio::sync::Mutex<Option<(std::time::Instant, Vec<u8>)>>>,
```

initialize in `AppState::from_env()` with `Arc::new(tokio::sync::Mutex::new(None))` (add `use std::sync::Arc;` already present). Replace the capture block in the `snapshot` handler:

```rust
    let mut cache = state.snapshot_cache.lock().await;
    if let Some((at, bytes)) = cache.as_ref() {
        if camera::snapshot_is_fresh(*at, std::time::Instant::now()) {
            let bytes = bytes.clone();
            return Ok(([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response());
        }
    }
    // Cold path: hold the lock across capture so concurrent requests coalesce
    // onto one rpicam-still run (capture itself is bounded by CAPTURE_TIMEOUT).
    let settings_for_capture = settings.clone();
    match run_blocking(move || camera::capture_jpeg(&settings_for_capture)).await? {
        Ok(data) => {
            *cache = Some((std::time::Instant::now(), data.clone()));
            Ok(([(header::CONTENT_TYPE, "image/jpeg")], data).into_response())
        }
        Err(error) => Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Snapshot unavailable: {error}\n"),
        )
            .into_response()),
    }
```

- [ ] **Step 4:** `cargo test && cargo clippy -- -D warnings` — pass.
- [ ] **Step 5:** Commit: `git commit -am "feat(web): cache /snapshot.jpg with 2s TTL and single-flight capture"`

---

## Task 8: HomeKit — per-session stream choice, sub fallback, snapshot cache

**Files:** Modify `homekit/octocam-homekit.js`

- [ ] **Step 1: Pure chooser + cache scaffolding.** Below `sourceStream(...)` add:

```js
const MAIN_QUALITY_MIN_HEIGHT = 720;
const MAIN_QUALITY_MIN_BITRATE_KBPS = 500;
const SNAPSHOT_CACHE_TTL_MS = 5000;

function localIpv4Prefixes() {
  const os = require("os");
  const prefixes = [];
  for (const addrs of Object.values(os.networkInterfaces() || {})) {
    for (const addr of addrs || []) {
      if (addr.family === "IPv4" && !addr.internal) {
        prefixes.push(addr.address.split(".").slice(0, 3).join(".") + ".");
      }
    }
  }
  return prefixes;
}

// Local viewers get main; remote/cellular (small frame, tight bitrate — HomeKit's
// remote profile) get sub. Network address alone is unreliable because hub-relayed
// remote sessions present LAN addresses, so requested quality is the primary signal.
function chooseStream(settings, video, targetAddress) {
  if (!settings.sub_stream_enabled) return "main";
  const height = Number.parseInt((video && video.height) || 0, 10);
  const bitrate = Number.parseInt((video && video.max_bit_rate) || 0, 10);
  const wantsMainQuality =
    height >= MAIN_QUALITY_MIN_HEIGHT || bitrate >= MAIN_QUALITY_MIN_BITRATE_KBPS;
  if (!wantsMainQuality) return "sub";
  if (targetAddress && targetAddress.includes(".")) {
    const onLan = localIpv4Prefixes().some((prefix) => targetAddress.startsWith(prefix));
    if (!onLan) return "sub";
  }
  return "main";
}
```

- [ ] **Step 2: Use it in `handleStreamRequest`.** Replace `const stream = sourceStream(settings);` in the start-stream path with:

```js
    const stream = chooseStream(settings, request.video, sessionInfo.address);
```

(`sessionInfo.address` is set from `prepareStream`'s `request.targetAddress` — verify the property name in `prepareStream` and reuse it.)

- [ ] **Step 3: Sub fallback on main failure.** In the ffmpeg start error path (`finishStart(new Error(...))` where the child exits before starting), when `stream === "main"` and `settings.sub_stream_enabled`, retry once: extract the args build into `buildStreamArgs(settings, streamName, request, sessionInfo)` and on first failure call it again with `"sub"` and spawn once more before reporting failure. Log both attempts.

- [ ] **Step 4: Snapshot cache + always-sub.** At module scope:

```js
let snapshotCache = { at: 0, buffer: null };
let snapshotInFlight = null;
```

In `handleSnapshotRequest`, before spawning ffmpeg:

```js
    const now = Date.now();
    if (snapshotCache.buffer && now - snapshotCache.at < SNAPSHOT_CACHE_TTL_MS) {
      callback(undefined, snapshotCache.buffer);
      return;
    }
    if (snapshotInFlight) {
      snapshotInFlight.then((buffer) => callback(undefined, buffer)).catch((error) => callback(error));
      return;
    }
```

and where the capture succeeds, populate the cache and clear `snapshotInFlight` (wrap the existing `runProcess("ffmpeg", ...)` promise as `snapshotInFlight`). Snapshot source stays `sourceStream(settings)` (sub when enabled) — no change needed there.

- [ ] **Step 5: Manual verification** (no Node test runner in repo): `node -e 'const m = require("./homekit/octocam-homekit.js")'` is not viable (starts the daemon) — instead verify by syntax check `node --check homekit/octocam-homekit.js` and the on-Pi test in Task 10.
- [ ] **Step 6:** Commit: `git commit -am "feat(homekit): per-session main/sub choice, sub fallback, snapshot cache"`

---

## Task 9: Captive-portal listener (setup mode only)

**Files:** Modify `rust/octocam-web/src/main.rs`

- [ ] **Step 1: Failing test** for the redirect URL builder (append to main.rs `mod tests`, or create one if absent — main.rs currently has no test module, so add at file end):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captive_redirect_builds_from_host_header() {
        assert_eq!(
            captive_redirect_target(Some("10.42.0.1")),
            "http://10.42.0.1:8080/setup"
        );
        assert_eq!(
            captive_redirect_target(Some("10.42.0.1:80")),
            "http://10.42.0.1:8080/setup"
        );
        assert_eq!(captive_redirect_target(None), "http://10.42.0.1:8080/setup");
    }
}
```

- [ ] **Step 2:** `cargo test captive_redirect` — FAIL.

- [ ] **Step 3: Implement** in `main.rs`:

```rust
/// Where captive-portal probes are redirected. Falls back to the NetworkManager
/// shared-mode gateway address used by the OctoCam-Setup AP.
fn captive_redirect_target(host_header: Option<&str>) -> String {
    let host = host_header
        .map(|value| value.split(':').next().unwrap_or(value))
        .filter(|value| !value.is_empty())
        .unwrap_or("10.42.0.1");
    format!("http://{host}:8080/setup")
}

async fn captive_probe(headers: HeaderMap) -> Response {
    let host = headers.get(http::header::HOST).and_then(|v| v.to_str().ok());
    Redirect::temporary(&captive_redirect_target(host)).into_response()
}

fn spawn_captive_portal_listener() {
    tokio::spawn(async {
        let app = Router::new()
            .route("/hotspot-detect.html", get(captive_probe))
            .route("/generate_204", get(captive_probe))
            .fallback(get(captive_probe));
        match tokio::net::TcpListener::bind("0.0.0.0:80").await {
            Ok(listener) => {
                let _ = axum::serve(listener, app).await;
            }
            Err(error) => {
                eprintln!("captive portal listener unavailable (port 80): {error}");
            }
        }
    });
}
```

In `async_main`, after the mediamtx reconcile block:

```rust
    {
        let settings = settings::load_settings(&state.config_path);
        if !settings.setup_complete {
            spawn_captive_portal_listener();
        }
    }
```

- [ ] **Step 4:** `cargo test && cargo clippy -- -D warnings` — pass.
- [ ] **Step 5:** Commit: `git commit -am "feat(web): captive-portal probe redirects during first-boot setup"`

---

## Task 10: Build, deploy, verify on the Pi

**Files:** none (verification)

- [ ] **Step 1:** `cargo test && cargo clippy -- -D warnings` (full suite; expect 19 pre-existing + ~10 new, all green) and `node --check homekit/octocam-homekit.js`.
- [ ] **Step 2:** Cross-build + deploy (health-gated): `scripts/build-pi-web.sh && OCTOCAM_PI_SSH=root@192.168.2.211 OCTOCAM_SERVICE_USER=root scripts/deploy-pi-web.sh --skip-build`. Also rsync the homekit daemon: `rsync -az homekit/octocam-homekit.js root@192.168.2.211:/root/OctoCam/homekit/ && ssh root@192.168.2.211 'systemctl restart octocam-homekit'`.
- [ ] **Step 3: Main stream fixed.** `ssh root@192.168.2.211 'grep -A3 "\"main\"" /etc/mediamtx.yml | grep rpiCameraHeight'` → `972` (startup reconcile rewrote it). `ssh root@192.168.2.211 'timeout 8 ffprobe -v error -rtsp_transport tcp -i rtsp://127.0.0.1:8554/main -show_streams 2>&1 | head -3'` → shows an H264 video stream, no 400.
- [ ] **Step 4: Counting.** Open the dashboard stream page (preview bridge) + start one RTSP client (`ffplay rtsp://192.168.2.211:8554/main` from the Mac or a second ffprobe on the Pi). `curl -s http://192.168.2.211:8080/api/status -H 'Cookie: <session>' | python3 -m json.tool | grep -A8 viewers` → counts match reality; close the RTSP client → counts drop within one 5s poll (graceful close).
- [ ] **Step 5: Reroute.** With one WebRTC viewer on main (maxReaders now `1 (+1 reserve)` = 2, capacity 1), open the stream page in a second browser tab → it must land on sub with the busy note visible.
- [ ] **Step 6: HomeKit.** Open the Home app on-LAN: tile snapshots at most every 5s in `journalctl -u octocam-rtsp` (cache working), live view logs `Starting HomeKit main stream` (quality heuristic chose main). If no Apple device is at hand, mark this a deferred manual check in the commit message.
- [ ] **Step 7: Snapshot throttle.** `for i in 1 2 3 4; do curl -s -o /dev/null -w "%{time_total}s\n" http://192.168.2.211:8080/snapshot.jpg -H 'Cookie: <session>'; done` → first slow (capture), rest ~instant (cache) with only one `rpicam-still` in the process list during the burst.
- [ ] **Step 8:** Commit any fixups; report results.

---

## Self-Review

- **Spec coverage:** §1 settings fix → Task 1; §2 config/API/reserve → Task 2 (+ Task 4 reconcile, which the spec's "fixes the currently-deployed device" line requires); §3 counting → Task 3; §4 status/reroute/UI → Tasks 5–6; §5 HomeKit chooser/fallback → Task 8; §6 snapshot throttling → Tasks 7 (web) + 8 (HomeKit); §7 captive portal → Task 9; testing section → per-task tests + Task 10 hardware matrix. No uncovered spec requirement found.
- **Placeholder scan:** all code steps carry complete code; the two "verify the property name" notes (Task 8 Step 2, Task 1 Step 3 load-path note) are explicit read-first instructions with stated expectations, not deferred design.
- **Type consistency:** `ViewerReport`/`PathViewers`/`main_available()` names match across Tasks 3/5/6; `render_mediamtx_config`/`write_mediamtx_config -> Result<bool, String>` consistent between Tasks 2/4; `snapshot_is_fresh`/`SNAPSHOT_TTL` consistent between camera.rs and the handler; `chooseStream(settings, video, targetAddress)` signature consistent within Task 8.
