use crate::{settings::Settings, system};
use serde::Serialize;
use std::{env, fs, path::PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct ConfigureResult {
    pub config: ActionResult,
    pub service: ActionResult,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActionResult {
    pub path: Option<String>,
    pub unit: Option<String>,
    pub changed: bool,
    pub message: String,
}

pub fn default_config_path() -> PathBuf {
    env::var_os("OCTOCAM_MEDIAMTX_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/mediamtx.yml"))
}

pub fn configure_rtsp_service(settings: &Settings, path: &PathBuf) -> ConfigureResult {
    let config = match write_mediamtx_config(settings, path) {
        Ok(changed) => ActionResult {
            path: Some(path.display().to_string()),
            unit: None,
            changed,
            message: if changed { "ok" } else { "unchanged" }.to_string(),
        },
        Err(error) => ActionResult {
            path: Some(path.display().to_string()),
            unit: None,
            changed: false,
            message: error,
        },
    };
    let service = match system::set_service_enabled("octocam-rtsp", settings.rtsp_enabled) {
        Ok(()) => ActionResult {
            path: None,
            unit: Some("octocam-rtsp".to_string()),
            changed: true,
            message: "ok".to_string(),
        },
        Err(error) => ActionResult {
            path: None,
            unit: Some("octocam-rtsp".to_string()),
            changed: false,
            message: error,
        },
    };
    ConfigureResult { config, service }
}

pub fn render_mediamtx_config(settings: &Settings) -> String {
    // HomeKit's daemon reads via a local RTSP session (ffmpeg), so reserve one
    // reader slot per path — user-facing capacity should not shrink when the
    // Home app is watching. Soft reservation: see the design doc.
    let reserve = if settings.homekit_enabled { 1 } else { 0 };
    let mut path_sections = vec![mediamtx_camera_path(
        &settings.rtsp_path,
        false,
        settings.resolution_width,
        settings.resolution_height,
        settings.framerate,
        settings.bitrate_kbps,
        settings.rtsp_max_clients + reserve,
    )];

    if settings.sub_stream_enabled {
        path_sections.push(mediamtx_camera_path(
            &settings.sub_rtsp_path,
            true,
            settings.sub_resolution_width,
            settings.sub_resolution_height,
            settings.sub_framerate,
            settings.sub_bitrate_kbps,
            settings.sub_rtsp_max_clients + reserve,
        ));
    }

    let mut content = vec![
        "logLevel: info".to_string(),
        String::new(),
        "api: yes".to_string(),
        "apiAddress: 127.0.0.1:9997".to_string(),
        String::new(),
        "rtsp: true".to_string(),
        "rtspAddress: :8554".to_string(),
        "rtspTransports: [udp, tcp]".to_string(),
        String::new(),
        "rtmp: false".to_string(),
        "hls: true".to_string(),
        "hlsAddress: :8888".to_string(),
        "hlsAllowOrigins: ['*']".to_string(),
        "hlsVariant: lowLatency".to_string(),
        "hlsSegmentDuration: 1s".to_string(),
        "hlsPartDuration: 200ms".to_string(),
        String::new(),
        "webrtc: true".to_string(),
        "webrtcAddress: :8889".to_string(),
        "webrtcAllowOrigins: ['*']".to_string(),
        "webrtcLocalUDPAddress: :8189".to_string(),
        "webrtcLocalTCPAddress: ''".to_string(),
        "srt: false".to_string(),
        "moq: false".to_string(),
        String::new(),
        "paths:".to_string(),
    ];
    content.extend(path_sections);
    content.push(String::new());
    content.join("\n")
}

/// Writes the config; returns Ok(true) if the file content changed.
pub fn write_mediamtx_config(settings: &Settings, path: &PathBuf) -> Result<bool, String> {
    let next = render_mediamtx_config(settings);
    let current = fs::read_to_string(path).unwrap_or_default();
    if current == next {
        return Ok(false);
    }
    fs::write(path, next).map_err(|error| error.to_string())?;
    Ok(true)
}

pub fn mediamtx_camera_path(
    name: &str,
    secondary: bool,
    width: i32,
    height: i32,
    fps: i32,
    bitrate_kbps: i32,
    max_readers: i32,
) -> String {
    format!(
        "  {name}:\n    source: rpiCamera\n    rpiCameraSecondary: {secondary}\n    rpiCameraCodec: hardwareH264\n    rpiCameraH264Profile: baseline\n    rpiCameraIDRPeriod: {idr_period}\n    rpiCameraWidth: {width}\n    rpiCameraHeight: {height}\n    rpiCameraFPS: {fps}\n    rpiCameraBitrate: {bitrate}\n    maxReaders: {max_readers}",
        name = yaml_quote(name),
        secondary = if secondary { "true" } else { "false" },
        idr_period = fps.max(1),
        bitrate = bitrate_kbps * 1000,
    )
}

fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_main_and_sub_paths() {
        let settings = Settings::default();
        let content = mediamtx_camera_path(&settings.rtsp_path, false, 1296, 972, 15, 2500, 1);
        assert!(content.contains("\"main\":"));
        assert!(content.contains("rpiCameraWidth: 1296"));
        assert!(content.contains("rpiCameraIDRPeriod: 15"));
        assert!(content.contains("rpiCameraH264Profile: baseline"));
        assert!(content.contains("maxReaders: 1"));
    }

    #[test]
    fn config_enables_localhost_api() {
        let settings = Settings::default();
        let content = render_mediamtx_config(&settings);
        assert!(content.contains("api: yes"));
        assert!(content.contains("apiAddress: 127.0.0.1:9997"));
    }

    #[test]
    fn homekit_reserve_adds_one_reader() {
        let mut settings = Settings::default();
        settings.homekit_enabled = false;
        let without = render_mediamtx_config(&settings);
        settings.homekit_enabled = true;
        let with = render_mediamtx_config(&settings);
        let max_readers = |content: &str| -> Vec<i32> {
            content
                .lines()
                .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
                .map(|v| v.parse().unwrap())
                .collect()
        };
        let a = max_readers(&without);
        let b = max_readers(&with);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(y - x, 1, "homekit reserve must add exactly one reader per path");
        }
    }
}
