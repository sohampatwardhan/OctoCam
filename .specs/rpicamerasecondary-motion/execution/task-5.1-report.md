# Task 5.1 Report: Cross-build, deploy, confirm device unaffected by default

**Status:** verified — deployed, healthy, inert by default (R6.2)

## Actions taken

1. Cross-built the release binary via `scripts/build-pi-web.sh` (Docker, `aarch64-unknown-linux-gnu`).
2. Deployed via `scripts/deploy-pi-web.sh --skip-build` — this script already backs up the
   previous binary (`/usr/local/bin/octocam-web.bak`), installs the new one, restarts the
   service, and runs its own health-check-and-automatic-rollback loop (polls `/` and `/login`
   for up to ~30s; rolls back to the backed-up binary on failure). Health check reported
   **`health check OK`** — no rollback triggered.

## Verification

- `md5sum /usr/local/bin/octocam-web` confirms the deployed binary is the one just built.
- `journalctl -u octocam-web` after restart shows `Motion session 1 starting against RTSP
  source: rtsp://127.0.0.1:8554/main` — direct log evidence that `resolve_motion_source`
  resolved to `Main` by default on this device, exactly as R6.2 requires (the toggle has no
  stored value yet — a pre-existing settings file predating this field — so it correctly
  defaults to `false` via the same overlay mechanism verified in task 1.2's unit tests).
  Motion continued operating normally (state-change events logged, first frame after 7.4s,
  consistent with the known cold-start range).
- `systemctl is-active octocam-web` / `octocam-rtsp` / `nginx`: all `active`.

## Known limitation discovered during this step (not a blocker)

`check_and_write_mediamtx_version()`'s cache file
(`/var/lib/octocam/mediamtx-version-check.json`) did not yet exist immediately after this
deploy's restart. Root cause: `main.rs`'s startup reconcile block (lines ~174-213) restarts
`octocam-rtsp` via a **separate, narrower path** (`write_mediamtx_config` +
`write_timezone_dropin` + `system::restart_service` called directly) that bypasses
`configure_rtsp_service()` entirely — the version-check call added in task 2.3 only fires
there. [03_design.md](../03_design.md)'s stated reasoning ("a mediamtx binary upgrade requires
a service restart, which only happens through this same path") is therefore not quite accurate
for this codebase — the boot-time reconcile is a second such path.

**Why this doesn't affect correctness:** `motion_use_secondary_stream` can only ever become
`true` via a settings save (there is no other way to set it), and that same save triggers
`configure_rtsp_service()`, refreshing the cache at exactly the moment it would matter. The gap
only means that if an operator upgrades mediamtx **outside** octocam-web (as task 2.1 just did
manually) and then reboots without touching any setting, the cache stays stale until the next
settings save — which keeps motion on `main` slightly longer than strictly necessary. This is
the safe direction (R3.2's fail-closed default), not a correctness violation of any numbered
requirement. Flagged as a minor follow-up (refresh the version check in the boot-time reconcile
block too), not applied here to avoid touching boot-critical code during a production deploy
for a latency-only improvement.

## Result

Feature is live on the device, inert by default. No behavior change until an operator
explicitly enables `motion_use_secondary_stream` in Settings.
