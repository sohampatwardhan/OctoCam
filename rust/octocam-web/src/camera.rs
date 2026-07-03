use crate::{settings::Settings, system};
use std::process::Command;
use std::time::{Duration, Instant};

pub const SNAPSHOT_TTL: Duration = Duration::from_secs(2);

pub fn snapshot_is_fresh(captured: Instant, now: Instant) -> bool {
    now.duration_since(captured) < SNAPSHOT_TTL
}

/// Grab one JPEG frame through mediamtx. While mediamtx runs, its rpiCamera
/// source owns the camera continuously and libcamera allows a single
/// consumer — `rpicam-still` CANNOT acquire the device then, so direct capture
/// would always fail. Pull a frame off the sub stream instead (same pattern the
/// HomeKit daemon already uses for its snapshots).
pub fn capture_jpeg_via_rtsp(settings: &Settings) -> Result<Vec<u8>, String> {
    let path = if settings.sub_stream_enabled {
        &settings.sub_rtsp_path
    } else {
        &settings.rtsp_path
    };
    let url = format!("rtsp://127.0.0.1:8554/{}", path.trim_start_matches('/'));
    let output = crate::proc::run(
        Command::new("ffmpeg").args([
            "-hide_banner",
            "-nostdin",
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

    let output = crate::proc::run(Command::new(&command).args(args), crate::proc::CAPTURE_TIMEOUT)
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
