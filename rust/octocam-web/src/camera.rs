use crate::{settings::Settings, system};
use std::process::Command;
use std::time::{Duration, Instant};

pub const SNAPSHOT_TTL: Duration = Duration::from_secs(2);

pub fn snapshot_is_fresh(captured: Instant, now: Instant) -> bool {
    now.duration_since(captured) < SNAPSHOT_TTL
}

/// Picks which RTSP path a one-shot snapshot capture should pull from.
///
/// `sub` is only free to grab from when it's the always-running *hardware*
/// secondary stream — when `text_overlay_enabled` is on, `sub` is instead an
/// on-demand *software* transcoder (`mediamtx::mediamtx_scaled_path`):
/// capturing from it would cold-start that transcoder just to grab one JPEG,
/// competing with motion detection's own read of `main` for CPU and starving
/// it — the same regression class `motion.rs`'s `resolve_motion_source`
/// already guards against for persistent reads. Mirrors that exact condition
/// rather than falling into the trap it was built to avoid.
fn snapshot_source_path(settings: &Settings) -> &str {
    if settings.sub_stream_enabled && !settings.text_overlay_enabled {
        &settings.sub_rtsp_path
    } else {
        &settings.rtsp_path
    }
}

/// Grab one JPEG frame through mediamtx. While mediamtx runs, its rpiCamera
/// source owns the camera continuously and libcamera allows a single
/// consumer — `rpicam-still` CANNOT acquire the device then, so direct capture
/// would always fail. Pull a frame off the sub stream instead (same pattern
/// the HomeKit daemon already uses for its snapshots) whenever
/// [`snapshot_source_path`] says it's safe to.
pub fn capture_jpeg_via_rtsp(settings: &Settings) -> Result<Vec<u8>, String> {
    let path = snapshot_source_path(settings);
    let url = format!("rtsp://127.0.0.1:8554/{}", path.trim_start_matches('/'));
    let output = crate::proc::run(
        Command::new("ffmpeg").args([
            "-hide_banner",
            "-nostdin",
            // The source is a local RTSP stream whose SDP already describes it, so
            // ffmpeg's default multi-second probe buys nothing but latency — the
            // same reasoning already applied to the scaled RTSP path in
            // mediamtx.rs's mediamtx_scaled_path(). Without this, capturing from
            // `main` (full-resolution H264, needing a real decode) could run past
            // CAPTURE_TIMEOUT under concurrent load from motion detection's own
            // decode of the same stream.
            "-fflags",
            "nobuffer",
            "-flags",
            "low_delay",
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-rtsp_transport",
            "tcp",
            "-i",
            &url,
            "-frames:v",
            "1",
            "-f",
            "image2",
            "-c:v",
            "mjpeg",
            "-",
        ]),
        crate::proc::CAPTURE_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    if output.status.success() && !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Route snapshots through mediamtx whenever it owns the camera — that is
/// whenever the octocam-rtsp unit runs, not just when LAN RTSP is exposed.
/// The routing condition MUST be the same predicate that decides whether the
/// unit runs (`rtsp_service_should_run`, unit-tested in mediamtx.rs); routing
/// on `rtsp_enabled` alone would run rpicam-still while mediamtx still holds
/// the single libcamera consumer slot, failing every snapshot.
pub fn capture_snapshot(settings: &Settings) -> Result<Vec<u8>, String> {
    if crate::mediamtx::rtsp_service_should_run(settings) {
        capture_jpeg_via_rtsp(settings)
    } else {
        capture_jpeg(settings)
    }
}

pub fn capture_jpeg(settings: &Settings) -> Result<Vec<u8>, String> {
    let command = system::first_available_command(&["rpicam-still", "libcamera-still"])
        .ok_or_else(|| "No rpicam-still/libcamera-still command found.".to_string())?;

    let mut args = vec![
        "-o".to_string(),
        "-".to_string(),
        "--width".to_string(),
        settings.resolution_width.to_string(),
        "--height".to_string(),
        settings.resolution_height.to_string(),
        "--timeout".to_string(),
        "350".to_string(),
        "--nopreview".to_string(),
    ];

    if settings.hflip || settings.rotation == 180 {
        args.push("--hflip".to_string());
    }
    if settings.vflip || settings.rotation == 180 {
        args.push("--vflip".to_string());
    }
    if settings.rotation != 0 && settings.rotation != 180 {
        args.push("--rotation".to_string());
        args.push(settings.rotation.to_string());
    }
    if settings.noir_mode {
        if let Some(sensor) = detect_camera_sensor() {
            if let Some(tuning_file) = find_noir_tuning_file(&sensor) {
                args.push("--tuning-file".to_string());
                args.push(tuning_file);
            }
        }
    }

    let output = crate::proc::run(
        Command::new(&command).args(args),
        crate::proc::CAPTURE_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    if output.status.success() && !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        let message = String::from_utf8_lossy(if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        });
        Err(message.trim().to_string())
    }
}

pub fn detect_camera_sensor() -> Option<String> {
    let command = system::first_available_command(&["rpicam-still", "libcamera-still"])?;
    let output = crate::proc::run(
        Command::new(&command).arg("--list-cameras"),
        crate::proc::SCAN_TIMEOUT,
    ).ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let full_output = format!("{}\n{}", stdout, stderr);

    for line in full_output.lines() {
        if let Some(pos) = line.find(" : ") {
            let part = &line[pos + 3..];
            if let Some(sensor) = part.split_whitespace().next() {
                let sensor_cleaned: String = sensor.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                if !sensor_cleaned.is_empty() {
                    return Some(sensor_cleaned);
                }
            }
        }
    }
    None
}

pub fn find_noir_tuning_file(sensor: &str) -> Option<String> {
    let paths = [
        format!("/usr/share/libcamera/ipa/rpi/vc4/{}_noir.json", sensor),
        format!("/usr/share/libcamera/ipa/rpi/pisp/{}_noir.json", sensor),
    ];
    for path in &paths {
        if std::path::Path::new(path).exists() {
            return Some(path.clone());
        }
    }
    None
}

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

    #[test]
    fn snapshot_uses_sub_only_when_it_is_the_hardware_secondary_stream() {
        // sub enabled + no overlay => sub is the free-running hardware secondary
        // stream (mediamtx.rs's rpiCameraSecondary path) — cheap to grab from.
        let hardware_sub = Settings {
            sub_stream_enabled: true,
            text_overlay_enabled: false,
            ..Settings::default()
        };
        assert_eq!(snapshot_source_path(&hardware_sub), "sub");

        // sub enabled + overlay on => sub is the on-demand SOFTWARE transcoder.
        // Capturing from it would cold-start that transcoder just for one JPEG,
        // starving motion detection's read of `main` — must fall back to `main`.
        let software_sub = Settings {
            sub_stream_enabled: true,
            text_overlay_enabled: true,
            ..Settings::default()
        };
        assert_eq!(snapshot_source_path(&software_sub), "main");

        // sub disabled entirely => always main, regardless of overlay.
        let sub_disabled = Settings {
            sub_stream_enabled: false,
            text_overlay_enabled: false,
            ..Settings::default()
        };
        assert_eq!(snapshot_source_path(&sub_disabled), "main");

        let sub_disabled_overlay_on = Settings {
            sub_stream_enabled: false,
            text_overlay_enabled: true,
            ..Settings::default()
        };
        assert_eq!(snapshot_source_path(&sub_disabled_overlay_on), "main");
    }
}
