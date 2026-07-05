# Config Backup & Restore — Design

**Date:** 2026-07-05
**Status:** Approved for planning
**Primary use case:** Device migration (move config to a replacement Pi)

## Summary

Let an admin download the OctoCam configuration as a JSON file and restore it
onto another device. Restore is an **authenticated, post-setup action**: the new
Pi is flashed, boots into the normal first-boot setup flow (admin password +
Wi-Fi), and only then does the user upload a backup to import portable
preferences and SSH keys.

This keeps the feature small — it reuses the existing settings validator
(`validate_map`), the service-reload path from `update_settings`, and the SSH
key plumbing — and it avoids two traps: leaking secrets into a downloadable file,
and re-creating device-bound state (pairing, auth) that can't be meaningfully
transplanted.

## Scope decisions

| Decision | Choice | Rationale |
|---|---|---|
| Use case | Device migration | Most demanding case; forces portable-vs-device-bound split. |
| Admin password hash | **Excluded** from backup | Keeps the file credential-free; consistent with "hashes never leave the settings API". New device sets its own password during setup. |
| Wi-Fi credentials | **Excluded** | PSK must be recoverable (can't be hashed) → plaintext in a downloadable file breaks the "no plaintext passwords" invariant. Also infeasible without granting octocam-web privileged read of NetworkManager secrets. Re-entered via setup flow. |
| SSH public keys | **Included** | Public keys are not secret. Portable and useful on migration. |
| Restore timing | Post-setup, admin-gated (Approach A) | Sidesteps the empty-hash auth hole; reuses existing reload path. Not folded into first-boot setup. |
| SSH key restore semantics | **Merge (union, dedupe by fingerprint)** | Additive; consistent with the existing "never truncate / never lock out" guarantee. |

### Non-goals

- No Wi-Fi credential export/import.
- No admin credential export/import.
- No backup of HomeKit/Matter pairing state (device-bound; re-pair on the new device).
- No integration with the first-boot setup wizard.
- No scheduled/automatic backups, no cloud storage — download only.
- No encryption of the backup file (it contains no secrets by design).

## Backup file format

Filename: `octocam-backup-<device_name>-<YYYY-MM-DD>.json` (device name slugified).

```json
{
  "octocam_backup_version": 1,
  "exported_at": 1751716800,
  "device_name": "Nursery Cam",
  "settings": {
    "device_name": "Nursery Cam",
    "room": "Nursery",
    "camera_label": "Nursery Cam",
    "camera_enabled": true,
    "resolution_width": 1280,
    "resolution_height": 720,
    "framerate": 15,
    "bitrate_kbps": 2500,
    "rtsp_enabled": true,
    "rtsp_max_clients": 1,
    "rtsp_path": "main",
    "sub_stream_enabled": true,
    "sub_resolution_width": 640,
    "sub_resolution_height": 480,
    "sub_framerate": 10,
    "sub_bitrate_kbps": 600,
    "sub_rtsp_max_clients": 2,
    "sub_rtsp_path": "sub",
    "rotation": 0,
    "hflip": false,
    "vflip": false,
    "brightness": 0,
    "contrast": 1.0,
    "homekit_enabled": false,
    "matter_enabled": false,
    "motion_enabled": false,
    "motion_sensitivity": 50
  },
  "ssh_authorized_keys": [
    "ssh-ed25519 AAAA... alice@laptop"
  ]
}
```

- `exported_at` is a **Unix epoch seconds integer** (`SystemTime::now()
  .duration_since(UNIX_EPOCH)`), informational only. Deliberately not RFC 3339:
  the workspace has no `chrono`/`time` crate (only `tokio`'s `time` feature for
  `Duration`/`sleep`), and the README says avoid growing the dependency tree. An
  epoch integer needs no new crate.
- `device_name` at the top level is informational (for humans reading the file);
  the authoritative value is inside `settings`.

### Portable settings fields (exported + applied on restore)

These 27 fields form an **explicit allow-list** (not "everything except the
excluded set"). Both export and restore iterate this named list. Consequence: a
field added to `Settings` later is **not** ported until someone deliberately adds
it here — so a future device-bound field (e.g. a paired-hub id) can't silently
leak into a backup. A unit test asserts every `Settings` field is either in this
allow-list or in the excluded set below, so a new field forces a conscious choice.

`device_name`, `room`, `camera_label`, `camera_enabled`,
`resolution_width`, `resolution_height`, `framerate`, `bitrate_kbps`,
`rtsp_enabled`, `rtsp_max_clients`, `rtsp_path`,
`sub_stream_enabled`, `sub_resolution_width`, `sub_resolution_height`,
`sub_framerate`, `sub_bitrate_kbps`, `sub_rtsp_max_clients`, `sub_rtsp_path`,
`rotation`, `hflip`, `vflip`, `brightness`, `contrast`,
`homekit_enabled`, `matter_enabled`, `motion_enabled`, `motion_sensitivity`.

### Excluded fields — preserved from the target device on restore

- `admin_password_hash` — never exported; target's current value kept.
- `setup_complete` — target's current value kept (must stay `true` on a set-up
  device; must never be forced `true` where the hash is empty).
- `homekit_paired` — target's current value kept (pairing is device-bound).
- `wifi_ssid` — target's current value kept (reflects the device's own network).

`homekit_enabled` / `matter_enabled` ARE portable (they are "run the daemon"
preferences). They are safe to restore as-is: the daemons advertise as unpaired
and the user pairs/commissions on the new device. `enforce_matter_requires_admin`
still applies after restore.

## Endpoints

### Auth gating — must not rely on `require_admin_login` alone

`require_admin_login` returns `Ok(None)` (i.e. "proceed, no login required")
whenever `admin_password_hash` is empty **or** `!setup_complete`
(`main.rs:1543`). That is fine for `/settings` today, but restore can inject
**root SSH keys** — a much larger blast radius — so it must not be reachable in
the pre-setup / no-password window.

Both `/backup` and `/restore` therefore do two checks, in order:

1. **Reject when `!setup_complete`** (return 403 / redirect to setup),
   independent of `require_admin_login`. This is what actually makes "restore is
   a post-setup action" true.
2. Then the normal `require_admin_login` guard.

Additionally, `POST /restore` performs the `cross_origin(&headers)` check that
`ssh_keys_add` / `ssh_keys_revoke` already do (`main.rs:910`, `:932`) and that
`update_settings` does *not* — because restore touches the same root-key surface
those handlers protect. Do not model restore's protection on `update_settings`.

### Cargo change (new)

`POST /restore` needs multipart parsing. The current `axum` features are
`["form","json","macros"]` — add **`"multipart"`**. No existing handler uses
`Multipart`, so this is a new extraction pattern, not a drop-in reuse.

### `GET /backup`

- Builds the envelope from current settings (the 27 portable fields only) +
  `ssh_keys::list()`.
- `exported_at` = current Unix epoch seconds.
- Returns `application/json` with
  `Content-Disposition: attachment; filename="octocam-backup-<device>-<date>.json"`.
- If SSH keys can't be read (e.g. `sudo -n` unavailable), the backup still
  succeeds with an empty `ssh_authorized_keys` array — the settings backup is the
  primary payload and must not be blocked by key-read failure. (The UI notes this.)

### `POST /restore`

Multipart file upload. Steps:

1. Read the uploaded field with a **bounded read** — cap at 256 KB and reject
   larger (a settings + keys envelope is a few KB). `axum`'s `DefaultBodyLimit`
   is a blunt global cap, so enforce the per-route limit explicitly while reading
   `Multipart::next_field`.
2. Parse JSON. Reject if not an object, if `octocam_backup_version` is missing,
   or if it is greater than the supported version (`1`). Unknown extra keys are
   ignored (forward-compatible reads within a version).
3. **Build the map to validate by seeding from current, not from the upload.**
   This is the crux, and it mirrors what `update_settings` already does at
   `main.rs:1245`: start from `settings_to_map(&load_settings(...))` (the current
   on-disk settings), then overlay **only the 27 portable keys** taken from the
   backup's `settings` object on top. Then call `settings::validate_map` **once**
   on the merged map.

   Rationale — do NOT `validate_map(uploaded_settings)` in isolation:
   `validate_map` starts from `Settings::default()` and treats any absent field
   as its default, not "keep current" (`settings.rs:247`). Since the backup omits
   the excluded fields, validating the upload alone would set
   `admin_password_hash → ""`, `setup_complete → false`, `homekit_paired → false`,
   `wifi_ssid → ""` — silently clobbering exactly the fields we promised to
   preserve, and tripping `enforce_matter_requires_admin` via the emptied hash.
   Seeding from current means the excluded fields are already correct and are
   never sourced from the upload.
4. Run `enforce_matter_requires_admin` on the validated result, then
   `save_settings`.
5. Run the shared service-reload sequence (mediamtx / HomeKit / Matter) — see
   refactor below.
6. Merge SSH keys via a **single batch call** — see SSH key merge below. Result
   is `(added, skipped_invalid)`.
7. Redirect back to the system page with a result summary (settings applied,
   N keys added, M skipped).

Malformed input, wrong version, or oversize files produce a user-facing error
and change nothing.

### SSH key merge — one batched write, not a loop

Do **not** call `ssh_keys::add()` per key. `add()` re-reads the file and does
~4 `sudo` round-trips each call (`ssh_keys.rs:274`), so N keys = 4N sudo spawns
under one blocking permit, with nondeterministic partial-failure (a mid-loop
`sudo` failure leaves some keys written and the rest silently dropped).

Add a batch entry point `ssh_keys::merge(state_dir, &[String]) -> Result<(added,
skipped), KeyError>` that: reads once (`read_raw`), validates each candidate via
`validate_new_key`, computes the deduped union against existing keys by
fingerprint, and does one atomic `write_raw`. This preserves the existing
"atomic swap, never truncate" guarantee and makes key restore all-or-nothing.
A key-write failure is surfaced but does not roll back the already-applied
settings (settings and keys are each individually atomic; `save_settings` has
already committed by this point).

## Refactor: shared apply/reload helper

`update_settings`, after saving, calls in sequence:
`mediamtx::configure_rtsp_service`, `configure_homekit_service` (via
`run_blocking`), `matter::configure_matter_service` (via `run_blocking`)
(`main.rs:1266-1271`). Restore needs the identical sequence.

Extract **only those three post-save reload calls** into one helper (e.g.
`apply_settings_side_effects(state, &settings)`), called from both
`update_settings` and `restore`, so the two paths cannot drift.

Keep out of the helper (caller-specific, order-sensitive, pre-save): password
hashing/confirmation, `validated.setup_complete = current.setup_complete`,
`enforce_matter_requires_admin`, `merge_settings`, and `save_settings` itself.
The helper takes an already-saved `&Settings` and the three paths it reads from
`AppState`, and does nothing but the reloads.

## UI

Add a **"Backup & Restore"** section to the `/advanced` (system) page:

- **Download backup** — link/button to `GET /backup`.
- **Restore from backup** — file input + submit posting multipart to `/restore`,
  behind a confirmation step (restore overwrites current stream/image/feature
  settings). Copy notes what restore does NOT change: admin password, Wi-Fi,
  and existing pairings, and that SSH keys are added (not replaced).

## Testing

Unit tests (Rust, alongside `settings.rs` / a new backup module):

- **Round trip:** build backup from settings → restore onto a fresh `Settings`
  → all portable fields equal the source.
- **Preservation:** restore does not change `admin_password_hash`,
  `setup_complete`, `homekit_paired`, or `wifi_ssid` on the target.
- **Sanitization:** a backup with out-of-range values (e.g. `framerate: 999`,
  oversize resolution) is clamped by `validate_map` on restore.
- **Rejection:** missing/newer `octocam_backup_version`, non-object JSON, and
  oversize payloads are rejected without mutating state.
- **SSH merge:** duplicate keys dedupe by fingerprint; invalid keys are skipped
  and counted; existing keys are never removed; the merge is a single batched
  write.
- **Matter guard:** restoring `matter_enabled: true` with an empty admin hash on
  the target leaves Matter disabled (`enforce_matter_requires_admin`).
- **Field-coverage guard:** a test asserts every `Settings` field name is in
  either the portable allow-list or the excluded set — a new field fails the
  test until it is classified.
- **Encoder boundary round trip:** a source resolution at/above the 1920×1080
  encoder limit round-trips as the clamped fallback preset, not the original
  (documents that `clamp_to_encoder_limits` snaps both axes).
- **Pre-setup lockout:** `/backup` and `/restore` are rejected when
  `!setup_complete`, even with no admin password set.
- **Path-collision note (not a hard test):** a hand-edited backup where
  `rtsp_path == sub_rtsp_path` is not rejected by `validate_map` (no cross-field
  uniqueness check). Accepted risk for hand-edited files; documented, not
  guarded. Normal full backups carry a consistent pair from the source.

## Files touched (anticipated)

- `rust/octocam-web/Cargo.toml` — add `"multipart"` to the `axum` features.
- `rust/octocam-web/src/settings.rs` — the 27-field portable allow-list; helper
  to overlay portable keys onto a current-seeded map; the backup envelope
  (de)serialization (or a new `backup.rs` module); field-coverage guard test.
- `rust/octocam-web/src/main.rs` — `/backup` + `/restore` routes and handlers
  (pre-setup lockout + `require_admin_login` + `cross_origin` on `/restore`;
  bounded 256 KB multipart read); extract `apply_settings_side_effects` (three
  reload calls only) shared with `update_settings`.
- `rust/octocam-web/src/ssh_keys.rs` — add `merge(state_dir, &[String]) ->
  Result<(added, skipped), KeyError>` (one read, dedupe by fingerprint, one
  atomic write), reusing `read_raw`/`validate_new_key`/`fingerprint`/`write_raw`.
- System page template under `static/` (or wherever `/advanced` renders) — the
  Backup & Restore section.
