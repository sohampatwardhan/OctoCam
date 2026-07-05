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
  "exported_at": "2026-07-05T12:00:00Z",
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

- `exported_at` uses the server clock (RFC 3339 UTC), informational only.
- `device_name` at the top level is informational (for humans reading the file);
  the authoritative value is inside `settings`.

### Portable settings fields (exported + applied on restore)

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

Both admin-gated via `require_admin_login` (same guard as `update_settings`).

### `GET /backup`

- Builds the envelope from current settings (portable fields only) + `ssh_keys::list()`.
- Returns `application/json` with
  `Content-Disposition: attachment; filename="octocam-backup-<device>-<date>.json"`.
- If SSH keys can't be read (e.g. `sudo -n` unavailable), the backup still
  succeeds with an empty `ssh_authorized_keys` array — the settings backup is the
  primary payload and must not be blocked by key-read failure. (The UI notes this.)

### `POST /restore`

Multipart file upload. Steps:

1. Read the uploaded file (bounded size, e.g. reject > 256 KB — a settings +
   keys envelope is a few KB).
2. Parse JSON. Reject if not an object, if `octocam_backup_version` is missing,
   or if it is greater than the supported version (`1`). Unknown extra keys are
   ignored (forward-compatible reads within a version).
3. Take the `settings` sub-object and run it through `settings::validate_map`,
   which clamps/sanitizes every field to safe ranges automatically.
4. Load current settings. Overwrite **only the portable fields** from the
   validated result onto current; preserve the excluded fields listed above.
   Run `enforce_matter_requires_admin`.
5. Save via `save_settings`, then run the shared service-reload sequence
   (mediamtx / HomeKit / Matter) — see refactor below.
6. For each entry in `ssh_authorized_keys`, validate via
   `ssh_keys::validate_new_key` and merge into the existing set (union, dedupe by
   fingerprint). Invalid keys are skipped and counted; a key-write failure is
   surfaced but does not roll back the already-applied settings.
7. Redirect back to the system page with a result summary (settings applied,
   N keys added, M skipped).

Malformed input, wrong version, or oversize files produce a user-facing error
and change nothing.

## Refactor: shared apply/reload helper

`update_settings` currently, after saving, calls in sequence:
`mediamtx::configure_rtsp_service`, `configure_homekit_service` (blocking),
`matter::configure_matter_service` (blocking). Restore needs the identical
sequence.

Extract this into one helper (e.g. `apply_settings_side_effects(state, &settings)`)
and call it from both `update_settings` and `restore`, so the two paths cannot
drift.

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
  and counted; existing keys are never removed.
- **Matter guard:** restoring `matter_enabled: true` with an empty admin hash on
  the target leaves Matter disabled (`enforce_matter_requires_admin`).

## Files touched (anticipated)

- `rust/octocam-web/src/settings.rs` — helpers to select portable fields for
  export and to merge portable fields on restore; the backup envelope
  (de)serialization (or a new `backup.rs` module).
- `rust/octocam-web/src/main.rs` — `/backup` + `/restore` routes and handlers;
  extract `apply_settings_side_effects` shared with `update_settings`.
- `rust/octocam-web/src/ssh_keys.rs` — reuse `list`, `validate_new_key`,
  `fingerprint`, and the safe-write path; add a merge entry point if needed.
- System page template under `static/` (or wherever `/advanced` renders) — the
  Backup & Restore section.
