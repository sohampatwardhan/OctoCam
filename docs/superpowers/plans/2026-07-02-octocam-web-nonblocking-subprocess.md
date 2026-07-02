# OctoCam Web Non-Blocking / Bounded Subprocess Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every external-command call in `octocam-web` time-bounded and keep it off the Tokio worker/reactor threads, so a wedged `iw`/`nmcli`/`rpicam` process can never freeze the web UI again.

**Architecture:** Introduce one small synchronous helper module (`proc`) that runs a `std::process::Command` with a wall-clock timeout, draining stdout/stderr on reader threads and killing the child on timeout. Route every existing `Command::…output()/status()` call through it. Then, in `main.rs`, move the (still-synchronous) status/scan/connect/capture helpers off the async worker threads with `tokio::task::spawn_blocking`, so slow-but-bounded commands never starve the I/O reactor.

**Tech Stack:** Rust, Tokio (multi-thread runtime, `process` + `time` features already enabled), Axum 0.8, `std::process`. **No new crate dependencies.**

**Background (root cause this fixes):** On 2026-07-02 the live Pi (`192.168.2.211`) served zero requests — even static files, even on loopback — while sitting at 0.2% CPU. A child `iw dev` (from `system.rs` `wireless_interfaces()` → `run_output("iw", &["dev"])`) had wedged for 8+ minutes. Because it was a synchronous `Command::output()` on a Tokio worker thread with no timeout, the worker that cooperatively drives the mio/epoll reactor never returned, so no connections were accepted. Killing the child instantly restored service (`HTTP 200` in ~2ms). Two independent defects: (1) no timeout on any subprocess; (2) blocking subprocesses run directly on async worker threads.

---

## File Structure

- **Create** `rust/octocam-web/src/proc.rs` — bounded subprocess runner (`run(&mut Command, Duration) -> io::Result<Output>`) + timeout constants + unit tests. Single responsibility: run a child process safely and never hang.
- **Modify** `rust/octocam-web/src/main.rs` — declare `mod proc;`; add `run_blocking` helper; wrap the synchronous helper calls in handlers with `spawn_blocking`; make the deferred `systemctl` fire-and-forget use `spawn_blocking` + `proc::run`.
- **Modify** `rust/octocam-web/src/system.rs` — route all `Command` call sites through `proc::run` with appropriate timeouts. `thread::sleep(120ms)` stays (it now runs on the blocking pool).
- **Modify** `rust/octocam-web/src/wifi.rs` — route all `Command` call sites through `proc::run`.
- **Modify** `rust/octocam-web/src/camera.rs` — route the still-capture command through `proc::run` with the capture timeout.
- **Modify** `rust/octocam-web/src/wifi_setup.rs` — route its `Command` call sites through `proc::run` (first-boot AP CLI mode; lower severity, done for consistency and to bound the setup flow).

**No `Cargo.toml` change:** `tokio` already enables `process` and `time`; the helper uses only `std`.

---

## Design Decisions (read before starting)

1. **`proc::run` takes `&mut Command`.** Rust extends a temporary's lifetime to the end of the statement, so existing fluent builders convert with almost no churn:
   - `Command::new("nmcli").args(["dev","wifi"]).output()` → `proc::run(Command::new("nmcli").args(["dev","wifi"]), proc::SCAN_TIMEOUT)`
   - `.status().map(|s| s.success())` → `proc::run(…, T).map(|o| o.status.success())`
   - Where a `Command` is already a bound variable (`let mut command = …; command.output()`), use `proc::run(&mut command, T)`.

2. **`proc::run` return type is `io::Result<Output>`** — identical to `Command::output()`. Existing call sites already handle `io::Result<Output>` via `?`, `.ok()?`, or `.map_err(|e| e.to_string())`, so a timeout surfaces as a normal `io::Error` (kind `TimedOut`) with no new error plumbing.

3. **Timeout constants (in `proc.rs`):**
   - `DEFAULT_TIMEOUT = 5s` — quick local queries (`ip`, `iw link`, `systemctl show`, `hostname`, `wpa_cli`, `command -v`).
   - `SCAN_TIMEOUT = 12s` — Wi-Fi scans and camera enumeration (legitimately slow).
   - `SERVICE_TIMEOUT = 10s` — `systemctl enable/disable/start/stop/restart`.
   - `CONNECT_TIMEOUT = 25s` — `nmcli … connect` / `connection up` (association + DHCP).
   - `CAPTURE_TIMEOUT = 8s` — `rpicam-still`/`libcamera-still` capture.

4. **Both fixes are required.** The timeout alone prevents the *permanent* hang (worst case a page is slow, not dead). `spawn_blocking` keeps the 4 worker threads (one per Pi core) free so bounded-but-slow commands don't starve the reactor under concurrency. Land `proc` + routing first (Tasks 1–5), then the boundary hardening (Task 6).

5. **Bound concurrency, not just time (added by hardening).** Tokio's blocking pool defaults to **512 threads** with an **unbounded queue and no backpressure** (verified against Tokio docs via Context7). On a 512MB device, unbounded `spawn_blocking` under a 5s dashboard auto-refresh can ratchet up threads/RAM. Tokio's own docs state: *"a semaphore or some other synchronization primitive should be used to limit the number of computations executed in parallel."* So we cap `max_blocking_threads` **and** gate `run_blocking` with a `Semaphore` (see FIX-1).

---

## Hardening Addenda (plan-harden thorough, 2026-07-02)

These fixes were produced by a 3-reviewer hardening pass (gap / code / adversarial) and cross-checked against Tokio + axum docs via Context7. They **override or augment** the numbered tasks below — apply them in place. Priority: P0 blocks the plan, P1 should-fix, P2 polish.

### FIX-1 (P0) — Bound the blocking pool + gate `run_blocking` with a semaphore
*Augments Task 6 Step 1; replaces the `#[tokio::main]` entry point.*

**Cargo.toml:** add the `sync` feature (needed for `tokio::sync::Semaphore`):

```toml
tokio = { version = "1", features = ["macros", "net", "process", "rt-multi-thread", "sync", "time"] }
```

**main.rs — replace the `#[tokio::main]` attribute + `async fn main()` signature** (around line 258). Remove `#[tokio::main]` and build the runtime explicitly so the blocking pool is bounded:

```rust
fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        // Default is 512; far too many 2 MB-stack threads for a 512 MB Pi Zero 2 W.
        .max_blocking_threads(12)
        .build()
        .expect("build Tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    // ... the ENTIRE existing body of `async fn main()` moves here verbatim ...
}
```

**main.rs — add the concurrency gate and use it in `run_blocking`** (this replaces Task 6 Step 1's `run_blocking`):

```rust
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// Caps how many subprocess-heavy helpers run at once, independent of request volume.
/// Tokio docs explicitly recommend a semaphore to bound spawn_blocking concurrency.
fn blocking_gate() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(4))
}

/// Run a blocking (subprocess-heavy) closure on Tokio's blocking pool so it never
/// occupies a worker/reactor thread, while bounding total concurrency. Maps a panic
/// in the closure (JoinError) to a 500.
async fn run_blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let _permit = blocking_gate()
        .acquire()
        .await
        .map_err(|_| AppError("blocking gate closed".to_string()))?;
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|error| AppError(format!("background task failed: {error}")))
}
```

The `_permit` is held across the `spawn_blocking(...).await`, so it is released only when the blocking work finishes. `Semaphore::new(4)` allows 4 concurrent subprocess batches; tune with the CHECK-3 measurement.

### FIX-2 (P0) — `scan_wifi` cannot use `?` (won't compile)
*Overrides Task 6 Step 4 for the `scan_wifi` handler only.*

`scan_wifi` returns `Response`, not `AppResult` ([main.rs:743](../../rust/octocam-web/src/main.rs)). `?` on `Result<_, AppError>` needs a `Result`-returning fn (confirmed via axum docs). Do **not** use `.await?` here — handle both the `JoinError` and the inner `Result` explicitly:

```rust
    let cache_path = state.wifi_cache_path.clone();
    let message = match run_blocking(move || wifi::scan_and_cache_networks(&cache_path)).await {
        Ok(Ok(_cache)) => "Wi-Fi scan complete.".to_string(), // keep the existing success text
        Ok(Err(error)) => error,                              // keep the existing error branch
        Err(_join) => "Wi-Fi scan failed.".to_string(),
    };
```

(Match the existing success/error strings from the current `scan_wifi` body.) `api_wifi_scan` returns `AppResult`, so the plain `.await?` from Task 6 Step 4 is fine there — this override is `scan_wifi`-specific.

### FIX-3 (P1) — Cache `command_exists` (kills repeated subprocesses + inline block)
*Replaces Task 2 Step 3.* The available-tool set is static at runtime, so memoize it. This removes ~8 `sh -c` calls per `status()` and the inline block in `schedule_systemctl`.

```rust
use std::sync::{Mutex, OnceLock}; // add near the top of system.rs (HashMap is already imported)

pub fn command_exists(command: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(&hit) = cache.lock().unwrap().get(command) {
        return hit;
    }
    let exists = crate::proc::run(
        Command::new("sh").args([
            "-c",
            &format!("command -v {} >/dev/null 2>&1", shell_escape(command)),
        ]),
        crate::proc::DEFAULT_TIMEOUT,
    )
    .map(|output| output.status.success())
    .unwrap_or(false);
    cache.lock().unwrap().insert(command.to_string(), exists);
    exists
}
```

Residual: the very first `/power` request could still block ≤5s on a cold `command_exists("systemctl")` (usually already warmed by earlier `status()` calls). Acceptable for a rare admin action; wrapping `schedule_power_action` in `run_blocking` is an optional extra.

### FIX-4 (P1) — Clean up NetworkManager after a killed `nmcli connect`
*Augments Task 3 Step 3.* SIGKILL of the `nmcli` client does not cancel the daemon-side activation, which can leave a half-activated profile that then fights a manual `wpa_cli` fallback. In `connect_to_network` ([wifi.rs:161-171](../../rust/octocam-web/src/wifi.rs)), force NM back to a known state on any nmcli failure before falling through:

```rust
    let nmcli_result = run_connect_command(command);
    if nmcli_result.0 {
        disable_setup_ap();
        return nmcli_result;
    }

    // FIX-4: a failed/timed-out `nmcli dev wifi connect` can leave NetworkManager with a
    // half-activated profile. Force it down before trying wpa_supplicant, so NM and a
    // manual wpa_cli attempt don't fight over the same interface. Best-effort, bounded.
    let _ = crate::proc::run(
        Command::new("nmcli").args(["connection", "down", "id", ssid]),
        crate::proc::SERVICE_TIMEOUT,
    );

    let wpa_result = connect_with_wpa_cli(ssid, password, &security);
    // ... unchanged ...
```

### FIX-5 (P1) — Deploy rollback + HTTP health gate
*Augments Task 7 (deploy).* `deploy-pi-web.sh` restarts `octocam-web` (the Wi-Fi control plane) unconditionally and only checks `systemctl is-active`, not that HTTP serves. Add: back up the current binary, and after restart, health-check; roll back on failure. Add to the remote block in `scripts/deploy-pi-web.sh` (or run inline over SSH during Task 7 Step 3):

```bash
# before overwrite (remote):
sudo -n cp -f /usr/local/bin/octocam-web /usr/local/bin/octocam-web.bak 2>/dev/null || true
# after `systemctl restart octocam-web` (remote):
sleep 2
if ! curl -fsS -m 8 -o /dev/null http://127.0.0.1:8080/login; then
  echo "health check FAILED — rolling back"
  sudo -n cp -f /usr/local/bin/octocam-web.bak /usr/local/bin/octocam-web
  sudo -n systemctl restart octocam-web
  exit 1
fi
```

### FIX-6 (P1) — Circuit-breaker for a persistently wedged command
*New; augments `proc` usage in `system.rs`.* A permanently wedged tool (the original `iw` incident) would otherwise be spawned+killed every ~5s forever. Add a short negative cache keyed on `command`, checked by `run_output` (and other hot leaf callers):

```rust
// system.rs — a per-command "recently timed out" breaker.
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const BREAKER_COOLDOWN: Duration = Duration::from_secs(30);

fn command_recently_timed_out(command: &str) -> bool {
    static BREAKER: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let breaker = BREAKER.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = breaker.lock().unwrap();
    if let Some(&when) = map.get(command) {
        if when.elapsed() < BREAKER_COOLDOWN {
            return true;
        }
        map.remove(command);
    }
    false
}

fn note_command_timeout(command: &str) {
    static BREAKER: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    // NOTE: use the SAME OnceLock as command_recently_timed_out — hoist both into one
    // module-level helper struct so they share the map. (Shown split for readability.)
    let breaker = BREAKER.get_or_init(|| Mutex::new(HashMap::new()));
    breaker.lock().unwrap().insert(command.to_string(), Instant::now());
}
```

Then in `run_output` (system.rs:361): short-circuit if the breaker is open, and record timeouts:

```rust
pub fn run_output(command: &str, args: &[&str]) -> Option<String> {
    if command_recently_timed_out(command) {
        return None;
    }
    match crate::proc::run(Command::new(command).args(args), crate::proc::DEFAULT_TIMEOUT) {
        Ok(output) => Some(
            String::from_utf8_lossy(if output.stdout.is_empty() {
                &output.stderr
            } else {
                &output.stdout
            })
            .trim()
            .to_string(),
        ),
        Err(error) => {
            if error.kind() == std::io::ErrorKind::TimedOut {
                note_command_timeout(command);
            }
            None
        }
    }
}
```

**Implementation note:** collapse the two `OnceLock`s above into a single shared map (e.g. a `fn breaker() -> &'static Mutex<HashMap<String, Instant>>` helper) so `note_command_timeout` and `command_recently_timed_out` read/write the same state — the split shown is for readability only.

### P2 polish (apply as notes / follow-ups, not blockers)
- **FIX-7 — process-group kill in `proc::run`.** `child.kill()` kills only the direct child (matches Tokio's own examples). Safe for all current call sites (none background a surviving grandchild). Defense-in-depth for future `sh -c "… &"` callers: add `.process_group(0)` (Rust 1.64+, `std::os::unix::process::CommandExt`) before spawn and kill the group, or document the constraint at the top of `proc.rs`.
- **FIX-8 — clone precision (Task 6 Step 5).** Both `connect_to_network` call sites already own their `String`s and don't reuse them after the call, so a plain `move` (no `.clone()`) suffices — drop the redundant clones the step hedged about.
- **FIX-9 — widen Task 7 curl timeouts.** With sequential per-command timeouts, a worst-case `status()` with several simultaneously-slow tools can take tens of seconds. Raise the smoke/concurrency-test `curl -m` values (Step 4/5) to ~40s, or note the expected worst case, so verification doesn't spuriously fail.

### Additional verification (append to Task 7)
- [ ] **CHECK-1 (highest priority) — prove the regression is actually fixed.** Point `iw`/`nmcli` at a stub on `PATH` that hangs (`#!/bin/sh` + `sleep 30`), start the real app, and assert concurrent `/login` requests stay fast while the wedged request returns a bounded error. Neither the unit test nor "40× curl /system" injects a real hang.
- [ ] **CHECK-2 — measure `status()` latency on the Pi under load** (camera + RTSP running); confirm `DEFAULT_TIMEOUT=5s` causes no false failures.
- [ ] **CHECK-3 — confirm thread/RAM stays bounded** under sustained `/api/status` polling with a wedged command, after FIX-1; tune `Semaphore` / `max_blocking_threads` from the numbers.
- [ ] **CHECK-4 — verify NetworkManager recovers** after a killed `nmcli connect` (device reconnects without reboot), validating FIX-4.
- [ ] **CHECK-5 — rehearse the deploy rollback** (FIX-5) once against the Pi.

---

## Task 1: Create the `proc` bounded-subprocess module

**Files:**
- Create: `rust/octocam-web/src/proc.rs`
- Modify: `rust/octocam-web/src/main.rs:1-7` (module declarations)

- [ ] **Step 1: Declare the module**

In `rust/octocam-web/src/main.rs`, add `mod proc;` to the module list (keep alphabetical-ish grouping):

```rust
mod camera;
mod mediamtx;
mod proc;
mod security;
mod settings;
mod system;
mod wifi;
mod wifi_setup;
```

- [ ] **Step 2: Write `proc.rs` with the runner and unit tests**

Create `rust/octocam-web/src/proc.rs`:

```rust
use std::io::{self, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Quick local queries (ip, iw link, systemctl show, hostname, wpa_cli, command -v).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Wi-Fi scans and camera enumeration, which are legitimately slow.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(12);
/// systemctl enable/disable/start/stop/restart.
pub const SERVICE_TIMEOUT: Duration = Duration::from_secs(10);
/// nmcli connect / connection up (association + DHCP).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(25);
/// rpicam-still / libcamera-still capture.
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);

/// How often the wait loop polls the child while waiting for exit or timeout.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Run `command`, capturing stdout/stderr, but never block longer than `timeout`.
///
/// Behavior:
/// - stdin is `/dev/null` so a child can never block waiting for input.
/// - stdout and stderr are drained on dedicated threads, so a child that writes
///   more than the pipe buffer (~64 KiB) cannot deadlock against a full pipe.
/// - On timeout the child is killed and an `io::Error` of kind `TimedOut` is returned.
///
/// Returns the same `io::Result<Output>` shape as `Command::output()`, so a
/// non-zero exit is `Ok(output)` with `output.status.success() == false`.
pub fn run(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;

    let mut child_stdout = child.stdout.take().expect("stdout piped");
    let mut child_stderr = child.stderr.take().expect("stderr piped");
    let out_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = child_stdout.read_to_end(&mut buf);
        buf
    });
    let err_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = child_stderr.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            // Child is dead; readers hit EOF. Join so no threads/pipes leak.
            let _ = out_handle.join();
            let _ = err_handle.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("command timed out after {timeout:?}"),
            ));
        }
        thread::sleep(POLL_INTERVAL);
    };

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_stdout_for_fast_command() {
        let out = run(Command::new("sh").args(["-c", "printf hello"]), DEFAULT_TIMEOUT)
            .expect("command should run");
        assert!(out.status.success());
        assert_eq!(out.stdout, b"hello");
    }

    #[test]
    fn nonzero_exit_is_ok_not_err() {
        let out = run(Command::new("sh").args(["-c", "exit 3"]), DEFAULT_TIMEOUT)
            .expect("command should run");
        assert!(!out.status.success());
    }

    #[test]
    fn kills_and_errors_on_timeout() {
        let start = Instant::now();
        let err = run(Command::new("sh").args(["-c", "sleep 30"]), Duration::from_millis(300))
            .expect_err("command should time out");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        // Must return promptly, not after the full 30s sleep.
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn captures_large_output_without_deadlock() {
        // ~220 KB, well past the typical 64 KiB pipe buffer.
        let out = run(
            Command::new("sh").args(["-c", "yes 0123456789 | head -n 20000"]),
            Duration::from_secs(10),
        )
        .expect("command should run");
        assert!(out.status.success());
        assert!(out.stdout.len() > 100_000);
    }

    #[test]
    fn spawn_failure_is_err() {
        let err = run(
            &mut Command::new("definitely-not-a-real-binary-xyz"),
            DEFAULT_TIMEOUT,
        )
        .expect_err("spawn should fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cd rust/octocam-web && cargo test proc::`
Expected: PASS — 5 tests (`returns_stdout_for_fast_command`, `nonzero_exit_is_ok_not_err`, `kills_and_errors_on_timeout`, `captures_large_output_without_deadlock`, `spawn_failure_is_err`).

- [ ] **Step 4: Commit**

```bash
git add rust/octocam-web/src/proc.rs rust/octocam-web/src/main.rs
git commit -m "feat(web): add bounded subprocess runner (proc module)"
```

---

## Task 2: Route `system.rs` subprocess calls through `proc::run`

**Files:**
- Modify: `rust/octocam-web/src/system.rs` (lines 308, 328, 344, 362, 1121)

- [ ] **Step 1: `set_service_enabled` (around line 308)**

Old:

```rust
        let output = Command::new("systemctl")
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
```

New:

```rust
        let output = crate::proc::run(Command::new("systemctl").args(args), crate::proc::SERVICE_TIMEOUT)
            .map_err(|error| error.to_string())?;
```

- [ ] **Step 2: `restart_service` (around line 328)**

Old:

```rust
    let output = Command::new("systemctl")
        .args(["restart", unit])
        .output()
        .map_err(|error| error.to_string())?;
```

New:

```rust
    let output = crate::proc::run(
        Command::new("systemctl").args(["restart", unit]),
        crate::proc::SERVICE_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
```

- [ ] **Step 3: `command_exists` (around line 344)**

Old:

```rust
    Command::new("sh")
        .args([
            "-c",
            &format!("command -v {} >/dev/null 2>&1", shell_escape(command)),
        ])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
```

New:

```rust
    crate::proc::run(
        Command::new("sh").args([
            "-c",
            &format!("command -v {} >/dev/null 2>&1", shell_escape(command)),
        ]),
        crate::proc::DEFAULT_TIMEOUT,
    )
    .map(|output| output.status.success())
    .unwrap_or(false)
```

- [ ] **Step 4: `run_output` (around line 362)** — this is the exact call path that wedged the server (`run_output("iw", &["dev"])`).

Old:

```rust
    let output = Command::new(command).args(args).output().ok()?;
```

New:

```rust
    let output = crate::proc::run(Command::new(command).args(args), crate::proc::DEFAULT_TIMEOUT).ok()?;
```

- [ ] **Step 5: `camera_status` `--list-cameras` (around line 1121)**

Old:

```rust
    let output = Command::new(&command).arg("--list-cameras").output();
```

New:

```rust
    let output = crate::proc::run(Command::new(&command).arg("--list-cameras"), crate::proc::SCAN_TIMEOUT);
```

- [ ] **Step 6: Build and run tests**

Run: `cd rust/octocam-web && cargo build && cargo test`
Expected: PASS, no warnings about unused `Command`/`thread`/`Duration` imports (all still used: `Command` for construction, `thread::sleep`/`Duration` at `system.rs:416`).

- [ ] **Step 7: Commit**

```bash
git add rust/octocam-web/src/system.rs
git commit -m "fix(web): bound all system.rs subprocess calls with timeouts"
```

---

## Task 3: Route `wifi.rs` subprocess calls through `proc::run`

**Files:**
- Modify: `rust/octocam-web/src/wifi.rs` (lines 88, 119, 242, 318, 491, 500)

- [ ] **Step 1: `scan_networks_with_nmcli` (around line 88)**

Old:

```rust
    let output = Command::new("nmcli")
        .args([
```

...through the closing `.output()` (line 99). Replace the whole `Command::new("nmcli")…​.output()` expression. Concretely, change the leading `Command::new("nmcli")` to `crate::proc::run(Command::new("nmcli")` and change the trailing:

```rust
        .output()
```

to:

```rust
        , crate::proc::SCAN_TIMEOUT)
```

Result (shape):

```rust
    let output = crate::proc::run(
        Command::new("nmcli").args([ /* unchanged args */ ]),
        crate::proc::SCAN_TIMEOUT,
    )
    .map_err(/* unchanged */)?;
```

Keep the existing `.map_err(...)?` exactly as it was.

- [ ] **Step 2: `scan_networks_with_iw` (around line 119)**

Old:

```rust
        let output = Command::new("iw")
            .args(["dev", &interface, "scan"])
            .output()
            .map_err(|error| error.to_string())?;
```

New:

```rust
        let output = crate::proc::run(
            Command::new("iw").args(["dev", &interface, "scan"]),
            crate::proc::SCAN_TIMEOUT,
        )
        .map_err(|error| error.to_string())?;
```

- [ ] **Step 3: `run_connect_command` (around line 242)** — takes `mut command: Command`.

Old:

```rust
    match command.output() {
```

New:

```rust
    match crate::proc::run(&mut command, crate::proc::CONNECT_TIMEOUT) {
```

- [ ] **Step 4: `run_wpa_cli` (around line 318)**

Old:

```rust
    let output = Command::new("wpa_cli")
        .args(...)
        .output();
```

New (keep the exact args expression; only wrap and set timeout):

```rust
    let output = crate::proc::run(Command::new("wpa_cli").args(/* unchanged */), crate::proc::DEFAULT_TIMEOUT);
```

- [ ] **Step 5: `disable_setup_ap` (around lines 491 and 500)** — two fire-and-forget `nmcli` calls.

For each, old:

```rust
    let _ = Command::new("nmcli")
        .args(...)
        .output();
```

New:

```rust
    let _ = crate::proc::run(Command::new("nmcli").args(/* unchanged */), crate::proc::SERVICE_TIMEOUT);
```

- [ ] **Step 6: Build and test**

Run: `cd rust/octocam-web && cargo build && cargo test`
Expected: PASS, no new warnings.

- [ ] **Step 7: Commit**

```bash
git add rust/octocam-web/src/wifi.rs
git commit -m "fix(web): bound all wifi.rs subprocess calls with timeouts"
```

---

## Task 4: Route `camera.rs` capture through `proc::run`

**Files:**
- Modify: `rust/octocam-web/src/camera.rs:31-34`

- [ ] **Step 1: Wrap the capture command**

Old:

```rust
    let output = Command::new(&command)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
```

New:

```rust
    let output = crate::proc::run(Command::new(&command).args(args), crate::proc::CAPTURE_TIMEOUT)
        .map_err(|error| error.to_string())?;
```

- [ ] **Step 2: Build**

Run: `cd rust/octocam-web && cargo build`
Expected: PASS. `use std::process::Command;` remains (still used to construct).

- [ ] **Step 3: Commit**

```bash
git add rust/octocam-web/src/camera.rs
git commit -m "fix(web): bound camera capture subprocess with a timeout"
```

---

## Task 5: Route `wifi_setup.rs` and the deferred `systemctl` in `main.rs`

**Files:**
- Modify: `rust/octocam-web/src/wifi_setup.rs` (lines 73, 189, 214, and the `nmcli`/`command_exists` helpers)
- Modify: `rust/octocam-web/src/main.rs:909-931` (`schedule_systemctl`)

`wifi_setup` runs only in first-boot AP CLI mode (`octocam-web --wifi-setup`, a one-shot that exits — it does not serve HTTP), so blocking is not catastrophic there, but bounding it prevents the setup flow from hanging forever.

- [ ] **Step 1: `wifi_setup.rs` `nmcli` helper (around line 189)**

Old:

```rust
fn nmcli<const N: usize>(args: [&str; N]) -> Result<Output, String> {
    let output = Command::new("nmcli")
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
```

New:

```rust
fn nmcli<const N: usize>(args: [&str; N]) -> Result<Output, String> {
    let output = crate::proc::run(Command::new("nmcli").args(args), crate::proc::CONNECT_TIMEOUT)
        .map_err(|error| error.to_string())?;
```

- [ ] **Step 2: `wifi_setup.rs` first `nmcli` status call (around line 73)**

Old:

```rust
        let output = Command::new("nmcli")
            .args(...)
            .output()
            .map_err(|error| error.to_string())?;
```

New:

```rust
        let output = crate::proc::run(Command::new("nmcli").args(/* unchanged */), crate::proc::DEFAULT_TIMEOUT)
            .map_err(|error| error.to_string())?;
```

- [ ] **Step 3: `wifi_setup.rs` `connection up` (around line 189, inside the connect loop)**

If distinct from the `nmcli` helper (an inline `Command::new("nmcli").args(["connection","up", …]).output()`), wrap it with `crate::proc::CONNECT_TIMEOUT` the same way as Step 2 but using `CONNECT_TIMEOUT`.

- [ ] **Step 4: `wifi_setup.rs` `sh` status (around line 214) and `command_exists`**

For the `.status()` sites (`Command::new("sh")…​.status()`), old:

```rust
    Command::new("sh")
        .args(...)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
```

New:

```rust
    crate::proc::run(Command::new("sh").args(/* unchanged */), crate::proc::DEFAULT_TIMEOUT)
        .map(|output| output.status.success())
        .unwrap_or(false)
```

For a bare `Command::new("sh").args(...).status()?` used for its side effect, use:

```rust
    crate::proc::run(Command::new("sh").args(/* unchanged */), crate::proc::DEFAULT_TIMEOUT)?;
```

- [ ] **Step 5: `main.rs` `schedule_systemctl` — fix blocking `.status()` inside `tokio::spawn` (around line 925)**

This fire-and-forget task currently blocks a worker thread on `.status()`. Move it to the blocking pool and bound it.

Old:

```rust
    tokio::spawn(async move {
        sleep(Duration::from_millis(900)).await;
        let _ = Command::new(command).args(command_args).status();
    });
```

New:

```rust
    tokio::spawn(async move {
        sleep(Duration::from_millis(900)).await;
        let _ = tokio::task::spawn_blocking(move || {
            let _ = proc::run(Command::new(command).args(command_args), proc::SERVICE_TIMEOUT);
        })
        .await;
    });
```

(`command` is `String`, `command_args` is `Vec<String>`; both are `Send + 'static`, so the closure is valid. `proc` is in-crate, referenced as `proc::` from `main.rs`.)

- [ ] **Step 6: Build and test**

Run: `cd rust/octocam-web && cargo build && cargo test`
Expected: PASS. Verify `Output` is still imported in `wifi_setup.rs` (its helpers return `Output`).

- [ ] **Step 7: Commit**

```bash
git add rust/octocam-web/src/wifi_setup.rs rust/octocam-web/src/main.rs
git commit -m "fix(web): bound wifi-setup and deferred systemctl subprocesses"
```

---

## Task 6: Move blocking helpers off the async worker threads (`spawn_blocking`)

**Files:**
- Modify: `rust/octocam-web/src/main.rs` — add `run_blocking` helper; update handler call sites.

Every synchronous helper that shells out (`system::status`, `system::stored_wifi_profiles`, `wifi::scan_and_cache_networks`, `wifi::connect_to_network`, `wifi::forget_saved_profile`, `camera::capture_jpeg`, `configure_homekit_service`) must run via `spawn_blocking` so bounded-but-slow commands cannot starve the 4 worker threads / the reactor.

- [ ] **Step 1: Add the `run_blocking` helper**

Add near the other free functions in `main.rs` (e.g. just below the `AppError` definition, around line 55):

```rust
/// Run a blocking (subprocess-heavy) closure on Tokio's blocking pool so it
/// never occupies a worker/reactor thread. Maps a panic in the closure to a 500.
async fn run_blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|error| AppError(format!("background task failed: {error}")))
}
```

- [ ] **Step 2: Wrap `system::status()` at every handler site**

There are 13 call sites of the form `let status = system::status();`. In each, replace with:

```rust
    let status = run_blocking(system::status).await?;
```

Exact sites (line numbers approximate — match on `system::status()`):
`identity` (390), `stream_settings` (413), `rtsp_page` (448), `homekit` (477), `admin` (501), `system_page` (525), `logs` (547), `terminal` (564), `stream` (581), `setup` (652), `system_page`/others. Also:
- `wifi_page` (413 area): `let status = system::status();` → `let status = run_blocking(system::status).await?;`
- `delete_wifi_profile` (808): `let active_ssid = system::status().wifi.ssid;` → `let active_ssid = run_blocking(system::status).await?.wifi.ssid;`
- `api_status` (986): `Ok(Json(system::status()).into_response())` → `let status = run_blocking(system::status).await?; Ok(Json(status).into_response())`

- [ ] **Step 3: Wrap `system::stored_wifi_profiles` (wifi_page, ~line 419)**

Old:

```rust
        stored_profiles: system::stored_wifi_profiles(&status.wifi),
```

Because it needs `&status.wifi` and runs subprocesses, compute it before building the template struct:

```rust
    let wifi_for_profiles = status.wifi.clone();
    let stored_profiles = run_blocking(move || system::stored_wifi_profiles(&wifi_for_profiles)).await?;
```

...and use `stored_profiles,` in the struct literal.

- [ ] **Step 4: Wrap `wifi::scan_and_cache_networks` (scan_wifi ~744, api_wifi_scan ~1008)**

Old (scan_wifi):

```rust
    let message = match wifi::scan_and_cache_networks(&state.wifi_cache_path) {
```

New:

```rust
    let cache_path = state.wifi_cache_path.clone();
    let message = match run_blocking(move || wifi::scan_and_cache_networks(&cache_path)).await? {
```

Apply the same pattern at `api_wifi_scan` (~1008): clone `state.wifi_cache_path` into the closure, `run_blocking(...).await?`, then keep the existing `match`.

- [ ] **Step 5: Wrap `wifi::connect_to_network` (connect_wifi ~712 and ~784)**

Old:

```rust
    let (connected, message) = wifi::connect_to_network(&wifi_ssid, &wifi_password, &wifi_security);
```

New:

```rust
    let (ssid, password, security) = (wifi_ssid.clone(), wifi_password.clone(), wifi_security.clone());
    let (connected, message) =
        run_blocking(move || wifi::connect_to_network(&ssid, &password, &security)).await?;
```

(If the earlier of the two sites builds the args differently, clone whatever the local owned `String`s are at that point.)

- [ ] **Step 6: Wrap `wifi::forget_saved_profile` (delete_wifi_profile ~816)**

Old:

```rust
    let (deleted, message) = wifi::forget_saved_profile(&profile_name, &profile_source);
```

New:

```rust
    let (name, source) = (profile_name.clone(), profile_source.clone());
    let (deleted, message) = run_blocking(move || wifi::forget_saved_profile(&name, &source)).await?;
```

- [ ] **Step 7: Wrap `camera::capture_jpeg` (snapshot ~1030)**

Old:

```rust
    match camera::capture_jpeg(&settings) {
```

New:

```rust
    let settings_for_capture = settings.clone();
    match run_blocking(move || camera::capture_jpeg(&settings_for_capture)).await? {
```

- [ ] **Step 8: Wrap `configure_homekit_service` (complete_setup ~736 and update_settings ~878)**

`configure_homekit_service(&Settings)` calls `system::set_service_enabled` / `system::restart_service`. Old:

```rust
    configure_homekit_service(&current);
```

New:

```rust
    let homekit_settings = current.clone();
    run_blocking(move || configure_homekit_service(&homekit_settings)).await?;
```

(`configure_homekit_service` returns `()`, so `run_blocking(...).await?` just propagates a JoinError as 500.)

- [ ] **Step 9: Build and test**

Run: `cd rust/octocam-web && cargo build && cargo test`
Expected: PASS. Resolve any borrow/`Send` errors by cloning the owned value into the closure (all inputs — `PathBuf`, `String`, `Settings`, `WifiStatus` — are `Clone`).

- [ ] **Step 10: Commit**

```bash
git add rust/octocam-web/src/main.rs
git commit -m "fix(web): run subprocess-heavy helpers on the blocking pool"
```

---

## Task 7: Cross-build, deploy, and verify on the Pi

**Files:** none (build/deploy/verification only).

- [ ] **Step 1: Full local test + clippy**

Run: `cd rust/octocam-web && cargo test && cargo clippy -- -D warnings`
Expected: all tests PASS; no clippy errors.

- [ ] **Step 2: Cross-build the Pi binary**

Run: `scripts/build-pi-web.sh`
Expected: produces `dist/pi/octocam-web` (aarch64). If the script prints the artifact path, note it.

- [ ] **Step 3: Deploy to the Pi**

Run: `OCTOCAM_PI_SSH=root@192.168.2.211 OCTOCAM_SERVICE_USER=root scripts/deploy-pi-web.sh`
Expected: rsync + `systemctl restart octocam-web` succeed. (Root login is available on this Pi.)

- [ ] **Step 4: Smoke test every page over the network**

Run:

```bash
for p in / /wifi /stream-settings /rtsp /homekit /admin /system /logs /terminal /login /api/status; do
  printf "%-16s " "$p"
  curl -s -o /dev/null -m 10 -w "%{http_code} %{time_total}s\n" "http://192.168.2.211:8080$p"
done
```

Expected: every route returns a status (200/redirect), none times out.

- [ ] **Step 5: Concurrency test (proves the reactor no longer stalls)**

Run:

```bash
for i in $(seq 1 40); do
  curl -s -o /dev/null -m 15 -w "%{http_code} %{time_total}s\n" "http://192.168.2.211:8080/system" &
done; wait
```

Expected: all 40 return 200 within a few seconds; no hangs.

- [ ] **Step 6: Confirm no subprocess outlives its timeout**

Run (root):

```bash
ssh root@192.168.2.211 'pid=$(pgrep -x octocam-web); sleep 2; ps --ppid "$pid" -o pid,stat,etime,cmd 2>/dev/null || echo "(no children)"'
```

Expected: `(no children)` shortly after requests settle — no `iw`/`nmcli`/`rpicam` child lingering. Any child present should be < the relevant timeout old.

- [ ] **Step 7: Refresh the preview**

Ensure the `socat` bridge preview (`.claude/launch.json` → `192.168.2.211:8080`) still loads the OctoCam UI; reload if needed.

- [ ] **Step 8: Final commit / branch push (if not already)**

```bash
git status
# If work was done on a branch, push and open a PR per project convention.
```

---

## Notes / Out of Scope

- **`thread::sleep(120ms)` at `system.rs:416`** (double `/proc/stat` read for CPU %) is intentionally kept. After Task 6 it runs on the blocking pool, so it no longer touches worker/reactor threads. Converting it to async would require making the whole `status()` call tree async for no real benefit.
- **`mediamtx.rs`** performs no subprocess calls (config file writer only) — nothing to change.
- **Parallelizing `system::status()`** (it runs ~12 commands sequentially, so a worst-case page under repeated timeouts could take tens of seconds) is a possible future optimization, not part of this fix.
- **Simulating a real `iw` wedge on the Pi** is not reliably reproducible; timeout-and-kill correctness is covered by the `proc` unit tests (`kills_and_errors_on_timeout`), and reactor-liveness by the Task 7 concurrency test.

---

## Self-Review

- **Spec coverage:** Root cause defect (1) no timeout → Tasks 2–5 route every `Command` site through `proc::run` (enumerated: system.rs ×5, wifi.rs ×6, camera.rs ×1, wifi_setup.rs ×3+helpers, main.rs deferred systemctl ×1). Defect (2) blocking on async workers → Task 6 (`spawn_blocking` at all 7 helper families) + Task 5 Step 5 (deferred systemctl). The exact wedge point (`run_output("iw",&["dev"])`) is covered by Task 2 Step 4.
- **Placeholder scan:** Timeout values, full `proc.rs` source, full test code, and concrete old→new edits are provided. The few "unchanged args" notes refer to copying existing argument lists verbatim (the surrounding expression is shown), not deferred design.
- **Type consistency:** `proc::run(&mut Command, Duration) -> io::Result<Output>` is used identically everywhere; `run_blocking<T,F>() -> Result<T, AppError>` matches `AppError(String)`; all closure inputs (`PathBuf`, `String`, `Settings`, `WifiStatus`) are `Clone` (verified) so `spawn_blocking`'s `'static` bound is satisfiable by cloning.
