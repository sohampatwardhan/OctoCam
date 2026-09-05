# Task 3.1 Report: On-device restart test of corrected secondary config

**Status:** verified — **PASS**

## Procedure and results

1. Backed up `/etc/mediamtx.yml` (checksum `86f3628d...`).
2. Installed a temporary config matching [03_design.md](../03_design.md) Component 2's exact
   corrected secondary-path shape for `sub`: `rpiCameraSecondary: true`, `rpiCameraCodec: auto`,
   `rpiCameraMJPEGQuality: 60`, at 640x480@10fps — `main` left untouched.
3. **Restarted `octocam-rtsp` 10 consecutive times. 10/10 crash-free** (R3.1's regression bar;
   the original bug crashed 3/3 times). `journalctl` shows a clean start each time; the API's
   `/v3/paths/list` shows both `main` (H264) and `sub` (M-JPEG) `available: true` /
   `online: true` simultaneously, with mediamtx's own log confirming `using hardware H264
   encoder` and `using MJPEG encoder` side by side — no encoder conflict.
4. Started a synthetic persistent `ffmpeg` reader against `sub` (standing in for motion
   detection's real 24/7 access pattern, since that code doesn't land until task 3.2) and
   confirmed it ran steadily (~10 fps, `inboundFramesInError: 0`) for the duration of the test.
5. With that persistent reader attached, opened a **second concurrent reader** against `sub`
   (standing in for Matter/web-UI live view) and successfully captured 5 real JPEG frames —
   confirming the path serves multiple simultaneous readers without degradation (R4.1, R4.2).
6. **MJPEG-quality frame-sanity check:** captured a full-resolution frame at quality 60 and ran
   it through motion's actual `scale=80:60,format=gray` filter. Visual inspection of both the
   640x480 source and the 80x60 grayscale output shows no compression artifacts that would
   degrade motion detection at that resolution — resolves [01_discovery.md](../01_discovery.md)'s
   Open Decision 3 with real evidence; quality 60 is confirmed suitable, not just assumed.
7. **H264-field compatibility check:** `rpiCameraH264Profile`/`rpiCameraIDRPeriod` stayed
   unconditional on the secondary path per task 1.3's implementation. No warning or error
   referencing either field appeared anywhere in the logs across 10 restarts or the sustained
   read session — the uncertain finding from the plan-hardening pass is resolved: **no follow-up
   fix needed**.
8. Stopped the synthetic reader, restored the original `/etc/mediamtx.yml` (checksum verified
   identical to the pre-test backup), restarted once more, and confirmed the device matches its
   exact starting state: `main` on hardware H264, `sub` back to `available: false` (software
   transcoder, no reader), and the only running `ffmpeg` process is the legitimate motion
   detector reading `main` — unaffected by any of this. All temporary files and the pre-upgrade
   mediamtx binary backup removed.

## Verdict

**R3.1: PASS** (10/10 crash-free). **R4.1/R4.2: PASS** (no regression under concurrent readers,
verified with a persistent reader + a second simultaneous reader). Task 5.1 (deploy) may
proceed.
