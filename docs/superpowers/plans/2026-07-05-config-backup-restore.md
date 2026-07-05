# Config Backup & Restore Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an admin download the OctoCam configuration as a JSON file and restore it onto another device via the web UI.

**Architecture:** A `GET /backup` route serializes the 27 portable settings fields plus authorized SSH public keys into a versioned JSON envelope (download attachment). A `POST /restore` route accepts that file as a multipart upload, overlays only the portable fields onto the current on-disk settings (seeding from current so device-bound fields are preserved), re-runs the existing service-reload sequence, and batch-merges the SSH keys. Restore is an admin-gated, post-setup action. No admin password hash and no Wi-Fi credentials ever enter the file.

**Tech Stack:** Rust, axum 0.8, serde/serde_json, askama templates. Existing modules: `settings.rs` (validation), `ssh_keys.rs` (atomic authorized_keys writes), `main.rs` (routes/handlers).

**Spec:** `docs/superpowers/specs/2026-07-05-config-backup-restore-design.md`

## File structure

- `rust/octocam-web/Cargo.toml` — add `"multipart"` to axum features.
- `rust/octocam-web/src/backup.rs` — **new module.** The backup envelope type, the portable/excluded field lists, `build_backup`, `parse_restore`, `backup_filename`, and all pure unit tests. One clear responsibility: envelope (de)serialization + the portable-field overlay policy.
- `rust/octocam-web/src/settings.rs` — no changes needed (its `validate_map`, `enforce_matter_requires_admin`, `Settings` are already `pub` and reused as-is).
- `rust/octocam-web/src/ssh_keys.rs` — add `merge_contents` (pure, testable), `merge` (batched atomic write), and `export_lines` (full normalized lines for backup).
- `rust/octocam-web/src/main.rs` — `mod backup;`; extract `apply_settings_side_effects`; add `backup_download` + `restore_upload` handlers and their routes; add `SystemQuery` + restore-message fields to `SystemTemplate`; wire `system_page`.
- `rust/octocam-web/templates/system.html` — add the "Backup & Restore" panel section.

Note: all `cargo` commands run from `rust/octocam-web`. Cross-compile/deploy to the Pi is a separate step (see memory: build on Mac, rsync to Pi — do not build on the Pi).

---

### Task 1: Enable axum multipart feature

**Files:**
- Modify: `rust/octocam-web/Cargo.toml:8`

- [ ] **Step 1: Add the `multipart` feature**

Change line 8 from:

```toml
axum = { version = "0.8", features = ["form", "json", "macros"] }
```

to:

```toml
axum = { version = "0.8", features = ["form", "json", "macros", "multipart"] }
```

- [ ] **Step 2: Verify it still builds**

Run: `cargo build`
Expected: builds successfully (pulls in axum's multipart support; no code uses it yet).

- [ ] **Step 3: Commit**

```bash
git add rust/octocam-web/Cargo.toml rust/octocam-web/Cargo.lock
git commit -m "build(web): enable axum multipart feature for restore upload"
```

---

### Task 2: Create backup module with field lists and a coverage guard

This task creates `backup.rs` with the single source of truth for which fields are portable vs. excluded, and a test that fails if a future `Settings` field is left unclassified.

**Files:**
- Create: `rust/octocam-web/src/backup.rs`
- Modify: `rust/octocam-web/src/main.rs` (add `mod backup;`)

- [ ] **Step 1: Register the module**

In `rust/octocam-web/src/main.rs`, find the `mod` declarations near the top (e.g. `mod settings;`, `mod ssh_keys;`) and add alphabetically:

```rust
mod backup;
```

- [ ] **Step 2: Write the failing coverage test**

Create `rust/octocam-web/src/backup.rs` with:

```rust
//! OctoCam configuration backup envelope: serialize the portable settings
//! fields + authorized SSH public keys to a versioned JSON file, and restore
//! one by overlaying only the portable fields onto the current device settings.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::settings::{self, Settings};

/// Current backup schema version. Restore rejects a file whose version is
/// greater than this.
pub const BACKUP_VERSION: u32 = 1;

/// The portable settings field names — the single source of truth for what
/// `build_backup` exports and what `parse_restore` overlays. This is an explicit
/// allow-list: a field added to `Settings` later is NOT ported until it is
/// listed here (or added to `EXCLUDED_FIELDS`). `field_lists_cover_all_settings`
/// fails until every field is classified.
pub const PORTABLE_FIELDS: &[&str] = &[
    "device_name",
    "room",
    "camera_label",
    "camera_enabled",
    "resolution_width",
    "resolution_height",
    "framerate",
    "bitrate_kbps",
    "rtsp_enabled",
    "rtsp_max_clients",
    "rtsp_path",
    "sub_stream_enabled",
    "sub_resolution_width",
    "sub_resolution_height",
    "sub_framerate",
    "sub_bitrate_kbps",
    "sub_rtsp_max_clients",
    "sub_rtsp_path",
    "rotation",
    "hflip",
    "vflip",
    "brightness",
    "contrast",
    "homekit_enabled",
    "matter_enabled",
    "motion_enabled",
    "motion_sensitivity",
];

/// Fields deliberately NOT ported — preserved from the target device on restore.
/// `admin_password_hash` is never even written to the file.
pub const EXCLUDED_FIELDS: &[&str] = &[
    "admin_password_hash",
    "setup_complete",
    "homekit_paired",
    "wifi_ssid",
];

/// Serialize a `Settings` to a JSON object map. Infallible in practice —
/// `Settings` is all primitives/strings — but returns an empty map rather than
/// panicking if serialization ever changes shape.
fn settings_map(settings: &Settings) -> Map<String, Value> {
    match serde_json::to_value(settings) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn field_lists_cover_all_settings() {
        let all: BTreeSet<String> = settings_map(&Settings::default())
            .keys()
            .cloned()
            .collect();
        let classified: BTreeSet<String> = PORTABLE_FIELDS
            .iter()
            .chain(EXCLUDED_FIELDS.iter())
            .map(|field| field.to_string())
            .collect();
        assert_eq!(
            all, classified,
            "every Settings field must be in PORTABLE_FIELDS or EXCLUDED_FIELDS"
        );
    }
}
```

- [ ] **Step 3: Run the test to verify it passes (or reveals a gap)**

Run: `cargo test --lib backup::tests::field_lists_cover_all_settings`
Expected: PASS. If it FAILS, the assertion message names the unclassified field(s) — add each to `PORTABLE_FIELDS` or `EXCLUDED_FIELDS` and re-run.

- [ ] **Step 4: Verify the crate still builds**

Run: `cargo build`
Expected: builds (a `dead_code` warning on `settings_map`/`BACKUP_VERSION` is fine — later tasks use them).

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/backup.rs rust/octocam-web/src/main.rs
git commit -m "feat(web): backup module scaffold with portable-field allow-list"
```

---

### Task 3: Implement build_backup and backup_filename

**Files:**
- Modify: `rust/octocam-web/src/backup.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `backup.rs`:

```rust
    #[test]
    fn build_backup_includes_only_portable_fields() {
        let mut settings = Settings::default();
        settings.admin_password_hash = "secret-hash".to_string();
        settings.device_name = "Nursery Cam".to_string();
        settings.homekit_paired = true;
        settings.wifi_ssid = "HomeNet".to_string();

        let backup = build_backup(&settings, 1_751_716_800, vec!["ssh-ed25519 AAAA test".to_string()]);

        assert_eq!(backup.octocam_backup_version, BACKUP_VERSION);
        assert_eq!(backup.exported_at, 1_751_716_800);
        assert_eq!(backup.device_name, "Nursery Cam");
        // Portable field present:
        assert_eq!(backup.settings.get("device_name").and_then(|v| v.as_str()), Some("Nursery Cam"));
        // Excluded fields absent:
        assert!(backup.settings.get("admin_password_hash").is_none());
        assert!(backup.settings.get("homekit_paired").is_none());
        assert!(backup.settings.get("wifi_ssid").is_none());
        assert!(backup.settings.get("setup_complete").is_none());
        assert_eq!(backup.ssh_authorized_keys, vec!["ssh-ed25519 AAAA test".to_string()]);
    }

    #[test]
    fn backup_filename_slugifies_device_name() {
        assert_eq!(
            backup_filename("Nursery Cam!", 1_751_716_800),
            "octocam-backup-nursery-cam-1751716800.json"
        );
        assert_eq!(
            backup_filename("", 42),
            "octocam-backup-octocam-42.json"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib backup::`
Expected: FAIL — `build_backup` and `backup_filename` and `Backup` are not defined.

- [ ] **Step 3: Implement `Backup`, `build_backup`, `backup_filename`**

Add to `backup.rs` (after `settings_map`, before the `tests` module):

```rust
/// The downloadable backup envelope. `settings` holds only the portable fields.
#[derive(Serialize)]
pub struct Backup {
    pub octocam_backup_version: u32,
    /// Unix epoch seconds (see spec: no time crate — an integer needs no dep).
    pub exported_at: u64,
    /// Informational copy of the device name for humans reading the file.
    pub device_name: String,
    pub settings: Map<String, Value>,
    pub ssh_authorized_keys: Vec<String>,
}

/// Build a backup envelope from the current settings and authorized key lines.
pub fn build_backup(settings: &Settings, exported_at: u64, ssh_authorized_keys: Vec<String>) -> Backup {
    let full = settings_map(settings);
    let mut portable = Map::new();
    for &field in PORTABLE_FIELDS {
        if let Some(value) = full.get(field) {
            portable.insert(field.to_string(), value.clone());
        }
    }
    Backup {
        octocam_backup_version: BACKUP_VERSION,
        exported_at,
        device_name: settings.device_name.clone(),
        settings: portable,
        ssh_authorized_keys,
    }
}

/// Download filename: `octocam-backup-<slug>-<epoch>.json`. The device name is
/// slugified (lowercase ascii-alnum, other runs collapse to nothing via the
/// per-char map + trim). Epoch is used in place of a date to avoid a time crate.
pub fn backup_filename(device_name: &str, exported_at: u64) -> String {
    let mapped: String = device_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug: String = mapped
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(40).collect();
    let slug = if slug.is_empty() { "octocam".to_string() } else { slug };
    format!("octocam-backup-{slug}-{exported_at}.json")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib backup::`
Expected: PASS (all backup tests).

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/backup.rs
git commit -m "feat(web): build_backup envelope + slugified filename"
```

---

### Task 4: Implement parse_restore (the seed-from-current overlay)

This is the crux fix from plan-hardening: validate a map seeded from **current** settings, overlaying only portable keys, so excluded fields are preserved and a malicious upload cannot set them.

**Files:**
- Modify: `rust/octocam-web/src/backup.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `backup.rs`:

```rust
    fn envelope(settings_json: Value, keys: Value, version: Value) -> Vec<u8> {
        let mut root = Map::new();
        root.insert("octocam_backup_version".to_string(), version);
        root.insert("settings".to_string(), settings_json);
        root.insert("ssh_authorized_keys".to_string(), keys);
        serde_json::to_vec(&Value::Object(root)).unwrap()
    }

    #[test]
    fn parse_restore_overlays_portable_and_preserves_excluded() {
        let mut current = Settings::default();
        current.admin_password_hash = "keep-me".to_string();
        current.setup_complete = true;
        current.homekit_paired = true;
        current.wifi_ssid = "TargetNet".to_string();
        current.device_name = "Old Name".to_string();

        // Upload sets a portable field AND tries to set excluded fields.
        let mut s = Map::new();
        s.insert("device_name".to_string(), Value::from("New Name"));
        s.insert("framerate".to_string(), Value::from(20));
        s.insert("admin_password_hash".to_string(), Value::from("attacker"));
        s.insert("setup_complete".to_string(), Value::from(false));
        s.insert("homekit_paired".to_string(), Value::from(false));
        s.insert("wifi_ssid".to_string(), Value::from("AttackerNet"));

        let bytes = envelope(Value::Object(s), Value::Array(vec![]), Value::from(1));
        let (restored, keys) = parse_restore(&bytes, &current).expect("valid backup");

        // Portable fields applied:
        assert_eq!(restored.device_name, "New Name");
        assert_eq!(restored.framerate, 20);
        // Excluded fields preserved from `current`, NOT taken from upload:
        assert_eq!(restored.admin_password_hash, "keep-me");
        assert!(restored.setup_complete);
        assert!(restored.homekit_paired);
        assert_eq!(restored.wifi_ssid, "TargetNet");
        assert!(keys.is_empty());
    }

    #[test]
    fn parse_restore_clamps_out_of_range_values() {
        let current = Settings::default();
        let mut s = Map::new();
        s.insert("framerate".to_string(), Value::from(999));
        let bytes = envelope(Value::Object(s), Value::Array(vec![]), Value::from(1));
        let (restored, _keys) = parse_restore(&bytes, &current).unwrap();
        assert_eq!(restored.framerate, 60); // validate_map clamps to max
    }

    #[test]
    fn parse_restore_reads_ssh_keys_array() {
        let current = Settings::default();
        let keys = Value::Array(vec![Value::from("ssh-ed25519 AAAA a"), Value::from("ssh-rsa BBBB b")]);
        let bytes = envelope(Value::Object(Map::new()), keys, Value::from(1));
        let (_restored, keys) = parse_restore(&bytes, &current).unwrap();
        assert_eq!(keys, vec!["ssh-ed25519 AAAA a".to_string(), "ssh-rsa BBBB b".to_string()]);
    }

    #[test]
    fn parse_restore_rejects_bad_version_and_shape() {
        let current = Settings::default();
        // Version too new:
        let bytes = envelope(Value::Object(Map::new()), Value::Array(vec![]), Value::from(2));
        assert!(matches!(parse_restore(&bytes, &current), Err(RestoreError::BadVersion)));
        // Missing version:
        let mut root = Map::new();
        root.insert("settings".to_string(), Value::Object(Map::new()));
        let bytes = serde_json::to_vec(&Value::Object(root)).unwrap();
        assert!(matches!(parse_restore(&bytes, &current), Err(RestoreError::BadVersion)));
        // Not an object at all:
        let bytes = serde_json::to_vec(&Value::from("nope")).unwrap();
        assert!(matches!(parse_restore(&bytes, &current), Err(RestoreError::BadJson)));
        // Not valid JSON:
        assert!(matches!(parse_restore(b"{not json", &current), Err(RestoreError::BadJson)));
    }

    #[test]
    fn parse_restore_matter_off_when_no_admin_password() {
        // Empty admin hash on target -> matter forced off even if upload asks for it.
        let current = Settings::default(); // admin_password_hash empty
        let mut s = Map::new();
        s.insert("matter_enabled".to_string(), Value::from(true));
        let bytes = envelope(Value::Object(s), Value::Array(vec![]), Value::from(1));
        let (restored, _keys) = parse_restore(&bytes, &current).unwrap();
        assert!(!restored.matter_enabled);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib backup::`
Expected: FAIL — `parse_restore` and `RestoreError` are not defined.

- [ ] **Step 3: Implement `RestoreError` and `parse_restore`**

Add to `backup.rs` (after `backup_filename`, before `tests`):

```rust
/// Why a restore upload was rejected. Coarse on purpose — the handler maps this
/// to a redirect query param; detailed causes are not surfaced to the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreError {
    /// Not valid JSON, not a JSON object, or `settings` is not an object.
    BadJson,
    /// `octocam_backup_version` is missing or newer than `BACKUP_VERSION`.
    BadVersion,
}

/// Parse and validate an uploaded backup file's bytes against the current
/// on-disk settings.
///
/// Returns the settings to save (portable fields overlaid on `current`, all
/// clamped/sanitized by `validate_map`, with `enforce_matter_requires_admin`
/// applied) and the raw SSH key strings to merge.
///
/// Crucially: the map handed to `validate_map` is seeded from `current` and only
/// the portable keys are overlaid from the upload. `validate_map` starts from
/// `Settings::default()` for any absent field, so validating the upload alone
/// would reset the excluded fields (empty admin hash, etc.). Seeding from current
/// keeps the excluded fields correct and makes them unreachable from the upload.
pub fn parse_restore(bytes: &[u8], current: &Settings) -> Result<(Settings, Vec<String>), RestoreError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| RestoreError::BadJson)?;
    let Value::Object(root) = value else {
        return Err(RestoreError::BadJson);
    };

    match root.get("octocam_backup_version").and_then(Value::as_u64) {
        Some(version) if version <= BACKUP_VERSION as u64 => {}
        _ => return Err(RestoreError::BadVersion),
    }

    let Some(Value::Object(uploaded)) = root.get("settings") else {
        return Err(RestoreError::BadJson);
    };

    // Seed from current settings, then overlay ONLY the portable keys.
    let mut seed = settings_map(current);
    for &field in PORTABLE_FIELDS {
        if let Some(value) = uploaded.get(field) {
            seed.insert(field.to_string(), value.clone());
        }
    }

    let mut restored = settings::validate_map(&seed);
    settings::enforce_matter_requires_admin(&mut restored);

    let keys = match root.get("ssh_authorized_keys") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };

    Ok((restored, keys))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib backup::`
Expected: PASS (all backup tests).

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/backup.rs
git commit -m "feat(web): parse_restore overlays portable fields onto current settings"
```

---

### Task 5: Add batched SSH key merge and export_lines to ssh_keys

`add()` does ~4 sudo round-trips per key and has nondeterministic partial-failure in a loop. Add a single-read/single-write batch merge, plus a helper that returns full normalized key lines for backup (the display `AuthorizedKey` only stores a truncated preview).

**Files:**
- Modify: `rust/octocam-web/src/ssh_keys.rs`

- [ ] **Step 1: Write the failing tests for the pure merge**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `ssh_keys.rs` (it already imports `use super::*;` and defines `ED25519_BODY`):

```rust
    #[test]
    fn merge_contents_appends_new_and_skips_duplicates_and_invalid() {
        let existing = format!("ssh-ed25519 {ED25519_BODY} existing@host\n");
        // Second real ed25519 vector (distinct from ED25519_BODY).
        let second = "AAAAC3NzaC1lZDI1NTE5AAAAIN8Xr3z6h1r2n8kQ0m4Yy9dT7pXvL2cQwE5rT6yU8iO";
        let candidates = vec![
            format!("ssh-ed25519 {ED25519_BODY} dup@host"), // duplicate fingerprint -> skip
            format!("ssh-ed25519 {second} new@host"),        // new -> add
            "not a key".to_string(),                          // invalid -> skip
            format!("ssh-ed25519 {second} again@host"),      // duplicate within batch -> skip
        ];
        let (contents, added, skipped) = merge_contents(&existing, &candidates);
        assert_eq!(added, 1);
        assert_eq!(skipped, 3);
        // Both keys now present, existing preserved:
        assert_eq!(parse_authorized_keys(&contents).len(), 2);
        assert!(contents.ends_with('\n'));
    }

    #[test]
    fn merge_contents_into_empty_file() {
        let (contents, added, skipped) =
            merge_contents("", &[format!("ssh-ed25519 {ED25519_BODY} a@b")]);
        assert_eq!(added, 1);
        assert_eq!(skipped, 0);
        assert_eq!(parse_authorized_keys(&contents).len(), 1);
    }
```

Note: if the `second` body above is not valid base64 on your machine, generate a real one with `ssh-keygen -t ed25519 -f /tmp/k -N ''` then read the body field from `/tmp/k.pub`; substitute it into `second`. The test only needs a decodable body distinct from `ED25519_BODY`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ssh_keys::tests::merge_contents`
Expected: FAIL — `merge_contents` is not defined.

- [ ] **Step 3: Implement `merge_contents`, `merge`, and `export_lines`**

Add to `ssh_keys.rs`. First add the import at the top of the file (near the other `use` lines):

```rust
use std::collections::HashSet;
```

Then add these functions (place `merge_contents` and `merge` after `add`, and `export_lines` after `list`):

```rust
/// Pure merge: given the current file contents and candidate key strings, return
/// the new file contents plus (added, skipped) counts. Skips invalid keys and
/// any whose fingerprint already exists (in the file or earlier in the batch).
/// No I/O — unit-testable.
fn merge_contents(raw: &str, candidates: &[String]) -> (String, usize, usize) {
    let mut seen: HashSet<String> = parse_authorized_keys(raw)
        .into_iter()
        .map(|key| key.fingerprint)
        .collect();
    let mut contents = raw.to_string();
    let (mut added, mut skipped) = (0usize, 0usize);
    for candidate in candidates {
        let Ok(line) = validate_new_key(candidate) else {
            skipped += 1;
            continue;
        };
        let fp = line.split_whitespace().nth(1).and_then(fingerprint);
        match fp {
            Some(fp) if seen.insert(fp) => {
                if !contents.is_empty() && !contents.ends_with('\n') {
                    contents.push('\n');
                }
                contents.push_str(&line);
                contents.push('\n');
                added += 1;
            }
            _ => skipped += 1,
        }
    }
    (contents, added, skipped)
}

/// Validate and merge a batch of candidate public keys into `authorized_keys` in
/// ONE atomic write (one `read_raw` + one `write_raw`), deduping by fingerprint.
/// Returns (added, skipped). Never removes existing keys. If nothing new is
/// added, no write occurs.
pub fn merge(state_dir: &Path, candidates: &[String]) -> Result<(usize, usize), KeyError> {
    let raw = read_raw()?;
    let existing_count = parse_authorized_keys(&raw).len();
    let (contents, added, skipped) = merge_contents(&raw, candidates);
    if added == 0 {
        return Ok((0, skipped));
    }
    if contents.len() > MAX_FILE_LEN {
        return Err(KeyError::TooLong);
    }
    write_raw(state_dir, &contents, existing_count + added, false)?;
    Ok((added, skipped))
}

/// Full normalized key lines for backup export (type + body + optional comment).
/// Reuses `validate_new_key` to normalize and drop options-prefixed/unknown
/// lines. Fails closed like `read_raw`.
pub fn export_lines() -> Result<Vec<String>, KeyError> {
    let raw = read_raw()?;
    Ok(raw.lines().filter_map(|line| validate_new_key(line).ok()).collect())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib ssh_keys::`
Expected: PASS (existing ssh_keys tests plus the two new `merge_contents` tests).

- [ ] **Step 5: Commit**

```bash
git add rust/octocam-web/src/ssh_keys.rs
git commit -m "feat(web): batched ssh_keys::merge + export_lines for backup"
```

---

### Task 6: Extract apply_settings_side_effects shared helper

Pull the three post-save reload calls out of `update_settings` so `restore` can reuse the exact same sequence. Nothing else moves.

**Files:**
- Modify: `rust/octocam-web/src/main.rs:1266-1273` (the tail of `update_settings`)
- Add: a new `apply_settings_side_effects` function

- [ ] **Step 1: Add the helper**

Add this function in `main.rs` near `configure_homekit_service` (around line 1516):

```rust
/// Reconfigure the downstream services from the current settings: mediamtx RTSP,
/// the HomeKit accessory daemon, and the Matter sidecar. Shared by
/// `update_settings` and `restore_upload` so the two paths cannot drift. Assumes
/// settings have already been persisted with `save_settings`.
async fn apply_settings_side_effects(state: &Arc<AppState>, settings: &Settings) -> Result<(), AppError> {
    let _ = mediamtx::configure_rtsp_service(settings, &state.mediamtx_config_path);
    let homekit_settings = settings.clone();
    run_blocking(move || configure_homekit_service(&homekit_settings)).await?;
    let matter_settings = settings.clone();
    let (matter_env, matter_id) = (state.matter_env_path.clone(), state.matter_identity_path.clone());
    run_blocking(move || matter::configure_matter_service(&matter_settings, &matter_env, &matter_id)).await?;
    Ok(())
}
```

- [ ] **Step 2: Replace the inline tail of `update_settings`**

In `update_settings`, replace these lines (currently at `main.rs:1268-1273`):

```rust
    let _ = mediamtx::configure_rtsp_service(&current, &state.mediamtx_config_path);
    let homekit_settings = current.clone();
    run_blocking(move || configure_homekit_service(&homekit_settings)).await?;
    let matter_settings = current.clone();
    let (matter_env, matter_id) = (state.matter_env_path.clone(), state.matter_identity_path.clone());
    run_blocking(move || matter::configure_matter_service(&matter_settings, &matter_env, &matter_id)).await?;
```

with:

```rust
    apply_settings_side_effects(&state, &current).await?;
```

(Leave everything above — hashing, `setup_complete` copy, `enforce_matter_requires_admin`, `merge_settings`, `save_settings` — unchanged, and leave the redirect that follows unchanged.)

- [ ] **Step 3: Verify build and that settings save still works**

Run: `cargo build && cargo test --lib`
Expected: builds; all existing tests pass (this is a pure refactor — no behavior change).

- [ ] **Step 4: Commit**

```bash
git add rust/octocam-web/src/main.rs
git commit -m "refactor(web): extract apply_settings_side_effects shared by update/restore"
```

---

### Task 7: Add GET /backup download handler and route

**Files:**
- Modify: `rust/octocam-web/src/main.rs` (imports, handler, route)

- [ ] **Step 1: Ensure required imports exist**

Confirm the top of `main.rs` imports the following (add any that are missing):

```rust
use axum::http::{header, HeaderValue, StatusCode};
```

(`header`, `HeaderValue`, and `StatusCode` are already used by `service_worker`/`AppError`; only add what is absent. `Redirect`, `State`, `HeaderMap`, `Uri` are already imported.)

- [ ] **Step 2: Add the `backup_download` handler**

Add near the other page handlers in `main.rs` (e.g. just after `system_page`):

```rust
async fn backup_download(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    let settings = settings::load_settings(&state.config_path);
    // Pre-setup lockout: never expose config before the device has an admin
    // password (require_admin_login is a no-op while the hash is empty).
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }

    // SSH keys are best-effort: a read failure must not block the settings backup.
    let ssh_keys = run_blocking(ssh_keys::export_lines)
        .await?
        .unwrap_or_default();

    let exported_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let backup = backup::build_backup(&settings, exported_at, ssh_keys);
    let body = serde_json::to_string_pretty(&backup).map_err(|error| AppError(error.to_string()))?;
    let filename = backup::backup_filename(&settings.device_name, exported_at);

    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}
```

- [ ] **Step 3: Register the route**

In the `Router::new()` chain (around `main.rs:400`), add after the `/system` route:

```rust
        .route("/backup", get(backup_download))
```

- [ ] **Step 4: Verify build**

Run: `cargo build`
Expected: builds successfully.

- [ ] **Step 5: Manual smoke check**

Run (from `rust/octocam-web`, in a dev shell with a test config):

```bash
OCTOCAM_CONFIG_PATH=/tmp/octocam-test/settings.json cargo run
```

Then in another terminal, after completing setup so `setup_complete=true` (or hand-edit `/tmp/octocam-test/settings.json` to set it plus an admin hash and log in), request the backup with a session cookie. Expected: a JSON body with `octocam_backup_version: 1`, a `settings` object containing `device_name` but NOT `admin_password_hash`, and a `Content-Disposition: attachment` header. Stop the server.

- [ ] **Step 6: Commit**

```bash
git add rust/octocam-web/src/main.rs
git commit -m "feat(web): GET /backup downloads config as JSON attachment"
```

---

### Task 8: Add POST /restore upload handler and route

**Files:**
- Modify: `rust/octocam-web/src/main.rs` (imports, handler, route with body limit)

- [ ] **Step 1: Add multipart imports**

Add to the axum imports in `main.rs`:

```rust
use axum::extract::{DefaultBodyLimit, Multipart};
```

(If `axum::extract::{...}` is already imported for `State`/`Query`/`Form`, merge `DefaultBodyLimit` and `Multipart` into that existing brace group instead of adding a duplicate line.)

- [ ] **Step 2: Add the `restore_upload` handler**

Add after `backup_download` in `main.rs`:

```rust
/// Cap the restore upload well under the global body limit — a settings + keys
/// envelope is a few KB; 256 KB is generous and bounds memory.
const MAX_RESTORE_BYTES: usize = 256 * 1024;

async fn restore_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    mut multipart: Multipart,
) -> AppResult {
    let current = settings::load_settings(&state.config_path);
    if !current.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    // Restore can inject root SSH keys — match the ssh_keys handlers' CSRF guard,
    // which update_settings does not have.
    if cross_origin(&headers) {
        return Ok(Redirect::to("/system?restore=csrf").into_response());
    }

    // Read the first uploaded field's bytes. The route-scoped DefaultBodyLimit
    // (see route registration) rejects an oversize body before we get here.
    let mut file_bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError(error.to_string()))?
    {
        let data = field
            .bytes()
            .await
            .map_err(|error| AppError(error.to_string()))?;
        file_bytes = Some(data.to_vec());
        break;
    }

    let Some(bytes) = file_bytes else {
        return Ok(Redirect::to("/system?restore=empty").into_response());
    };
    if bytes.len() > MAX_RESTORE_BYTES {
        return Ok(Redirect::to("/system?restore=too_large").into_response());
    }

    let (restored, keys) = match backup::parse_restore(&bytes, &current) {
        Ok(result) => result,
        Err(_) => return Ok(Redirect::to("/system?restore=invalid").into_response()),
    };

    settings::save_settings(&state.config_path, &restored)
        .map_err(|error| AppError(error.to_string()))?;
    apply_settings_side_effects(&state, &restored).await?;

    // Best-effort key merge; a key-write failure does not roll back the settings
    // (both are individually atomic and settings are already committed).
    let state_dir = ssh_keys_state_dir(&state);
    let redirect = match run_blocking(move || ssh_keys::merge(&state_dir, &keys)).await? {
        Ok((added, _skipped)) => format!("/system?restore=ok&keys={added}"),
        Err(_) => "/system?restore=ok_keys_failed".to_string(),
    };
    Ok(Redirect::to(&redirect).into_response())
}
```

- [ ] **Step 3: Register the route with a scoped body limit**

In the `Router::new()` chain, add after the `/backup` route:

```rust
        .route(
            "/restore",
            post(restore_upload).layer(DefaultBodyLimit::max(MAX_RESTORE_BYTES)),
        )
```

- [ ] **Step 4: Verify build**

Run: `cargo build && cargo test --lib`
Expected: builds; all tests pass.

- [ ] **Step 5: Manual smoke check**

With the dev server running and a valid session (as in Task 7 step 5), download a backup via `/backup`, then upload it back to `/restore` as a multipart form field. Expected: redirect to `/system?restore=ok&keys=<n>`; `/tmp/octocam-test/settings.json` unchanged in its excluded fields (admin hash, setup_complete, wifi_ssid). Then upload a garbage file — expected redirect to `/system?restore=invalid` with the config unchanged.

- [ ] **Step 6: Commit**

```bash
git add rust/octocam-web/src/main.rs
git commit -m "feat(web): POST /restore imports backup (pre-setup lockout, CSRF guard, 256KB cap)"
```

---

### Task 9: Backup & Restore UI on the system page

**Files:**
- Modify: `rust/octocam-web/src/main.rs` (SystemTemplate fields, SystemQuery, system_page)
- Modify: `rust/octocam-web/templates/system.html`

- [ ] **Step 1: Add `SystemQuery` and extend `SystemTemplate`**

In `main.rs`, add a query struct near the other query structs (e.g. beside `SshKeysQuery`):

```rust
#[derive(Deserialize)]
struct SystemQuery {
    restore: Option<String>,
    keys: Option<String>,
}
```

Then extend `SystemTemplate` (currently `main.rs:200-206`) to add three fields:

```rust
#[derive(Template)]
#[template(path = "system.html")]
struct SystemTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    active_page: &'static str,
    restore_message: String,
    has_restore_message: bool,
    restore_is_error: bool,
}
```

- [ ] **Step 2: Map the query to a message in `system_page`**

Replace the body of `system_page` (currently `main.rs:733-751`) with this version (adds `Query` extraction and message mapping; the auth/setup guards are unchanged):

```rust
async fn system_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SystemQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    let (restore_message, restore_is_error) = match query.restore.as_deref() {
        Some("ok") => {
            let added = query.keys.as_deref().unwrap_or("0");
            (format!("Configuration restored. {added} SSH key(s) added."), false)
        }
        Some("ok_keys_failed") => (
            "Configuration restored, but SSH keys could not be written.".to_string(),
            true,
        ),
        Some("invalid") => ("That file is not a valid OctoCam backup.".to_string(), true),
        Some("too_large") => ("That backup file is too large.".to_string(), true),
        Some("empty") => ("No backup file was uploaded.".to_string(), true),
        Some("csrf") => ("Restore blocked: request came from another origin.".to_string(), true),
        _ => (String::new(), false),
    };
    render(SystemTemplate {
        page_title: "System info".to_string(),
        settings,
        system: system::view(&status),
        active_page: "system",
        has_restore_message: !restore_message.is_empty(),
        restore_message,
        restore_is_error,
    })
}
```

- [ ] **Step 3: Add the UI section to `system.html`**

In `templates/system.html`, add this block immediately after the closing `</dl>` of the main `status-list` (before the `{% if system.wifi_details.len() > 0 %}` block), so it sits inside the existing `<section class="panel settings-card">`... Actually place it as a new sibling panel: insert it right after the closing `</section>` of the first `panel settings-card` and before the closing `</section>` of `content-stack`. Concretely, find the tail:

```html
          </section>
        </section>
      </div>
```

and change it to:

```html
          </section>

          <section class="panel settings-card">
            <div class="section-heading"><h2>Backup &amp; Restore</h2></div>
            {% if has_restore_message %}
              <p class="settings-toast {% if restore_is_error %}is-error{% endif %}">{{ restore_message }}</p>
            {% endif %}
            <p class="field-hint">
              Download this camera's configuration as a JSON file, or restore a
              backup onto this device. Restore does <strong>not</strong> change the
              admin password, Wi-Fi, or existing HomeKit/Matter pairings; SSH keys
              in the backup are added to the existing set.
            </p>
            <p>
              <a class="button" href="/backup">Download backup</a>
            </p>
            <form method="post" action="/restore" enctype="multipart/form-data"
                  onsubmit="return confirm('Restore configuration from this file? Current stream, image, and feature settings will be overwritten.');">
              <label class="field-label" for="backup-file">Restore from backup file</label>
              <input id="backup-file" type="file" name="backup" accept="application/json,.json" required>
              <button type="submit">Restore</button>
            </form>
          </section>
        </section>
      </div>
```

Note: reuse whatever button/field classes the codebase already uses. Check `templates/ssh_keys.html` and `templates/admin.html` for the exact class names (`button`, `field-hint`, `field-label`, `settings-toast`) and match them; if a class differs, use the codebase's actual name rather than inventing one.

- [ ] **Step 4: Verify build (askama compiles templates)**

Run: `cargo build`
Expected: builds. Askama validates the template against `SystemTemplate` at compile time — a field name typo fails here.

- [ ] **Step 5: Manual visual check**

Run the dev server (Task 7 step 5), log in, visit `/system`. Expected: a "Backup & Restore" panel with a Download button and a file-upload restore form. Click Download → a JSON file downloads. Upload it back → redirect to `/system` showing "Configuration restored. N SSH key(s) added." Stop the server.

- [ ] **Step 6: Commit**

```bash
git add rust/octocam-web/src/main.rs rust/octocam-web/templates/system.html
git commit -m "feat(web): Backup & Restore UI on the system page"
```

---

### Task 10: Full verification pass

**Files:** none (verification only)

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: all tests pass, including the new `backup::` and `ssh_keys::` tests.

- [ ] **Step 2: Lint**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any that appear (common: unused import if `add` is no longer used anywhere — it still is, via `/ssh-keys/add`, so leave it).

- [ ] **Step 3: Release build (host)**

Run: `cargo build --release`
Expected: builds clean.

- [ ] **Step 4: Verification checklist from plan-hardening**

Confirm each by inspection/manual test (these are the "verify before shipping" items):
- **Pre-setup lockout:** with `setup_complete=false` in the config, `GET /backup` and `POST /restore` both redirect to `/setup` (never serve/apply). Test by editing the test config.
- **CSRF guard:** `POST /restore` with an `Origin` header from a different host redirects to `/system?restore=csrf`.
- **Excluded-field preservation:** restore a backup whose `settings` object was hand-edited to include `admin_password_hash`, `setup_complete:false`, `wifi_ssid` — confirm the on-disk config keeps the target's original values for all four (covered by `parse_restore_overlays_portable_and_preserves_excluded`, but verify end-to-end once).
- **Path-collision (accepted risk):** note only — a hand-edited backup with `rtsp_path == sub_rtsp_path` is not rejected; this is documented in the spec as accepted for hand-edited files.
- **Encoder-boundary round trip:** back up a config at 1920×1080, confirm restore keeps it legal (validate_map clamps only if it exceeds the limit).

- [ ] **Step 5: Deploy to the Pi (per project memory: build on Mac, rsync — do NOT build on the Pi)**

Follow the existing cross-build + rsync deploy flow used for octocam-web (see `scripts/` and prior deploy commits). Then on the Pi, exercise `/backup` and `/restore` against a real device once. Do not run `cargo build` on the Pi Zero 2 W.

- [ ] **Step 6: Final commit / changelog**

If the project keeps a CHANGELOG (`CHANGELOG.md`), add an entry:

```bash
git add CHANGELOG.md
git commit -m "docs: changelog entry for config backup & restore"
```

---

## Self-review notes

- **Spec coverage:** envelope format (Task 3), portable allow-list + coverage guard (Task 2), seed-from-current restore (Task 4), batched ssh merge + export_lines (Task 5), shared reload helper (Task 6), `GET /backup` (Task 7), `POST /restore` with pre-setup lockout + CSRF + 256 KB cap (Task 8), UI (Task 9), all spec tests (Tasks 2–5) + verification checklist (Task 10). Every spec section maps to a task.
- **Type consistency:** `Backup`, `RestoreError`, `parse_restore`, `build_backup`, `backup_filename`, `merge`, `merge_contents`, `export_lines`, `apply_settings_side_effects`, `SystemQuery`, and the three new `SystemTemplate` fields are defined once and referenced with the same names/signatures throughout.
- **Known human step:** Task 5 and Task 9 ask the implementer to confirm a real ed25519 test vector and the actual CSS class names from existing templates — these depend on machine/codebase specifics that can't be hardcoded blind.
