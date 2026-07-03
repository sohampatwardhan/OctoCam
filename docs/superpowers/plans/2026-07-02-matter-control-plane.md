# Matter Camera Control Plane Implementation Plan (Plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement everything octocam-web needs to orchestrate a Matter 1.5 camera daemon — settings, identity + onboarding codes, daemon config, reader reservation, viewer classification, internal snapshot listener, `/matter` UI, and the sandboxed systemd unit — per the hardened spec at `docs/superpowers/specs/2026-07-02-matter-camera-design.md`.

**Architecture:** A new `matter.rs` module in octocam-web (pattern: `mediamtx.rs`) renders an env file consumed by a new `octocam-matter.service` systemd unit, computes Matter onboarding payloads (QR + 11-digit manual code) locally, and reads a status JSON the daemon writes. The daemon binary itself (patched CHIP camera-app) is Plan 2 (`2026-07-02-matter-chip-fork.md`); this plan defines its CLI/env/status-file **contract** so both sides meet in the middle.

**Tech Stack:** Rust (axum 0.8, askama 0.12, serde), one new crate (`qrcode`, svg-only), systemd, bash.

**Working directory for all commands:** `/Users/soham/GitRepos/OctoCam` (cargo commands run in `rust/octocam-web/`).

**Contract with the Plan-2 daemon (fixed here, consumed there):**

| Item | Value |
| --- | --- |
| Env file | `/var/lib/octocam/matter-env` (root 0600, read by systemd) |
| Env keys | `OCTOCAM_MATTER_DISCRIMINATOR`, `OCTOCAM_MATTER_PASSCODE`, `OCTOCAM_MATTER_VENDOR_ID`, `OCTOCAM_MATTER_PRODUCT_ID`, `OCTOCAM_MATTER_RTSP_URL`, `OCTOCAM_MATTER_SNAPSHOT_URL` |
| KVS | `/var/lib/octocam/matter-storage/kvs` |
| Status file (daemon writes) | `/var/lib/octocam/matter-storage/status.json` with keys `status` (string), `commissioned` (bool), `fabric_count` (u32), `stream_state` (string), `error` (string) |
| Snapshot endpoint | `GET http://127.0.0.1:8081/internal/snapshot.jpg` — 200 image/jpeg, 409 camera disabled, 503 capture failed; may take up to 8s cold |
| Identity | VID `0xFFF1` (65521), PID `0x8001` (32769), passcode 8 digits, discriminator 12-bit |

---

### Task 1: `matter_enabled` setting + admin-password gate

**Files:**
- Modify: `rust/octocam-web/src/settings.rs`

- [ ] **Step 1: Write the failing tests** (append inside `mod tests` in `settings.rs`)

```rust
#[test]
fn matter_defaults_off_and_parses() {
    assert!(!Settings::default().matter_enabled);
    let mut map = Map::new();
    map.insert("matter_enabled".into(), Value::String("true".into()));
    map.insert("admin_password_hash".into(), Value::String("x".into()));
    assert!(validate_map(&map).matter_enabled);
}

#[test]
fn matter_requires_admin_password() {
    let mut map = Map::new();
    map.insert("matter_enabled".into(), Value::String("true".into()));
    let mut s = validate_map(&map);
    assert!(s.admin_password_hash.is_empty());
    enforce_matter_requires_admin(&mut s);
    assert!(!s.matter_enabled, "matter must not enable without an admin password");
    s.admin_password_hash = "hash".into();
    s.matter_enabled = true;
    enforce_matter_requires_admin(&mut s);
    assert!(s.matter_enabled);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml matter -- --nocapture`
Expected: compile error — `matter_enabled` field and `enforce_matter_requires_admin` do not exist.

- [ ] **Step 3: Implement**

In the `Settings` struct, after `homekit_paired: bool,`:

```rust
    pub matter_enabled: bool,
```

In `impl Default for Settings`, after `homekit_paired: false,`:

```rust
            matter_enabled: false,
```

In `validate_map`, after the `homekit_paired` line:

```rust
    settings.matter_enabled = bool_value(&map, "matter_enabled", settings.matter_enabled);
```

New public function after `validate_map` (before `clamp_to_encoder_limits`):

```rust
/// The Matter pairing QR is a durable commission-this-camera credential, and
/// require_admin_login() is a no-op while the admin password hash is empty —
/// so an empty hash must force Matter off (spec: "octocam-web integration").
pub fn enforce_matter_requires_admin(settings: &mut Settings) {
    if settings.admin_password_hash.is_empty() {
        settings.matter_enabled = false;
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml`
Expected: all pass (existing + 2 new).

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/settings.rs
git commit -m "feat(web): add matter_enabled setting gated on admin password"
```

---

### Task 2: mediamtx — additive reserve + service forced on for daemons

**Files:**
- Modify: `rust/octocam-web/src/mediamtx.rs`

- [ ] **Step 1: Write the failing tests** (append inside `mod tests`)

```rust
#[test]
fn matter_reserve_adds_one_reader() {
    let mut settings = Settings { matter_enabled: false, ..Default::default() };
    let without = render_mediamtx_config(&settings);
    settings.matter_enabled = true;
    let with = render_mediamtx_config(&settings);
    let max_readers = |content: &str| -> Vec<i32> {
        content.lines()
            .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
            .map(|v| v.parse().unwrap())
            .collect()
    };
    for (x, y) in max_readers(&without).iter().zip(max_readers(&with).iter()) {
        assert_eq!(y - x, 1, "matter reserve must add exactly one reader per path");
    }
}

#[test]
fn homekit_and_matter_reserves_are_additive() {
    let base = Settings { homekit_enabled: false, matter_enabled: false, ..Default::default() };
    let both = Settings { homekit_enabled: true, matter_enabled: true, ..base.clone() };
    let first_max = |content: &str| -> i32 {
        content.lines()
            .find_map(|l| l.trim().strip_prefix("maxReaders: "))
            .unwrap().parse().unwrap()
    };
    assert_eq!(
        first_max(&render_mediamtx_config(&both)) - first_max(&render_mediamtx_config(&base)),
        2
    );
}

#[test]
fn rtsp_service_runs_whenever_a_daemon_needs_it() {
    // rtsp_enabled=false must NOT stop mediamtx while a daemon consumes it —
    // the daemons' only video source is this unit (hardening FIX-1).
    let mut s = Settings { rtsp_enabled: false, homekit_enabled: false, matter_enabled: false, ..Default::default() };
    assert!(!rtsp_service_should_run(&s));
    s.matter_enabled = true;
    assert!(rtsp_service_should_run(&s));
    s.matter_enabled = false;
    s.homekit_enabled = true;
    assert!(rtsp_service_should_run(&s));
    s = Settings { rtsp_enabled: true, ..Default::default() };
    assert!(rtsp_service_should_run(&s));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml mediamtx`
Expected: compile error — `rtsp_service_should_run` missing; reserve test fails.

- [ ] **Step 3: Implement**

Replace the `reserve` line in `render_mediamtx_config`:

```rust
    // Each enabled local daemon (HomeKit, Matter) reads via its own local RTSP
    // session, so reserve one slot per daemon per path — user-facing capacity
    // must not shrink when a bridge is watching. Soft reservation: see the spec.
    let reserve = i32::from(settings.homekit_enabled) + i32::from(settings.matter_enabled);
```

New public function after `configure_rtsp_service`:

```rust
/// mediamtx must keep running while any local daemon consumes it, even when the
/// user turns LAN RTSP exposure off — rtsp_enabled=false used to stop the unit,
/// permanently killing the daemons' only video source.
pub fn rtsp_service_should_run(settings: &Settings) -> bool {
    settings.rtsp_enabled || settings.homekit_enabled || settings.matter_enabled
}
```

In `configure_rtsp_service`, change the `set_service_enabled` call:

```rust
    let service = match system::set_service_enabled("octocam-rtsp", rtsp_service_should_run(settings)) {
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/mediamtx.rs
git commit -m "feat(web): additive daemon reader reserve; keep mediamtx up for daemons"
```

---

### Task 3: streams — distinguish Matter from HomeKit local readers

**Files:**
- Modify: `rust/octocam-web/src/streams.rs`

- [ ] **Step 1: Write the failing tests** (append inside `mod tests`; also update the two existing `PathViewers { .. }` literal assertions to include `matter: 0`)

```rust
#[test]
fn local_reader_attributed_to_matter_when_only_matter_enabled() {
    let paths = r#"{"items":[{"name":"main","readers":[{"type":"rtspSession","id":"r2"}]},{"name":"sub","readers":[]}]}"#;
    let report = classify(paths, SESSIONS, "main", "sub", 1, 2, false, true).unwrap();
    assert_eq!(report.main.matter, 1);
    assert_eq!(report.main.homekit, 0);
    assert!(report.main_available(), "a Matter reader must not consume user capacity");
}

#[test]
fn two_local_readers_split_between_daemons_when_both_enabled() {
    let paths = r#"{"items":[{"name":"sub","readers":[{"type":"rtspSession","id":"r2"},{"type":"rtspSession","id":"r3"}]},{"name":"main","readers":[]}]}"#;
    let sessions = r#"{"items":[
        {"id":"r2","remoteAddr":"127.0.0.1:44064"},
        {"id":"r3","remoteAddr":"127.0.0.1:44100"}
    ]}"#;
    let report = classify(paths, sessions, "main", "sub", 1, 2, true, true).unwrap();
    assert_eq!(report.sub.homekit, 1);
    assert_eq!(report.sub.matter, 1);
}

#[test]
fn attribute_local_rules() {
    assert_eq!(attribute_local(3, true, false), (3, 0));
    assert_eq!(attribute_local(3, false, true), (0, 3));
    assert_eq!(attribute_local(1, true, true), (1, 0)); // ambiguous single: homekit, stable
    assert_eq!(attribute_local(2, true, true), (1, 1));
    assert_eq!(attribute_local(3, true, true), (2, 1)); // transient snapshot ffmpeg rides homekit bucket
    assert_eq!(attribute_local(2, false, false), (2, 0)); // legacy fallback
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml streams`
Expected: compile errors — `matter` field, new `classify` arity, `attribute_local` missing.

- [ ] **Step 3: Implement**

Add to `PathViewers` after `pub homekit: u32,`:

```rust
    pub matter: u32,
```

Change `classify` signature and local-reader handling. Full replacement of the relevant parts:

```rust
fn classify(
    paths_json: &str,
    sessions_json: &str,
    main_path: &str,
    sub_path: &str,
    main_cap: u32,
    sub_cap: u32,
    homekit_enabled: bool,
    matter_enabled: bool,
) -> Option<ViewerReport> {
```

Inside the reader loop, replace the `"rtspSession" | "rtspsSession"` arm body to count locals into a per-path temporary instead of `target.homekit` directly. Concretely: before the `for item in paths…` loop add `let mut local_counts = [0u32; 2];` won't work with the `target` borrow pattern — instead add a `local: u32` field approach: give `PathViewers` a private accumulation via a local variable per path. Simplest correct structure: count into `target.homekit` as today, then post-process. Replace the arm with:

```rust
                "rtspSession" | "rtspsSession" => {
                    if local_session.get(id).copied().unwrap_or(false) {
                        // Temporarily accumulate ALL local daemon readers here;
                        // attributed between homekit/matter after the loop.
                        target.homekit += 1;
                    } else {
                        target.rtsp += 1;
                    }
                }
```

After the loop (before `Some(report)`):

```rust
    for path in [&mut report.main, &mut report.sub] {
        let (homekit, matter) = attribute_local(path.homekit, homekit_enabled, matter_enabled);
        path.homekit = homekit;
        path.matter = matter;
    }
```

New function after `classify`:

```rust
/// mediamtx's session list exposes only remoteAddr, so two loopback daemons are
/// indistinguishable at the protocol level. Each daemon holds at most one
/// persistent reader per path; the transient snapshot ffmpeg also shows as
/// local. Attribution: single-daemon setups get everything; with both enabled,
/// matter is credited one reader once a second local reader exists, and any
/// extras (snapshot capture) ride the homekit bucket. Both buckets are excluded
/// from capacity math, so mis-attribution can never mark a path full.
fn attribute_local(local: u32, homekit_enabled: bool, matter_enabled: bool) -> (u32, u32) {
    if !matter_enabled {
        return (local, 0);
    }
    if !homekit_enabled {
        return (0, local);
    }
    let matter = u32::from(local >= 2);
    (local - matter, matter)
}
```

Update `viewer_report` to pass the flags:

```rust
    classify(
        &paths,
        &sessions,
        &settings.rtsp_path,
        &settings.sub_rtsp_path,
        settings.rtsp_max_clients.max(0) as u32,
        settings.sub_rtsp_max_clients.max(0) as u32,
        settings.homekit_enabled,
        settings.matter_enabled,
    )
```

Update existing tests: every `classify(` call gains `, true, false` (preserves old homekit attribution semantics), and the two `PathViewers { … }` struct literals gain `matter: 0,`.

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/streams.rs
git commit -m "feat(web): attribute local readers between HomeKit and Matter daemons"
```

---

### Task 4: matter.rs — identity (generate/load/rotate, 0600)

**Files:**
- Create: `rust/octocam-web/src/matter.rs`
- Modify: `rust/octocam-web/src/main.rs` (add `mod matter;` after `mod mediamtx;`)

- [ ] **Step 1: Create `matter.rs` with types + failing tests**

```rust
use crate::settings::Settings;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path, path::PathBuf};

pub const VENDOR_ID: u16 = 0xFFF1; // CSA test VID; not shippable as a product
pub const PRODUCT_ID: u16 = 0x8001;

/// Matter spec 5.1.7.1: these passcodes are invalid and must never be used.
const INVALID_PASSCODES: [u32; 12] = [
    0, 11111111, 22222222, 33333333, 44444444, 55555555, 66666666, 77777777,
    88888888, 99999999, 12345678, 87654321,
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MatterIdentity {
    pub passcode: u32,      // 27-bit, 1..=99999998, excluding INVALID_PASSCODES
    pub discriminator: u16, // 12-bit
    pub vendor_id: u16,
    pub product_id: u16,
}

pub fn generate_identity() -> MatterIdentity {
    let mut rng = rand::thread_rng();
    let passcode = loop {
        let candidate = rng.gen_range(1..=99999998u32);
        if !INVALID_PASSCODES.contains(&candidate) {
            break candidate;
        }
    };
    MatterIdentity {
        passcode,
        discriminator: rng.gen_range(0..=4095u16),
        vendor_id: VENDOR_ID,
        product_id: PRODUCT_ID,
    }
}

/// Load the persisted identity, or generate + persist one (file mode 0600 —
/// the passcode is a durable commission-this-camera credential; see the spec's
/// documented deviation from the no-plaintext-secrets model).
pub fn load_or_generate_identity(path: &Path) -> io::Result<MatterIdentity> {
    if let Ok(raw) = fs::read_to_string(path) {
        if let Ok(identity) = serde_json::from_str::<MatterIdentity>(&raw) {
            return Ok(identity);
        }
    }
    let identity = generate_identity();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&identity)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(identity)
}

/// Reset pairing rotates the passcode: delete and regenerate.
pub fn rotate_identity(path: &Path) -> io::Result<MatterIdentity> {
    let _ = fs::remove_file(path);
    load_or_generate_identity(path)
}

pub fn default_identity_path() -> PathBuf {
    std::env::var_os("OCTOCAM_MATTER_IDENTITY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/matter-identity.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_identity_is_in_spec_ranges() {
        for _ in 0..200 {
            let id = generate_identity();
            assert!((1..=99999998).contains(&id.passcode));
            assert!(!INVALID_PASSCODES.contains(&id.passcode));
            assert!(id.discriminator <= 4095);
            assert_eq!(id.vendor_id, 0xFFF1);
            assert_eq!(id.product_id, 0x8001);
        }
    }

    #[test]
    fn identity_persists_and_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("matter-identity.json");
        let first = load_or_generate_identity(&path).unwrap();
        let again = load_or_generate_identity(&path).unwrap();
        assert_eq!(first, again, "identity must be stable across loads");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let rotated = rotate_identity(&path).unwrap();
        assert_ne!(first.passcode, rotated.passcode, "reset must rotate the passcode");
    }
}
```

(The `assert_ne!` on passcode has a ~1e-8 flake chance; acceptable.)

- [ ] **Step 2: Add `mod matter;` to `main.rs`** (after `mod mediamtx;`) and run

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml matter::`
Expected: PASS (module is self-contained; `Settings` import unused yet — remove the `use crate::settings::Settings;` line until Task 6 needs it, or prefix with `#[allow(unused_imports)]` — prefer removing it now and re-adding in Task 6).

- [ ] **Step 3: Commit**

```bash
git add rust/octocam-web/src/matter.rs rust/octocam-web/src/main.rs
git commit -m "feat(web): matter identity generation, persistence, rotation"
```

---

### Task 5: matter.rs — onboarding payload (manual code, QR payload, QR SVG)

**Files:**
- Modify: `rust/octocam-web/src/matter.rs`
- Modify: `rust/octocam-web/Cargo.toml`

- [ ] **Step 1: Add the qrcode dependency**

In `Cargo.toml` `[dependencies]` (alphabetical position, after `rand`):

```toml
qrcode = { version = "0.14", default-features = false, features = ["svg"] }
```

- [ ] **Step 2: Write the failing tests** (append to `matter.rs` tests)

```rust
    /// Known CHIP test vector: discriminator 3840, passcode 20202021.
    /// Digits derived: digit1=3 (short disc 15 >> 2), chunk2=49701, chunk3=1233,
    /// Verhoeff check digit 2 → 34970112332 (matches chip-tool's documented code).
    #[test]
    fn manual_pairing_code_matches_chip_test_vector() {
        assert_eq!(manual_pairing_code(3840, 20202021), "34970112332");
    }

    #[test]
    fn qr_payload_shape_and_roundtrip() {
        let id = MatterIdentity {
            passcode: 20202021,
            discriminator: 3840,
            vendor_id: 0xFFF1,
            product_id: 0x8001,
        };
        let payload = qr_payload(&id);
        assert!(payload.starts_with("MT:"));
        assert_eq!(payload.len(), 3 + 19, "88 bits → 11 bytes → 19 base38 chars");
        let bytes = pack_payload_bits(&id);
        let decoded = base38_decode(&payload[3..]);
        assert_eq!(decoded, bytes, "base38 must round-trip");
        // Field-level checks against the packed bits (LSB-first layout).
        let acc = bytes.iter().rev().fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
        assert_eq!(acc & 0x7, 0, "version");
        assert_eq!((acc >> 3) & 0xFFFF, 0xFFF1, "vid");
        assert_eq!((acc >> 19) & 0xFFFF, 0x8001, "pid");
        assert_eq!((acc >> 35) & 0x3, 0, "custom flow");
        assert_eq!((acc >> 37) & 0xFF, 0x04, "discovery: on-network");
        assert_eq!((acc >> 45) & 0xFFF, 3840, "discriminator");
        assert_eq!((acc >> 57) & 0x7FF_FFFF, 20202021, "passcode");
    }

    #[test]
    fn qr_svg_renders() {
        let id = generate_identity();
        let svg = qr_svg(&qr_payload(&id));
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("</svg>"));
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml matter::`
Expected: compile errors — functions missing.

- [ ] **Step 4: Implement** (add to `matter.rs` above the tests)

```rust
const BASE38_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-.";

/// Pack the onboarding payload bit fields LSB-first into 11 bytes
/// (Matter spec 5.1.3): version(3)=0, vid(16), pid(16), custom-flow(2)=0,
/// discovery-capabilities(8)=0x04 (on-network only), discriminator(12),
/// passcode(27), padding(4)=0.
fn pack_payload_bits(id: &MatterIdentity) -> [u8; 11] {
    let mut acc: u128 = 0;
    let mut shift = 0u32;
    for (value, width) in [
        (0u128, 3u32),
        (u128::from(id.vendor_id), 16),
        (u128::from(id.product_id), 16),
        (0, 2),
        (0x04, 8),
        (u128::from(id.discriminator & 0x0FFF), 12),
        (u128::from(id.passcode & 0x07FF_FFFF), 27),
        (0, 4),
    ] {
        acc |= value << shift;
        shift += width;
    }
    let mut bytes = [0u8; 11];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = ((acc >> (8 * i)) & 0xFF) as u8;
    }
    bytes
}

/// Base38 per Matter spec 5.1.3.1: bytes consumed in groups of 3 (LE u32 → 5
/// chars), a trailing 2-byte group → 4 chars, 1-byte → 2 chars.
fn base38_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let (mut value, chars) = match chunk.len() {
            3 => (u32::from(chunk[0]) | u32::from(chunk[1]) << 8 | u32::from(chunk[2]) << 16, 5),
            2 => (u32::from(chunk[0]) | u32::from(chunk[1]) << 8, 4),
            _ => (u32::from(chunk[0]), 2),
        };
        for _ in 0..chars {
            out.push(BASE38_CHARS[(value % 38) as usize] as char);
            value /= 38;
        }
    }
    out
}

#[cfg(test)]
fn base38_decode(text: &str) -> [u8; 11] {
    let mut bytes = Vec::new();
    let chars: Vec<u32> = text
        .bytes()
        .map(|b| BASE38_CHARS.iter().position(|c| *c == b).unwrap() as u32)
        .collect();
    for group in chars.chunks(5) {
        let value = group.iter().rev().fold(0u32, |acc, c| acc * 38 + c);
        let n = match group.len() {
            5 => 3,
            4 => 2,
            _ => 1,
        };
        for i in 0..n {
            bytes.push(((value >> (8 * i)) & 0xFF) as u8);
        }
    }
    bytes.try_into().unwrap()
}

pub fn qr_payload(id: &MatterIdentity) -> String {
    format!("MT:{}", base38_encode(&pack_payload_bits(id)))
}

/// 11-digit manual pairing code (Matter spec 5.1.4.1, VID/PID not included):
/// digit1 = short-discriminator(4 bits) >> 2; next 5 digits =
/// ((short_disc & 3) << 14) | (passcode & 0x3FFF); next 4 = passcode >> 14;
/// final digit = Verhoeff checksum over the first 10.
pub fn manual_pairing_code(discriminator: u16, passcode: u32) -> String {
    let short_disc = u32::from((discriminator >> 8) & 0xF);
    let digit1 = short_disc >> 2; // VID_PID_PRESENT = 0
    let chunk2 = ((short_disc & 0x3) << 14) | (passcode & 0x3FFF);
    let chunk3 = passcode >> 14;
    let first10 = format!("{digit1}{chunk2:05}{chunk3:04}");
    format!("{first10}{}", verhoeff_digit(&first10))
}

fn verhoeff_digit(digits: &str) -> u32 {
    const D: [[u8; 10]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
        [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
        [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
        [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
        [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
        [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
        [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
        [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
        [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    ];
    const P: [[u8; 10]; 8] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
        [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
        [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
        [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
        [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
        [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
        [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
    ];
    const INV: [u8; 10] = [0, 4, 3, 2, 1, 5, 6, 7, 8, 9];
    let mut c = 0u8;
    for (i, ch) in digits.bytes().rev().enumerate() {
        let digit = ch - b'0';
        c = D[c as usize][P[(i + 1) % 8][digit as usize] as usize];
    }
    u32::from(INV[c as usize])
}

pub fn qr_svg(payload: &str) -> String {
    use qrcode::render::svg;
    match qrcode::QrCode::new(payload.as_bytes()) {
        Ok(code) => code
            .render::<svg::Color>()
            .min_dimensions(180, 180)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
        Err(_) => String::new(),
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml matter::`
Expected: PASS, including the `34970112332` vector. If the vector test fails, the bug is in `manual_pairing_code` or `verhoeff_digit`, not the vector — it is independently derived in the test comment. (The QR string itself gets cross-checked against `chip-tool payload parse-setup-payload` in Plan 2 acceptance.)

- [ ] **Step 6: Commit**

```bash
git add rust/octocam-web/src/matter.rs rust/octocam-web/Cargo.toml rust/octocam-web/Cargo.lock
git commit -m "feat(web): matter onboarding payload — manual code, QR payload, QR SVG"
```

---

### Task 6: matter.rs — daemon env render/write, status read, preflight

**Files:**
- Modify: `rust/octocam-web/src/matter.rs`

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn env_render_selects_sub_stream_and_contains_contract_keys() {
        let id = MatterIdentity { passcode: 20202021, discriminator: 3840, vendor_id: 0xFFF1, product_id: 0x8001 };
        let settings = Settings::default(); // sub_stream_enabled: true
        let env = render_matter_env(&settings, &id);
        assert!(env.contains("OCTOCAM_MATTER_DISCRIMINATOR=3840\n"));
        assert!(env.contains("OCTOCAM_MATTER_PASSCODE=20202021\n"));
        assert!(env.contains("OCTOCAM_MATTER_VENDOR_ID=65521\n"));
        assert!(env.contains("OCTOCAM_MATTER_PRODUCT_ID=32769\n"));
        assert!(env.contains("OCTOCAM_MATTER_RTSP_URL=rtsp://127.0.0.1:8554/sub\n"));
        assert!(env.contains("OCTOCAM_MATTER_SNAPSHOT_URL=http://127.0.0.1:8081/internal/snapshot.jpg\n"));
        let main_only = Settings { sub_stream_enabled: false, ..Settings::default() };
        assert!(render_matter_env(&main_only, &id).contains("OCTOCAM_MATTER_RTSP_URL=rtsp://127.0.0.1:8554/main\n"));
    }

    #[test]
    fn env_write_reports_changes_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("matter-env");
        let id = generate_identity();
        let settings = Settings::default();
        assert!(write_matter_env(&settings, &id, &path).unwrap(), "first write changes");
        assert!(!write_matter_env(&settings, &id, &path).unwrap(), "identical write is a no-op");
        let changed = Settings { sub_stream_enabled: false, ..settings };
        assert!(write_matter_env(&changed, &id, &path).unwrap(), "config change must be detected");
    }

    #[test]
    fn status_parses_and_defaults() {
        let view = status_view(r#"{"status":"running","commissioned":true,"fabric_count":2,"stream_state":"streaming","error":""}"#);
        assert_eq!(view.status, "running");
        assert!(view.commissioned);
        assert_eq!(view.fabric_count, 2);
        let empty = status_view("not json");
        assert_eq!(empty.status, "");
        assert_eq!(empty.fabric_count, 0);
    }

    #[test]
    fn ipv6_preflight_detects_link_local() {
        let with = "fe800000000000001234567890abcdef 03 40 20 80    wlan0\n";
        let without = "20010db8000000000000000000000001 02 40 00 80    eth0\n";
        assert!(ipv6_link_local_present(with));
        assert!(!ipv6_link_local_present(without));
        assert!(!ipv6_link_local_present(""));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml matter::`
Expected: compile errors.

- [ ] **Step 3: Implement** (add to `matter.rs`; re-add `use crate::settings::Settings;` at the top)

```rust
#[derive(Clone, Debug, Default, Deserialize)]
pub struct MatterStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub commissioned: bool,
    #[serde(default)]
    pub fabric_count: u32,
    #[serde(default)]
    pub stream_state: String,
    #[serde(default)]
    pub error: String,
}

pub fn render_matter_env(settings: &Settings, identity: &MatterIdentity) -> String {
    // Mirror the HomeKit daemon's default source preference: sub when enabled
    // (bandwidth-friendly), main otherwise. The daemon is configured at exec;
    // configure_matter_service() restarts it only when this render changes.
    let stream_path = if settings.sub_stream_enabled {
        &settings.sub_rtsp_path
    } else {
        &settings.rtsp_path
    };
    format!(
        "OCTOCAM_MATTER_DISCRIMINATOR={disc}\nOCTOCAM_MATTER_PASSCODE={pass}\nOCTOCAM_MATTER_VENDOR_ID={vid}\nOCTOCAM_MATTER_PRODUCT_ID={pid}\nOCTOCAM_MATTER_RTSP_URL=rtsp://127.0.0.1:8554/{path}\nOCTOCAM_MATTER_SNAPSHOT_URL=http://127.0.0.1:8081/internal/snapshot.jpg\n",
        disc = identity.discriminator,
        pass = identity.passcode,
        vid = identity.vendor_id,
        pid = identity.product_id,
        path = stream_path,
    )
}

/// Writes the daemon env file; Ok(true) when content changed (mirrors
/// write_mediamtx_config so callers restart only on real changes).
pub fn write_matter_env(settings: &Settings, identity: &MatterIdentity, path: &Path) -> Result<bool, String> {
    let next = render_matter_env(settings, identity);
    let current = fs::read_to_string(path).unwrap_or_default();
    if current == next {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, next).map_err(|error| error.to_string())?;
    Ok(true)
}

pub fn read_status(path: &Path) -> MatterStatus {
    fs::read_to_string(path)
        .ok()
        .map(|raw| status_view(&raw))
        .unwrap_or_default()
}

fn status_view(raw: &str) -> MatterStatus {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Matter requires IPv6 (at least link-local). Parses /proc/net/if_inet6
/// content; separated from the read for testability off-Linux.
pub fn ipv6_link_local_present(if_inet6: &str) -> bool {
    if_inet6
        .lines()
        .any(|line| line.trim_start().to_ascii_lowercase().starts_with("fe80"))
}

pub fn ipv6_preflight_ok() -> bool {
    match fs::read_to_string("/proc/net/if_inet6") {
        Ok(content) => ipv6_link_local_present(&content),
        // Non-Linux dev machines: don't block the UI on a missing procfs.
        Err(_) => true,
    }
}

pub fn default_env_path() -> PathBuf {
    std::env::var_os("OCTOCAM_MATTER_ENV_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/matter-env"))
}

pub fn default_status_path() -> PathBuf {
    std::env::var_os("OCTOCAM_MATTER_STATUS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/matter-storage/status.json"))
}

pub fn default_storage_dir() -> PathBuf {
    std::env::var_os("OCTOCAM_MATTER_STORAGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/matter-storage"))
}
```

Add `Deserialize` to the serde import: `use serde::{Deserialize, Serialize};` (already present from Task 4).

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/matter.rs
git commit -m "feat(web): matter daemon env render, status read, IPv6 preflight"
```

---

### Task 7: internal loopback snapshot listener

**Files:**
- Modify: `rust/octocam-web/src/main.rs`

- [ ] **Step 1: Refactor `snapshot()` into a shared core**

Replace the body of `async fn snapshot(...)` (main.rs:1146-1181) with:

```rust
async fn snapshot(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    serve_snapshot(&state).await
}
```

and move the rest into a new function directly below (same logic, unchanged comments):

```rust
/// Shared snapshot core: the authenticated /snapshot.jpg route and the
/// loopback-only internal listener both funnel here, so the camera_enabled
/// gate and the 2s single-flight cache apply identically to both.
async fn serve_snapshot(state: &Arc<AppState>) -> AppResult {
    let settings = settings::load_settings(&state.config_path);
    if !settings.camera_enabled {
        return Ok((
            StatusCode::CONFLICT,
            "Camera is disabled in OctoCam settings.\n",
        )
            .into_response());
    }
    let mut cache = state.snapshot_cache.lock().await;
    if let Some((at, bytes)) = cache.as_ref() {
        if camera::snapshot_is_fresh(*at, std::time::Instant::now()) {
            let bytes = bytes.clone();
            return Ok(([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response());
        }
    }
    // Cold path: hold the lock across capture so concurrent requests coalesce onto
    // one capture (bounded by CAPTURE_TIMEOUT = 8s). Accepted trade-off: a burst of
    // concurrent snapshot requests serializes behind the first — worst case one
    // 8s wait, then everyone is served from cache.
    let settings_for_capture = settings.clone();
    match run_blocking(move || camera::capture_snapshot(&settings_for_capture)).await? {
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
}
```

- [ ] **Step 2: Add the internal listener**

New handler + spawner (place near `spawn_captive_portal_listener`):

```rust
/// Loopback-only endpoint for local daemons (the Matter camera-app fetches
/// snapshots here). Binding a separate 127.0.0.1 listener is the guard —
/// structurally unreachable from the LAN, no header/peer-address parsing —
/// while serve_snapshot keeps the camera_enabled check (hardening FIX-3).
async fn internal_snapshot(State(state): State<Arc<AppState>>) -> AppResult {
    serve_snapshot(&state).await
}

fn spawn_internal_listener(state: Arc<AppState>) {
    tokio::spawn(async move {
        let port = env::var("OCTOCAM_INTERNAL_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(8081);
        let app = Router::new()
            .route("/internal/snapshot.jpg", get(internal_snapshot))
            .with_state(state);
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => {
                let _ = axum::serve(listener, app).await;
            }
            Err(error) => {
                eprintln!("internal listener unavailable (127.0.0.1:{port}): {error}");
            }
        }
    });
}
```

In `async_main`, after the captive-portal block, add:

```rust
    spawn_internal_listener(state.clone());
```

- [ ] **Step 3: Verify**

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml && cargo clippy --manifest-path rust/octocam-web/Cargo.toml -- -D warnings`
Expected: builds clean, tests pass. Manual check: `cargo run` locally then `curl -i http://127.0.0.1:8081/internal/snapshot.jpg` returns 409 or 503 (no Pi camera on the Mac) — proving the route exists and the gate fires; the LAN interface must NOT serve it: `curl -i http://<mac-lan-ip>:8081/` fails to connect.

- [ ] **Step 4: Commit**

```bash
git add rust/octocam-web/src/main.rs
git commit -m "feat(web): loopback-only internal snapshot listener on 127.0.0.1:8081"
```

---

### Task 8: /matter page, template, nav, settings flow, reset action

**Files:**
- Create: `rust/octocam-web/templates/matter.html`
- Modify: `rust/octocam-web/src/main.rs`
- Modify: `rust/octocam-web/src/matter.rs` (view builder + service configure + reset)
- Modify: `rust/octocam-web/templates/_sidebar.html`

- [ ] **Step 1: Add the view builder and service orchestration to `matter.rs`**

```rust
/// Everything matter.html needs, precomputed (askama templates stay logic-free).
#[derive(Clone, Debug)]
pub struct MatterView {
    pub status: String,
    pub commissioned: bool,
    pub fabric_count: u32,
    pub has_fabrics: bool,
    pub orphaned_fabrics: bool, // fabrics persisted while matter_enabled=false
    pub manual_code: String,
    pub qr_svg: String,
    pub qr_payload: String,
    pub stream_source: String,
    pub error: String,
    pub has_error: bool,
    pub ipv6_ok: bool,
    pub admin_password_set: bool,
}

pub fn view(settings: &Settings, identity: Option<&MatterIdentity>, status: &MatterStatus) -> MatterView {
    let status_label = if !status.status.is_empty() {
        status.status.clone()
    } else if settings.matter_enabled {
        "starting".to_string()
    } else {
        "disabled".to_string()
    };
    let (manual_code, qr_svg_text, payload) = match identity {
        Some(id) => {
            let payload = qr_payload(id);
            (
                manual_pairing_code(id.discriminator, id.passcode),
                qr_svg(&payload),
                payload,
            )
        }
        None => (String::new(), String::new(), String::new()),
    };
    MatterView {
        status: status_label,
        commissioned: status.commissioned,
        fabric_count: status.fabric_count,
        has_fabrics: status.fabric_count > 0,
        orphaned_fabrics: status.fabric_count > 0 && !settings.matter_enabled,
        manual_code,
        qr_svg: qr_svg_text,
        qr_payload: payload,
        stream_source: if settings.sub_stream_enabled { "sub" } else { "main" }.to_string(),
        has_error: !status.error.is_empty(),
        error: status.error.clone(),
        ipv6_ok: ipv6_preflight_ok(),
        admin_password_set: !settings.admin_password_hash.is_empty(),
    }
}

/// Enable/disable + reconfigure the daemon. Unlike configure_homekit_service,
/// this restarts ONLY when the rendered config changed — a brightness save must
/// not drop live Matter WebRTC sessions (hardening FIX-10).
pub fn configure_matter_service(settings: &Settings, env_path: &Path, identity_path: &Path) {
    const UNIT: &str = "octocam-matter";
    if !settings.matter_enabled {
        let _ = crate::system::set_service_enabled(UNIT, false);
        return;
    }
    let Ok(identity) = load_or_generate_identity(identity_path) else {
        eprintln!("matter: cannot load or generate identity");
        return;
    };
    let changed = write_matter_env(settings, &identity, env_path).unwrap_or(false);
    let _ = crate::system::set_service_enabled(UNIT, true);
    if changed {
        let _ = crate::system::restart_service(UNIT);
    }
}

/// Reset pairing: stop → wipe KVS → rotate passcode → restart if enabled.
/// Wiping under a live daemon is racy (it holds fabric state in memory and
/// rewrites the KVS), hence the strict ordering.
pub fn reset_pairing(settings: &Settings, storage_dir: &Path, env_path: &Path, identity_path: &Path) {
    const UNIT: &str = "octocam-matter";
    let _ = crate::system::set_service_enabled(UNIT, false);
    let _ = fs::remove_dir_all(storage_dir);
    let _ = fs::create_dir_all(storage_dir);
    if let Ok(identity) = rotate_identity(identity_path) {
        let _ = write_matter_env(settings, &identity, env_path);
    }
    if settings.matter_enabled {
        let _ = crate::system::set_service_enabled(UNIT, true);
        let _ = crate::system::restart_service(UNIT);
    }
}
```

Check `system::set_service_enabled` / `system::restart_service` visibility (used from `mediamtx.rs` already, so they're `pub` within the crate — call as `crate::system::…`).

- [ ] **Step 2: AppState + routes + handlers in `main.rs`**

Add fields to `AppState`:

```rust
    matter_identity_path: PathBuf,
    matter_env_path: PathBuf,
    matter_status_path: PathBuf,
    matter_storage_dir: PathBuf,
```

In `AppState::from_env()`:

```rust
            matter_identity_path: matter::default_identity_path(),
            matter_env_path: matter::default_env_path(),
            matter_status_path: matter::default_status_path(),
            matter_storage_dir: matter::default_storage_dir(),
```

Template struct (next to `HomeKitTemplate`):

```rust
#[derive(Template)]
#[template(path = "matter.html")]
struct MatterTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    matter: matter::MatterView,
    saved: bool,
    active_page: &'static str,
}
```

Routes (after the `/homekit` route):

```rust
        .route("/matter", get(matter_page))
        .route("/matter/reset", post(matter_reset))
```

Handlers (place after `homekit`):

```rust
async fn matter_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    // Identity is only materialized once Matter has been enabled; before that
    // the page shows the enable flow without minting a credential.
    let identity = if settings.matter_enabled {
        matter::load_or_generate_identity(&state.matter_identity_path).ok()
    } else {
        None
    };
    let matter_status = matter::read_status(&state.matter_status_path);
    render(MatterTemplate {
        page_title: "Matter".to_string(),
        saved: query.saved.as_deref() == Some("1"),
        matter: matter::view(&settings, identity.as_ref(), &matter_status),
        settings,
        system: system::view(&status),
        active_page: "matter",
    })
}

async fn matter_reset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    let (storage, env_path, id_path) = (
        state.matter_storage_dir.clone(),
        state.matter_env_path.clone(),
        state.matter_identity_path.clone(),
    );
    run_blocking(move || matter::reset_pairing(&settings, &storage, &env_path, &id_path)).await?;
    Ok(Redirect::to("/matter?saved=1").into_response())
}
```

In `update_settings`, after `validated.setup_complete = current.setup_complete;`:

```rust
    settings::enforce_matter_requires_admin(&mut validated);
```

and after the `configure_homekit_service` call:

```rust
    let matter_settings = current.clone();
    let (matter_env, matter_id) = (state.matter_env_path.clone(), state.matter_identity_path.clone());
    run_blocking(move || matter::configure_matter_service(&matter_settings, &matter_env, &matter_id)).await?;
```

- [ ] **Step 3: Create `templates/matter.html`** (modeled on homekit.html)

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{ settings.device_name }} Matter</title>
    <link rel="stylesheet" href="/static/styles.css?v=20260702-wifi-setup-lucide">
    <script src="/static/app.js?v=20260702-wifi-setup-lucide" defer></script>
  </head>
  <body>
    <main class="shell">
      {% include "_topbar.html" %}
      {% include "_settings_toast.html" %}

      <div class="app-layout">
        {% include "_sidebar.html" %}
        <section class="content-stack">
          <form class="settings-stack" method="post" action="/settings">
            <input type="hidden" name="_return_to" value="/matter">
            <input type="hidden" name="_checkboxes" value="matter_enabled">
            <section class="panel settings-card">
              <div class="section-heading"><h2>Matter</h2></div>
              {% if !matter.admin_password_set %}
                <div class="pairing-box error-pairing"><span>Set an admin password first (Admin page). The Matter pairing code is a durable credential for this camera's feed, and without a password anyone on your network could read it.</span></div>
              {% endif %}
              <div class="toggle-row"><span>Matter camera enabled</span><label class="switch"><input type="checkbox" name="matter_enabled" {% if settings.matter_enabled %}checked{% endif %} {% if !matter.admin_password_set %}disabled{% endif %}><span></span></label></div>
              <dl class="status-list compact-list">
                <div><dt>Accessory</dt><dd>{{ matter.status }}</dd></div>
                <div><dt>Commissioned</dt><dd>{% if matter.commissioned %}yes ({{ matter.fabric_count }} fabric{% if matter.fabric_count != 1 %}s{% endif %}){% else %}not commissioned{% endif %}</dd></div>
                <div><dt>Stream source</dt><dd>{{ matter.stream_source }}</dd></div>
              </dl>
              {% if !matter.ipv6_ok %}
                <div class="pairing-box error-pairing"><span>IPv6 appears disabled on this device. Matter requires IPv6 (link-local at minimum) — commissioning will fail until it is re-enabled.</span></div>
              {% endif %}
              {% if matter.orphaned_fabrics %}
                <div class="pairing-box error-pairing"><span>Matter is disabled but {{ matter.fabric_count }} previously paired ecosystem(s) still hold credentials. Re-enabling restores their access to the camera; use "Reset Matter pairing" to revoke.</span></div>
              {% endif %}
              {% if matter.has_error %}
                <div class="pairing-box error-pairing"><span>{{ matter.error }}</span></div>
              {% endif %}
              {% if settings.matter_enabled %}
                {% if matter.manual_code.len() > 0 %}
                  <div class="homekit-pairing">
                    <div class="homekit-qr">{{ matter.qr_svg|safe }}</div>
                    <div class="homekit-pairing-details">
                      <span>Manual code</span>
                      <strong>{{ matter.manual_code }}</strong>
                      <small>{{ matter.qr_payload }}</small>
                    </div>
                  </div>
                {% endif %}
                <div class="pairing-box compact-pairing"><span>Ecosystem support (July 2026): SmartThings works. Home Assistant is experimental and blocks uncertified devices by default (this camera uses a test vendor ID) — a manual override is required. Alexa commissions but cannot show video yet. Google Home and Apple Home do not support Matter cameras yet.</span></div>
                <div class="pairing-box compact-pairing"><span>Disabling Matter stops the service but does not revoke access: previously paired ecosystems regain the camera feed when re-enabled. Use "Reset Matter pairing" to revoke all pairings.</span></div>
              {% else %}
                <div class="pairing-box compact-pairing"><span>Enable Matter and save to publish this camera to Matter ecosystems (SmartThings, Home Assistant, and others as support rolls out).</span></div>
              {% endif %}
            </section>
            <section class="panel action-card">
              <button class="primary" type="submit">Save Matter settings</button>
            </section>
          </form>
          <form class="settings-stack" method="post" action="/matter/reset">
            <section class="panel action-card">
              <button type="submit">Reset Matter pairing</button>
              <small>Removes all paired ecosystems and rotates the pairing code.</small>
            </section>
          </form>
        </section>
      </div>
    </main>
  </body>
</html>
```

- [ ] **Step 4: Sidebar link** — in `templates/_sidebar.html`, after the HomeKit `</a>` (line 56), add:

```html
    <a class="nav-link {% if active_page == "matter" %}is-active{% endif %}" href="/matter">
      <svg class="nav-icon" aria-hidden="true" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10"></circle>
        <path d="M12 2v10l6.4 6.4"></path>
        <path d="M12 12 5.6 18.4"></path>
      </svg>
      <span>Matter</span>
    </a>
```

- [ ] **Step 5: Verify** (askama compiles templates at build time — a build IS the template test)

Run: `cargo test --manifest-path rust/octocam-web/Cargo.toml && cargo clippy --manifest-path rust/octocam-web/Cargo.toml -- -D warnings`
Expected: clean. Then `cargo run` and load `http://127.0.0.1:8080/matter` (dev mode: no admin hash → page renders with the enable-blocked warning and disabled toggle).

- [ ] **Step 6: Commit**

```bash
git add rust/octocam-web/src/main.rs rust/octocam-web/src/matter.rs rust/octocam-web/templates/matter.html rust/octocam-web/templates/_sidebar.html
git commit -m "feat(web): /matter page — pairing QR, fabric status, reset, admin gate"
```

---

### Task 9: systemd unit + install.sh + deploy script

**Files:**
- Create: `systemd/octocam-matter.service`
- Modify: `install.sh`
- Modify: `scripts/deploy-pi-web.sh`

- [ ] **Step 1: Create `systemd/octocam-matter.service`**

```ini
[Unit]
Description=OctoCam Matter camera accessory (CHIP camera-app)
Wants=network-online.target octocam-rtsp.service avahi-daemon.service
After=network-online.target octocam-rtsp.service octocam-web.service avahi-daemon.service

[Service]
Type=simple
# Deliberately NOT __SERVICE_USER__: this daemon parses untrusted pre-auth
# Matter traffic from the whole LAN on TCP/UDP 5540 and is built from
# example-quality upstream code. Least privilege, always.
User=octocam-matter
Group=octocam-matter
EnvironmentFile=/var/lib/octocam/matter-env
ExecStart=__PROJECT_DIR__/dist/chip-camera-app \
  --discriminator $OCTOCAM_MATTER_DISCRIMINATOR \
  --passcode $OCTOCAM_MATTER_PASSCODE \
  --vendor-id $OCTOCAM_MATTER_VENDOR_ID \
  --product-id $OCTOCAM_MATTER_PRODUCT_ID \
  --secured-device-port 5540 \
  --KVS /var/lib/octocam/matter-storage/kvs \
  --rtsp-source $OCTOCAM_MATTER_RTSP_URL \
  --snapshot-url $OCTOCAM_MATTER_SNAPSHOT_URL \
  --status-file /var/lib/octocam/matter-storage/status.json
Restart=on-failure
RestartSec=5
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/octocam/matter-storage
CapabilityBoundingSet=
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX AF_NETLINK
MemoryMax=150M
OOMScoreAdjust=500
LogRateLimitIntervalSec=30
LogRateLimitBurst=1000

[Install]
WantedBy=multi-user.target
```

(`--rtsp-source`, `--snapshot-url`, `--status-file` are the Plan-2 patch flags — the contract table at the top of this plan. `MemoryMax=150M` is the initial budget, tuned after CHECK-5.)

- [ ] **Step 2: install.sh** — locate the block that installs the homekit unit (`install.sh` ~lines 148-183; it copies `systemd/*.service` with `sed` substitutions for `__PROJECT_DIR__`/`__SERVICE_USER__`, then a `grep '"homekit_enabled": true'` conditional enable). Mirror it exactly for Matter, adding user + storage dir creation before unit install:

```bash
# Matter daemon runs sandboxed as its own user (never root: parses untrusted
# LAN traffic). Storage dir owns the CHIP KVS + status file.
if ! id -u octocam-matter >/dev/null 2>&1; then
  useradd --system --no-create-home --shell /usr/sbin/nologin octocam-matter
fi
install -d -o octocam-matter -g octocam-matter -m 750 /var/lib/octocam/matter-storage
```

and after the homekit enable block:

```bash
if grep -q '"matter_enabled": true' /var/lib/octocam/settings.json 2>/dev/null; then
  systemctl enable octocam-matter.service
fi
```

The unit file gets the same `sed` treatment as the others for `__PROJECT_DIR__` (its `User=` is hardcoded and must NOT be substituted — verify the existing sed only replaces the `__…__` placeholders, which it does by token). Do NOT add GStreamer/avahi apt packages yet — the dependency list is derived empirically in Plan 2 (CHECK-2) before touching the apt line; add a one-line comment in install.sh at the apt install site: `# octocam-matter runtime deps (GStreamer/avahi) land with the Plan-2 daemon; see docs/matter.md`.

- [ ] **Step 3: deploy-pi-web.sh** — this script currently installs only the web + wifi units. Mirror its unit-install lines for `octocam-matter.service` (same sed substitutions), plus the same user/storage-dir creation block from Step 2, so the reference Pi (rsync-only workflow) picks the unit up on next deploy. Do not enable the unit here; enable-state is reconciled from settings by install.sh and toggled at runtime by octocam-web.

- [ ] **Step 4: Verify**

Run: `bash -n install.sh && bash -n scripts/deploy-pi-web.sh && systemd-analyze verify systemd/octocam-matter.service 2>/dev/null || true`
Expected: both `bash -n` pass (syntax). `systemd-analyze` is unavailable on macOS — the unit gets verified on the Pi in Plan 2; on the Mac just confirm the file has no `__SERVICE_USER__` token: `! grep -q __SERVICE_USER__ systemd/octocam-matter.service`.

- [ ] **Step 5: Commit**

```bash
git add systemd/octocam-matter.service install.sh scripts/deploy-pi-web.sh
git commit -m "feat(deploy): sandboxed octocam-matter unit + install/deploy wiring"
```

---

### Task 10: documentation

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Create: `docs/matter.md`

- [ ] **Step 1: README** — replace the paragraph beginning "Matter 1.5 camera support is a future consideration…" with:

```markdown
Matter 1.5 camera support is implemented as an optional sidecar daemon (a
patched build of connectedhomeip's camera-app) that relays the mediamtx H.264
stream over Matter/WebRTC. As of mid-2026 only SmartThings has shipped Matter
camera viewing; Home Assistant support is experimental, and Alexa/Google/Apple
have not shipped it — the /matter page in the web UI shows an honest
per-ecosystem support matrix. See docs/matter.md. Note: disabling mDNS
(scripts/minimize-os.sh --disable-mdns) breaks Matter commissioning.
```

Also grep README for any other "Matter" mentions (e.g. a "revisit later" line) and update them consistently.

- [ ] **Step 2: CHANGELOG** — prepend an entry matching the existing format:

```markdown
- feat(matter): Matter 1.5 camera control plane — matter_enabled setting,
  onboarding QR/manual code generated locally, sandboxed octocam-matter
  systemd unit, loopback snapshot endpoint, additive reader reservation,
  /matter settings page. Daemon binary (patched CHIP camera-app) tracked
  separately; see docs/matter.md.
```

- [ ] **Step 3: Create `docs/matter.md`** with these sections (full prose, not stubs): What it is (architecture diagram from the spec); Ecosystem support matrix (July 2026 table from the spec's Goal section); Commissioning walkthrough (enable on /matter → scan QR / manual code, commissioning window note); Security model (dedicated user + sandbox directives, persisted-passcode deviation + rotation-on-reset, disable ≠ revoke, factory reset guidance incl. flash-block caveat); Daemon contract (the table from this plan's header); Build & deploy (pointer to Plan 2: pinned CHIP SHA + image digest, `linux-arm64-camera-clang`, glibc ≤ 2.36 requirement, never build on the Pi); Open verifications (CHECK-1..9 list from the spec).

- [ ] **Step 4: Commit**

```bash
git add README.md CHANGELOG.md docs/matter.md
git commit -m "docs: Matter camera architecture, security model, ecosystem matrix"
```

---

## Final verification (whole plan)

- [ ] `cargo test --manifest-path rust/octocam-web/Cargo.toml` — all green
- [ ] `cargo clippy --manifest-path rust/octocam-web/Cargo.toml -- -D warnings` — clean
- [ ] `cargo build --manifest-path rust/octocam-web/Cargo.toml --release` — compiles (templates verified)
- [ ] `bash -n install.sh scripts/deploy-pi-web.sh`
- [ ] `! grep -rn "TODO\|TBD" rust/octocam-web/src/matter.rs systemd/octocam-matter.service`

**Explicitly deferred to Plan 2 (`2026-07-02-matter-chip-fork.md`):** the CHIP fork + RTSP-ingest/snapshot/status patches, `scripts/build-matter.sh`, empirical GStreamer/avahi dependency list, on-Pi deployment, CHECK-1..9, and the camera-controller acceptance gate. Nothing in Plan 1 turns the daemon on for real users until that binary exists — `matter_enabled` just orchestrates a unit whose ExecStart binary is absent (systemd reports it failed; the /matter page shows "starting"/error state, which is accurate).
