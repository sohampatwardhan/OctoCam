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

pub fn default_timezone_dropin_path() -> PathBuf {
    env::var_os("OCTOCAM_RTSP_TIMEZONE_DROPIN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/etc/systemd/system/octocam-rtsp.service.d/10-octocam-timezone.conf")
        })
}

pub fn configure_rtsp_service(settings: &Settings, path: &PathBuf) -> ConfigureResult {
    let mut should_restart = false;
    let config = match write_mediamtx_config(settings, path) {
        Ok(changed) => {
            should_restart |= changed;
            ActionResult {
                path: Some(path.display().to_string()),
                unit: None,
                changed,
                message: if changed { "ok" } else { "unchanged" }.to_string(),
            }
        }
        Err(error) => ActionResult {
            path: Some(path.display().to_string()),
            unit: None,
            changed: false,
            message: error,
        },
    };
    let timezone = match write_timezone_dropin(settings, &default_timezone_dropin_path()) {
        Ok(changed) => {
            if changed {
                should_restart = true;
                if let Err(error) = system::daemon_reload() {
                    return ConfigureResult {
                        config,
                        service: ActionResult {
                            path: None,
                            unit: Some("octocam-rtsp".to_string()),
                            changed: false,
                            message: error,
                        },
                    };
                }
            }
            ActionResult {
                path: Some(default_timezone_dropin_path().display().to_string()),
                unit: None,
                changed,
                message: if changed { "ok" } else { "unchanged" }.to_string(),
            }
        }
        Err(error) => ActionResult {
            path: Some(default_timezone_dropin_path().display().to_string()),
            unit: None,
            changed: false,
            message: error,
        },
    };
    let service =
        match system::set_service_enabled("octocam-rtsp", rtsp_service_should_run(settings)) {
            Ok(()) => {
                if rtsp_service_should_run(settings) && should_restart {
                    if let Err(error) = system::restart_service("octocam-rtsp") {
                        return ConfigureResult {
                            config,
                            service: ActionResult {
                                path: timezone.path,
                                unit: Some("octocam-rtsp".to_string()),
                                changed: false,
                                message: error,
                            },
                        };
                    }
                }
                ActionResult {
                    path: timezone.path,
                    unit: Some("octocam-rtsp".to_string()),
                    changed: true,
                    message: "ok".to_string(),
                }
            }
            Err(error) => ActionResult {
                path: None,
                unit: Some("octocam-rtsp".to_string()),
                changed: false,
                message: error,
            },
        };
    ConfigureResult { config, service }
}

/// mediamtx must keep running while any local daemon consumes it, even when the
/// user turns LAN RTSP exposure off — rtsp_enabled=false used to stop the unit,
/// permanently killing the daemons' only video source.
pub fn rtsp_service_should_run(settings: &Settings) -> bool {
    settings.rtsp_enabled || settings.homekit_enabled || settings.matter_enabled
}

/// Must match `cameraStreamCount` advertised by the HomeKit bridge
/// (homekit/octocam-homekit.js). The bridge can open this many concurrent
/// live-view RTSP readers.
const HOMEKIT_STREAM_COUNT: i32 = 2;

pub fn render_mediamtx_config(settings: &Settings) -> String {
    let tuning_file = if settings.noir_mode {
        crate::camera::detect_camera_sensor()
            .and_then(|sensor| crate::camera::find_noir_tuning_file(&sensor))
    } else {
        None
    };

    // Reserve RTSP reader slots for the local daemons so their readers never eat
    // user-facing capacity. Be generous — over-reserving only raises the cap
    // (harmless), while under-reserving makes mediamtx refuse a daemon connection.
    // HomeKit may hold up to HOMEKIT_STREAM_COUNT live-view readers plus one HKSV
    // recording reader; motion.rs holds one persistent reader; Matter holds one.
    // A single scalar `reserve` is applied to every path; a path that doesn't need
    // all of it is simply capped a little higher, which is fine.
    let homekit_readers = if settings.homekit_enabled {
        HOMEKIT_STREAM_COUNT + i32::from(settings.hksv_enabled)
    } else {
        0
    };
    let reserve = homekit_readers
        + i32::from(settings.matter_enabled)
        + i32::from(settings.motion_enabled);
    let mut path_sections = vec![mediamtx_camera_path(
        &settings.rtsp_path,
        false,
        settings.text_overlay_enabled,
        &settings.camera_label,
        &settings.text_overlay_clock_format,
        &settings.text_overlay_date_format,
        settings.resolution_width,
        settings.resolution_height,
        settings.framerate,
        settings.bitrate_kbps,
        settings.rtsp_max_clients + reserve,
        tuning_file.as_deref(),
    )];

    if settings.sub_stream_enabled {
        if settings.text_overlay_enabled {
            path_sections.push(mediamtx_scaled_path(
                &settings.rtsp_path,
                &settings.sub_rtsp_path,
                settings.sub_resolution_width,
                settings.sub_resolution_height,
                settings.sub_framerate,
                settings.sub_bitrate_kbps,
                settings.sub_rtsp_max_clients + reserve,
            ));
        } else {
            path_sections.push(mediamtx_camera_path(
                &settings.sub_rtsp_path,
                true,
                false,
                &settings.camera_label,
                &settings.text_overlay_clock_format,
                &settings.text_overlay_date_format,
                settings.sub_resolution_width,
                settings.sub_resolution_height,
                settings.sub_framerate,
                settings.sub_bitrate_kbps,
                settings.sub_rtsp_max_clients + reserve,
                tuning_file.as_deref(),
            ));
        }
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

pub fn write_timezone_dropin(settings: &Settings, path: &PathBuf) -> Result<bool, String> {
    let next = render_timezone_dropin(settings);
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

pub fn render_timezone_dropin(settings: &Settings) -> String {
    format!(
        "[Service]\nEnvironment=TZ={}\n",
        settings.text_overlay_timezone
    )
}

pub fn mediamtx_camera_path(
    name: &str,
    secondary: bool,
    text_overlay_enabled: bool,
    camera_label: &str,
    clock_format: &str,
    date_format: &str,
    width: i32,
    height: i32,
    fps: i32,
    bitrate_kbps: i32,
    max_readers: i32,
    tuning_file: Option<&str>,
) -> String {
    let mut config = format!(
        "  {name}:\n    source: rpiCamera\n    rpiCameraSecondary: {secondary}\n    rpiCameraCodec: hardwareH264\n    rpiCameraH264Profile: baseline\n    rpiCameraIDRPeriod: {idr_period}\n    rpiCameraTextOverlayEnable: {text_overlay_enabled}\n    rpiCameraTextOverlay: {text_overlay}\n    rpiCameraWidth: {width}\n    rpiCameraHeight: {height}\n    rpiCameraFPS: {fps}\n    rpiCameraBitrate: {bitrate}\n    maxReaders: {max_readers}",
        name = yaml_quote(name),
        secondary = if secondary { "true" } else { "false" },
        idr_period = fps.max(1),
        text_overlay_enabled = if text_overlay_enabled { "true" } else { "false" },
        text_overlay = yaml_quote(&overlay_text(
            camera_label,
            clock_format,
            date_format,
            text_overlay_enabled
        )),
        bitrate = bitrate_kbps * 1000,
    );
    if let Some(tf) = tuning_file {
        config.push_str(&format!("\n    rpiCameraTuningFile: {}", yaml_quote(tf)));
    }
    config
}

pub fn mediamtx_scaled_path(
    source_path: &str,
    name: &str,
    width: i32,
    height: i32,
    fps: i32,
    bitrate_kbps: i32,
    max_readers: i32,
) -> String {
    let bitrate = bitrate_kbps * 1000;
    let command = format!(
        "ffmpeg -hide_banner -loglevel warning -rtsp_transport tcp -i rtsp://127.0.0.1:8554/{source_path} -vf scale={width}:{height},fps={fps} -c:v libx264 -preset veryfast -tune zerolatency -profile:v baseline -b:v {bitrate} -maxrate {bitrate} -bufsize {bufsize} -an -f rtsp rtsp://127.0.0.1:$RTSP_PORT/{name}",
        bufsize = bitrate * 2,
    );
    format!(
        "  {name}:\n    source: publisher\n    maxReaders: {max_readers}\n    runOnDemand: {command}\n    runOnDemandRestart: false\n    runOnDemandStartTimeout: 20s",
        name = yaml_quote(name),
        command = yaml_quote(&command),
    )
}

fn overlay_text(
    camera_label: &str,
    clock_format: &str,
    date_format: &str,
    enabled: bool,
) -> String {
    if enabled {
        let date_format = match date_format {
            "dd/mm/yyyy" => "%d/%m/%Y",
            "mm/dd/yyyy" => "%m/%d/%Y",
            _ => "%Y-%m-%d",
        };
        let time_format = if clock_format == "12h" {
            "%I:%M:%S %p"
        } else {
            "%H:%M:%S"
        };
        format!("{date_format} {time_format} - {camera_label}")
    } else {
        String::new()
    }
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
        let content = mediamtx_camera_path(
            &settings.rtsp_path,
            false,
            settings.text_overlay_enabled,
            &settings.camera_label,
            &settings.text_overlay_clock_format,
            &settings.text_overlay_date_format,
            1296,
            972,
            15,
            2500,
            1,
            None,
        );
        assert!(content.contains("\"main\":"));
        assert!(content.contains("rpiCameraWidth: 1296"));
        assert!(content.contains("rpiCameraIDRPeriod: 15"));
        assert!(content.contains("rpiCameraTextOverlayEnable: false"));
        assert!(content.contains("rpiCameraH264Profile: baseline"));
        assert!(content.contains("maxReaders: 1"));
    }

    #[test]
    fn renders_rpicamera_tuning_file() {
        let settings = Settings::default();
        let content = mediamtx_camera_path(
            &settings.rtsp_path,
            false,
            settings.text_overlay_enabled,
            &settings.camera_label,
            &settings.text_overlay_clock_format,
            &settings.text_overlay_date_format,
            1296,
            972,
            15,
            2500,
            1,
            Some("/usr/share/libcamera/ipa/rpi/vc4/ov5647_noir.json"),
        );
        assert!(content.contains("rpiCameraTuningFile: \"/usr/share/libcamera/ipa/rpi/vc4/ov5647_noir.json\""));
    }

    #[test]
    fn renders_text_overlay_and_scales_sd_from_hd_when_enabled() {
        let settings = Settings {
            text_overlay_enabled: true,
            camera_label: "Front Door".to_string(),
            ..Default::default()
        };
        let content = render_mediamtx_config(&settings);
        assert_eq!(
            content.matches("rpiCameraTextOverlayEnable: true").count(),
            1
        );
        assert!(content.contains("rpiCameraTextOverlay: \"%Y-%m-%d %H:%M:%S - Front Door\""));
        assert!(content.contains("\"sub\":\n    source: publisher"));
        assert!(content.contains("runOnDemand: \"ffmpeg "));
        assert!(content.contains("rtsp://127.0.0.1:8554/main"));
        assert!(content.contains("scale=640:480,fps=10"));
        assert!(!content.contains("rpiCameraSecondary: true"));
    }

    #[test]
    fn renders_12_hour_overlay_format() {
        let settings = Settings {
            text_overlay_enabled: true,
            text_overlay_clock_format: "12h".to_string(),
            ..Default::default()
        };
        let content = render_mediamtx_config(&settings);
        assert!(content.contains("rpiCameraTextOverlay: \"%Y-%m-%d %I:%M:%S %p - OctoCam\""));
    }

    #[test]
    fn renders_selected_overlay_date_format() {
        let settings = Settings {
            text_overlay_enabled: true,
            text_overlay_date_format: "dd/mm/yyyy".to_string(),
            ..Default::default()
        };
        let content = render_mediamtx_config(&settings);
        assert!(content.contains("rpiCameraTextOverlay: \"%d/%m/%Y %H:%M:%S - OctoCam\""));

        let settings = Settings {
            text_overlay_enabled: true,
            text_overlay_date_format: "mm/dd/yyyy".to_string(),
            ..Default::default()
        };
        let content = render_mediamtx_config(&settings);
        assert!(content.contains("rpiCameraTextOverlay: \"%m/%d/%Y %H:%M:%S - OctoCam\""));
    }

    #[test]
    fn renders_timezone_dropin() {
        let settings = Settings {
            text_overlay_timezone: "America/New_York".to_string(),
            ..Default::default()
        };
        assert_eq!(
            render_timezone_dropin(&settings),
            "[Service]\nEnvironment=TZ=America/New_York\n"
        );
    }

    #[test]
    fn config_enables_localhost_api() {
        let settings = Settings::default();
        let content = render_mediamtx_config(&settings);
        assert!(content.contains("api: yes"));
        assert!(content.contains("apiAddress: 127.0.0.1:9997"));
    }

    #[test]
    fn reserve_covers_worst_case_all_on_main() {
        // sub disabled => motion, HomeKit live-view (x cameraStreamCount), and the
        // HKSV recording pull all share the MAIN path. Budget must fit them on top
        // of the user-facing rtsp_max_clients.
        let s = Settings {
            homekit_enabled: true,
            hksv_enabled: true,
            motion_enabled: true,
            matter_enabled: false,
            sub_stream_enabled: false,
            rtsp_max_clients: 1,
            ..Default::default()
        };
        let content = render_mediamtx_config(&s);
        let main_max: i32 = content
            .lines()
            .find_map(|l| l.trim().strip_prefix("maxReaders: ").map(|v| v.parse().unwrap()))
            .expect("a maxReaders line");
        // Need: rtsp_max_clients(1) + motion(1) + live-view(2) + recording(1) = 5.
        assert!(main_max >= 5, "main path budget {main_max} must fit all concurrent readers (>=5)");
    }

    #[test]
    fn homekit_reserve_adds_stream_count_readers() {
        let mut settings = Settings {
            homekit_enabled: false,
            ..Default::default()
        };
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
            assert_eq!(
                y - x,
                HOMEKIT_STREAM_COUNT,
                "homekit reserve must add cameraStreamCount readers per path"
            );
        }
    }

    #[test]
    fn matter_reserve_adds_one_reader() {
        let mut settings = Settings {
            matter_enabled: false,
            ..Default::default()
        };
        let without = render_mediamtx_config(&settings);
        settings.matter_enabled = true;
        let with = render_mediamtx_config(&settings);
        let max_readers = |content: &str| -> Vec<i32> {
            content
                .lines()
                .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
                .map(|v| v.parse().unwrap())
                .collect()
        };
        for (x, y) in max_readers(&without).iter().zip(max_readers(&with).iter()) {
            assert_eq!(
                y - x,
                1,
                "matter reserve must add exactly one reader per path"
            );
        }
    }

    #[test]
    fn homekit_and_matter_reserves_are_additive() {
        let base = Settings {
            homekit_enabled: false,
            matter_enabled: false,
            ..Default::default()
        };
        let both = Settings {
            homekit_enabled: true,
            matter_enabled: true,
            ..base.clone()
        };
        let first_max = |content: &str| -> i32 {
            content
                .lines()
                .find_map(|l| l.trim().strip_prefix("maxReaders: "))
                .unwrap()
                .parse()
                .unwrap()
        };
        // matter adds 1, homekit adds HOMEKIT_STREAM_COUNT (defaults: motion off,
        // hksv off), so enabling both raises the budget by 1 + cameraStreamCount.
        assert_eq!(
            first_max(&render_mediamtx_config(&both)) - first_max(&render_mediamtx_config(&base)),
            1 + HOMEKIT_STREAM_COUNT
        );
    }

    #[test]
    fn hksv_reserves_an_extra_reader_when_homekit_enabled() {
        let max_readers = |content: &str| -> Vec<i32> {
            content
                .lines()
                .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
                .map(|v| v.parse().unwrap())
                .collect()
        };
        let base = Settings {
            homekit_enabled: true,
            hksv_enabled: false,
            ..Default::default()
        };
        let with_hksv = Settings {
            hksv_enabled: true,
            ..base.clone()
        };
        let a = max_readers(&render_mediamtx_config(&base));
        let b = max_readers(&render_mediamtx_config(&with_hksv));
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(y - x, 1, "enabling HKSV must reserve one more reader per path");
        }
    }

    #[test]
    fn hksv_reserves_nothing_without_homekit() {
        let max_readers = |content: &str| -> Vec<i32> {
            content
                .lines()
                .filter_map(|l| l.trim().strip_prefix("maxReaders: "))
                .map(|v| v.parse().unwrap())
                .collect()
        };
        let base = Settings {
            homekit_enabled: false,
            hksv_enabled: false,
            ..Default::default()
        };
        let hksv_no_homekit = Settings {
            hksv_enabled: true,
            ..base.clone()
        };
        let a = max_readers(&render_mediamtx_config(&base));
        let b = max_readers(&render_mediamtx_config(&hksv_no_homekit));
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(y - x, 0, "HKSV without HomeKit must not change the reader budget");
        }
    }

    #[test]
    fn rtsp_service_runs_whenever_a_daemon_needs_it() {
        // rtsp_enabled=false must NOT stop mediamtx while a daemon consumes it —
        // the daemons' only video source is this unit (hardening FIX-1).
        let mut s = Settings {
            rtsp_enabled: false,
            homekit_enabled: false,
            matter_enabled: false,
            ..Default::default()
        };
        assert!(!rtsp_service_should_run(&s));
        s.matter_enabled = true;
        assert!(rtsp_service_should_run(&s));
        s.matter_enabled = false;
        s.homekit_enabled = true;
        assert!(rtsp_service_should_run(&s));
        s = Settings {
            rtsp_enabled: true,
            ..Default::default()
        };
        assert!(rtsp_service_should_run(&s));
    }
}
