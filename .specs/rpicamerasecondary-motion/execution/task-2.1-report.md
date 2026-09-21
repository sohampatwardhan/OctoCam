# Task 2.1 Report: Upgrade device mediamtx binary to >= v1.20.1

**Status:** verified

## Actions taken

1. Backed up the installed binary: `/usr/local/bin/mediamtx.v1.19.2.bak` (62,164,043 bytes,
   matches the original exactly).
2. Downloaded `mediamtx_v1.20.1_linux_arm64.tar.gz` directly on the device from the official
   GitHub release, extracted to `/tmp/mediamtx-upgrade/`, and confirmed the extracted binary
   reports `v1.20.1` via `--version` **before** touching the installed one.
3. Stopped `octocam-rtsp`, swapped the binary at `/usr/local/bin/mediamtx`, restarted.
4. Confirmed: service `active`; `mediamtx --version` now reports `v1.20.1`; `main` path came
   back online (`ready: true`, hardware H264 encoder, existing RTSP reader reconnected
   automatically) against the **unmodified** existing `/etc/mediamtx.yml` — this step changed
   only the binary, not the configuration.
5. Removed the temporary download directory.

## Evidence

- `journalctl -u octocam-rtsp` shows a clean stop/start cycle: `MediaMTX v1.20.1, linux, arm64`,
  `[path main] [RPI Camera source] started`, `using hardware H264 encoder`, and the existing
  reader session recreated within ~2 seconds of restart.
- `/v3/paths/list` shows `main` `available: true`, `online: true`, one active RTSP reader.
  `sub` shows `available: false` only because it's the on-demand software transcoder with no
  reader currently attached — expected, unrelated to this change.

## Result

The device's `mediamtx` now satisfies `MIN_SECONDARY_SAFE_VERSION` (`>= v1.20.1`), unblocking
task 3.1's restart test. The pre-upgrade binary remains at
`/usr/local/bin/mediamtx.v1.19.2.bak` for rollback if needed (not expected — no anomalies
observed).
