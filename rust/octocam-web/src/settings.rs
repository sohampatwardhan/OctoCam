use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{env, fs, io, path::PathBuf};

/// Pi hardware H.264 encoder limits. 1640x1232 is a valid IMX219 sensor mode but
/// exceeds 1080 encode lines; mediamtx then fails every frame with
/// `encoder_hardware_h264_encode(): ioctl(VIDIOC_QBUF) failed` and readers get 400.
pub const MAX_ENCODER_WIDTH: i32 = 1920;
pub const MAX_ENCODER_HEIGHT: i32 = 1080;

/// Fallback when a stored/submitted resolution exceeds the encoder limit:
/// the largest encoder-safe 4:3 preset (main) and the sub-stream default (sub).
const ENCODER_FALLBACK_MAIN: (i32, i32) = (1296, 972);
const ENCODER_FALLBACK_SUB: (i32, i32) = (640, 480);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub setup_complete: bool,
    pub admin_password_hash: String,
    pub device_name: String,
    pub room: String,
    pub camera_label: String,
    pub wifi_ssid: String,
    pub camera_enabled: bool,
    pub resolution_width: i32,
    pub resolution_height: i32,
    pub framerate: i32,
    pub bitrate_kbps: i32,
    pub rtsp_enabled: bool,
    pub rtsp_max_clients: i32,
    pub rtsp_path: String,
    pub sub_stream_enabled: bool,
    pub sub_resolution_width: i32,
    pub sub_resolution_height: i32,
    pub sub_framerate: i32,
    pub sub_bitrate_kbps: i32,
    pub sub_rtsp_max_clients: i32,
    pub sub_rtsp_path: String,
    pub rotation: i32,
    pub hflip: bool,
    pub vflip: bool,
    pub brightness: i32,
    pub contrast: f64,
    pub text_overlay_enabled: bool,
    pub text_overlay_timezone: String,
    pub text_overlay_clock_format: String,
    pub text_overlay_date_format: String,
    pub time_server: String,
    pub homekit_enabled: bool,
    pub homekit_paired: bool,
    pub matter_enabled: bool,
    pub motion_enabled: bool,
    pub motion_sensitivity: i32,
    pub scheduled_service_restart_enabled: bool,
    pub scheduled_service_restart_time: String,
    pub scheduled_reboot_enabled: bool,
    pub scheduled_reboot_time: String,
}

#[derive(Clone, Debug)]
pub struct ResolutionPreset {
    pub value: &'static str,
    pub label: &'static str,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug)]
pub struct PresetView {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

pub const RESOLUTION_PRESETS: &[ResolutionPreset] = &[
    ResolutionPreset {
        value: "640x480",
        label: "640 x 480 (4:3)",
        width: 640,
        height: 480,
    },
    ResolutionPreset {
        value: "800x600",
        label: "800 x 600 (4:3)",
        width: 800,
        height: 600,
    },
    ResolutionPreset {
        value: "1024x768",
        label: "1024 x 768 (4:3)",
        width: 1024,
        height: 768,
    },
    ResolutionPreset {
        value: "1296x972",
        label: "1296 x 972 (4:3)",
        width: 1296,
        height: 972,
    },
    ResolutionPreset {
        value: "1536x864",
        label: "1536 x 864 (16:9)",
        width: 1536,
        height: 864,
    },
    ResolutionPreset {
        value: "1280x720",
        label: "1280 x 720 (16:9 cropped)",
        width: 1280,
        height: 720,
    },
    ResolutionPreset {
        value: "1920x1080",
        label: "1920 x 1080 (16:9 cropped)",
        width: 1920,
        height: 1080,
    },
];

pub const SUB_RESOLUTION_PRESETS: &[ResolutionPreset] = &[
    ResolutionPreset {
        value: "320x240",
        label: "320 x 240 (4:3)",
        width: 320,
        height: 240,
    },
    ResolutionPreset {
        value: "640x480",
        label: "640 x 480 (4:3)",
        width: 640,
        height: 480,
    },
    ResolutionPreset {
        value: "800x600",
        label: "800 x 600 (4:3)",
        width: 800,
        height: 600,
    },
    ResolutionPreset {
        value: "1024x768",
        label: "1024 x 768 (4:3)",
        width: 1024,
        height: 768,
    },
    ResolutionPreset {
        value: "640x360",
        label: "640 x 360 (16:9 cropped)",
        width: 640,
        height: 360,
    },
    ResolutionPreset {
        value: "854x480",
        label: "854 x 480 (16:9 cropped)",
        width: 854,
        height: 480,
    },
];

impl Default for Settings {
    fn default() -> Self {
        Self {
            setup_complete: false,
            admin_password_hash: String::new(),
            device_name: "OctoCam".to_string(),
            room: "Living Room".to_string(),
            camera_label: "OctoCam".to_string(),
            wifi_ssid: String::new(),
            camera_enabled: true,
            resolution_width: 1280,
            resolution_height: 720,
            framerate: 15,
            bitrate_kbps: 2500,
            rtsp_enabled: true,
            rtsp_max_clients: 1,
            rtsp_path: "main".to_string(),
            sub_stream_enabled: true,
            sub_resolution_width: 640,
            sub_resolution_height: 480,
            sub_framerate: 10,
            sub_bitrate_kbps: 600,
            sub_rtsp_max_clients: 2,
            sub_rtsp_path: "sub".to_string(),
            rotation: 0,
            hflip: false,
            vflip: false,
            brightness: 0,
            contrast: 1.0,
            text_overlay_enabled: false,
            text_overlay_timezone: "Etc/UTC".to_string(),
            text_overlay_clock_format: "24h".to_string(),
            text_overlay_date_format: "yyyy-mm-dd".to_string(),
            time_server: "pool.ntp.org".to_string(),
            homekit_enabled: false,
            homekit_paired: false,
            matter_enabled: false,
            motion_enabled: false,
            motion_sensitivity: 50,
            scheduled_service_restart_enabled: false,
            scheduled_service_restart_time: "03:00".to_string(),
            scheduled_reboot_enabled: false,
            scheduled_reboot_time: "04:00".to_string(),
        }
    }
}

impl Settings {
    pub fn current_resolution(&self) -> String {
        format!("{}x{}", self.resolution_width, self.resolution_height)
    }

    pub fn current_sub_resolution(&self) -> String {
        format!(
            "{}x{}",
            self.sub_resolution_width, self.sub_resolution_height
        )
    }
}

pub fn default_config_path() -> PathBuf {
    env::var_os("OCTOCAM_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".config/octocam/settings.json")
        })
}

pub fn load_settings(path: &PathBuf) -> Settings {
    let Ok(raw) = fs::read_to_string(path) else {
        return Settings::default();
    };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&raw) else {
        return Settings::default();
    };
    validate_map(&map)
}

pub fn save_settings(path: &PathBuf, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let value = serde_json::to_string_pretty(settings)?;
    fs::write(path, format!("{value}\n"))
}

pub fn public_settings(settings: &Settings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.remove("admin_password_hash");
    }
    value
}

pub fn validate_form(fields: &std::collections::HashMap<String, String>) -> Settings {
    let mut map = Map::new();
    for (key, value) in fields {
        map.insert(key.clone(), Value::String(value.clone()));
    }
    validate_map(&map)
}

pub fn validate_map(raw: &Map<String, Value>) -> Settings {
    let mut settings = Settings::default();
    let mut map = raw.clone();
    apply_resolution_preset(
        &mut map,
        "resolution",
        "resolution_width",
        "resolution_height",
        RESOLUTION_PRESETS,
    );
    apply_resolution_preset(
        &mut map,
        "sub_resolution",
        "sub_resolution_width",
        "sub_resolution_height",
        SUB_RESOLUTION_PRESETS,
    );

    settings.setup_complete = bool_value(&map, "setup_complete", settings.setup_complete);
    settings.admin_password_hash = string_value(
        &map,
        "admin_password_hash",
        &settings.admin_password_hash,
        256,
    );
    settings.device_name = string_value(&map, "device_name", &settings.device_name, 80);
    settings.room = string_value(&map, "room", &settings.room, 80);
    settings.camera_label = string_value(&map, "camera_label", &settings.camera_label, 80);
    settings.wifi_ssid = string_value(&map, "wifi_ssid", &settings.wifi_ssid, 80);
    settings.camera_enabled = bool_value(&map, "camera_enabled", settings.camera_enabled);
    settings.resolution_width = int_value(
        &map,
        "resolution_width",
        settings.resolution_width,
        320,
        3280,
    );
    settings.resolution_height = int_value(
        &map,
        "resolution_height",
        settings.resolution_height,
        240,
        2464,
    );
    settings.framerate = int_value(&map, "framerate", settings.framerate, 1, 60);
    settings.bitrate_kbps = int_value(&map, "bitrate_kbps", settings.bitrate_kbps, 250, 25000);
    settings.rtsp_enabled = bool_value(&map, "rtsp_enabled", settings.rtsp_enabled);
    settings.rtsp_max_clients =
        int_value(&map, "rtsp_max_clients", settings.rtsp_max_clients, 1, 4);
    settings.rtsp_path = migrate_default_path(&path_value(&map, "rtsp_path", &settings.rtsp_path));
    settings.sub_stream_enabled =
        bool_value(&map, "sub_stream_enabled", settings.sub_stream_enabled);
    settings.sub_resolution_width = int_value(
        &map,
        "sub_resolution_width",
        settings.sub_resolution_width,
        320,
        1920,
    );
    settings.sub_resolution_height = int_value(
        &map,
        "sub_resolution_height",
        settings.sub_resolution_height,
        240,
        1440,
    );
    settings.sub_framerate = int_value(&map, "sub_framerate", settings.sub_framerate, 1, 30);
    settings.sub_bitrate_kbps = int_value(
        &map,
        "sub_bitrate_kbps",
        settings.sub_bitrate_kbps,
        150,
        5000,
    );
    settings.sub_rtsp_max_clients = int_value(
        &map,
        "sub_rtsp_max_clients",
        settings.sub_rtsp_max_clients,
        1,
        4,
    );
    settings.sub_rtsp_path =
        migrate_default_path(&path_value(&map, "sub_rtsp_path", &settings.sub_rtsp_path));
    settings.rotation = choice_value(&map, "rotation", settings.rotation, &[0, 90, 180, 270]);
    settings.hflip = bool_value(&map, "hflip", settings.hflip);
    settings.vflip = bool_value(&map, "vflip", settings.vflip);
    settings.brightness = int_value(&map, "brightness", settings.brightness, -100, 100);
    settings.contrast = float_value(&map, "contrast", settings.contrast, 0.0, 4.0);
    settings.text_overlay_enabled =
        bool_value(&map, "text_overlay_enabled", settings.text_overlay_enabled);
    settings.text_overlay_timezone = timezone_value(
        &map,
        "text_overlay_timezone",
        &settings.text_overlay_timezone,
    );
    settings.text_overlay_clock_format = clock_format_value(
        &map,
        "text_overlay_clock_format",
        &settings.text_overlay_clock_format,
    );
    settings.text_overlay_date_format = date_format_value(
        &map,
        "text_overlay_date_format",
        &settings.text_overlay_date_format,
    );
    settings.time_server = time_server_value(&map, "time_server", &settings.time_server);
    settings.homekit_enabled = bool_value(&map, "homekit_enabled", settings.homekit_enabled);
    settings.homekit_paired = bool_value(&map, "homekit_paired", settings.homekit_paired);
    settings.matter_enabled = bool_value(&map, "matter_enabled", settings.matter_enabled);
    settings.motion_enabled = bool_value(&map, "motion_enabled", settings.motion_enabled);
    settings.motion_sensitivity = int_value(
        &map,
        "motion_sensitivity",
        settings.motion_sensitivity,
        1,
        100,
    );
    settings.scheduled_service_restart_enabled = bool_value(
        &map,
        "scheduled_service_restart_enabled",
        settings.scheduled_service_restart_enabled,
    );
    settings.scheduled_service_restart_time = time_of_day_value(
        &map,
        "scheduled_service_restart_time",
        &settings.scheduled_service_restart_time,
    );
    settings.scheduled_reboot_enabled = bool_value(
        &map,
        "scheduled_reboot_enabled",
        settings.scheduled_reboot_enabled,
    );
    settings.scheduled_reboot_time = time_of_day_value(
        &map,
        "scheduled_reboot_time",
        &settings.scheduled_reboot_time,
    );
    clamp_to_encoder_limits(&mut settings);
    settings
}

/// The Matter pairing QR is a durable commission-this-camera credential, and
/// require_admin_login() is a no-op while the admin password hash is empty —
/// so an empty hash must force Matter off (spec: "octocam-web integration").
pub fn enforce_matter_requires_admin(settings: &mut Settings) {
    if settings.admin_password_hash.is_empty() {
        settings.matter_enabled = false;
    }
}

/// Snap any resolution the hardware encoder cannot handle to a safe fallback.
/// If either dimension exceeds the limit, BOTH are reset to the fallback preset —
/// we snap to a known-good mode rather than clamp per-axis into an untested
/// aspect ratio.
fn clamp_to_encoder_limits(settings: &mut Settings) {
    if settings.resolution_width > MAX_ENCODER_WIDTH
        || settings.resolution_height > MAX_ENCODER_HEIGHT
    {
        settings.resolution_width = ENCODER_FALLBACK_MAIN.0;
        settings.resolution_height = ENCODER_FALLBACK_MAIN.1;
    }
    if settings.sub_resolution_width > MAX_ENCODER_WIDTH
        || settings.sub_resolution_height > MAX_ENCODER_HEIGHT
    {
        settings.sub_resolution_width = ENCODER_FALLBACK_SUB.0;
        settings.sub_resolution_height = ENCODER_FALLBACK_SUB.1;
    }
}

pub fn preset_views(presets: &[ResolutionPreset], current: &str) -> Vec<PresetView> {
    presets
        .iter()
        .map(|preset| PresetView {
            value: preset.value.to_string(),
            label: preset.label.to_string(),
            selected: preset.value == current,
        })
        .collect()
}

fn apply_resolution_preset(
    raw: &mut Map<String, Value>,
    field: &str,
    width_field: &str,
    height_field: &str,
    presets: &[ResolutionPreset],
) {
    let Some(Value::String(value)) = raw.remove(field) else {
        return;
    };
    if let Some(preset) = presets.iter().find(|preset| preset.value == value) {
        raw.insert(width_field.to_string(), Value::from(preset.width));
        raw.insert(height_field.to_string(), Value::from(preset.height));
    }
}

fn string_value(map: &Map<String, Value>, key: &str, default: &str, max_len: usize) -> String {
    let value = match map.get(key) {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    };
    let value: String = value.chars().take(max_len).collect();
    if value.is_empty() {
        default.to_string()
    } else {
        value
    }
}

fn path_value(map: &Map<String, Value>, key: &str, default: &str) -> String {
    let raw = string_value(map, key, default, 80);
    let cleaned: String = raw
        .trim_matches('/')
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.' | '/'))
        .take(80)
        .collect();
    if cleaned.is_empty() {
        default.to_string()
    } else {
        cleaned
    }
}

fn timezone_value(map: &Map<String, Value>, key: &str, default: &str) -> String {
    let raw = string_value(map, key, default, 80);
    let cleaned: String = raw
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '/' | '_' | '-' | '+'))
        .take(80)
        .collect();
    if cleaned.is_empty()
        || cleaned.starts_with('/')
        || cleaned.contains("//")
        || cleaned.contains("..")
    {
        default.to_string()
    } else {
        cleaned
    }
}

fn clock_format_value(map: &Map<String, Value>, key: &str, default: &str) -> String {
    match string_value(map, key, default, 8).as_str() {
        "12h" => "12h".to_string(),
        "24h" => "24h".to_string(),
        _ => default.to_string(),
    }
}

fn date_format_value(map: &Map<String, Value>, key: &str, default: &str) -> String {
    match string_value(map, key, default, 16).as_str() {
        "dd/mm/yyyy" => "dd/mm/yyyy".to_string(),
        "mm/dd/yyyy" => "mm/dd/yyyy".to_string(),
        "yyyy-mm-dd" => "yyyy-mm-dd".to_string(),
        _ => default.to_string(),
    }
}

fn time_server_value(map: &Map<String, Value>, key: &str, default: &str) -> String {
    let raw = string_value(map, key, default, 120);
    let cleaned: String = raw
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '-' | ':'))
        .take(120)
        .collect();
    if cleaned.is_empty()
        || cleaned != raw
        || cleaned.starts_with(['.', '-', ':'])
        || cleaned.ends_with(['.', '-', ':'])
        || cleaned.contains("..")
    {
        default.to_string()
    } else {
        cleaned
    }
}

fn time_of_day_value(map: &Map<String, Value>, key: &str, default: &str) -> String {
    let raw = string_value(map, key, default, 8);
    let Some((hour, minute)) = raw.split_once(':') else {
        return default.to_string();
    };
    let (Ok(hour), Ok(minute)) = (hour.parse::<u32>(), minute.parse::<u32>()) else {
        return default.to_string();
    };
    if hour > 23 || minute > 59 {
        return default.to_string();
    }
    format!("{hour:02}:{minute:02}")
}

fn migrate_default_path(value: &str) -> String {
    match value {
        "octocam" => "main".to_string(),
        "octocam-sub" => "sub".to_string(),
        _ => value.to_string(),
    }
}

fn bool_value(map: &Map<String, Value>, key: &str, default: bool) -> bool {
    match map.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => {
            matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
        }
        Some(Value::Number(value)) => value.as_i64().unwrap_or(0) != 0,
        Some(_) => true,
        None => default,
    }
}

fn int_value(map: &Map<String, Value>, key: &str, default: i32, min: i32, max: i32) -> i32 {
    let value = match map.get(key) {
        Some(Value::Number(value)) => value.as_i64().map(|value| value as i32),
        Some(Value::String(value)) => value.parse::<i32>().ok(),
        _ => None,
    };
    value.unwrap_or(default).clamp(min, max)
}

fn choice_value(map: &Map<String, Value>, key: &str, default: i32, choices: &[i32]) -> i32 {
    let value = int_value(map, key, default, i32::MIN, i32::MAX);
    if choices.contains(&value) {
        value
    } else {
        default
    }
}

fn float_value(map: &Map<String, Value>, key: &str, default: f64, min: f64, max: f64) -> f64 {
    let value = match map.get(key) {
        Some(Value::Number(value)) => value.as_f64(),
        Some(Value::String(value)) => value.parse::<f64>().ok(),
        _ => None,
    };
    value.unwrap_or(default).clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_resolution_preset_and_bounds() {
        let mut map = Map::new();
        map.insert("resolution".into(), Value::String("1296x972".into()));
        map.insert("framerate".into(), Value::String("99".into()));
        let settings = validate_map(&map);
        assert_eq!(settings.resolution_width, 1296);
        assert_eq!(settings.resolution_height, 972);
        assert_eq!(settings.framerate, 60);
    }

    #[test]
    fn sanitizes_rtsp_paths() {
        let mut map = Map::new();
        map.insert("rtsp_path".into(), Value::String("/octo cam?bad/".into()));
        let settings = validate_map(&map);
        assert_eq!(settings.rtsp_path, "octocambad");
    }

    #[test]
    fn migrates_old_default_stream_paths() {
        let mut map = Map::new();
        map.insert("rtsp_path".into(), Value::String("octocam".into()));
        map.insert("sub_rtsp_path".into(), Value::String("octocam-sub".into()));
        let settings = validate_map(&map);
        assert_eq!(settings.rtsp_path, "main");
        assert_eq!(settings.sub_rtsp_path, "sub");
    }

    #[test]
    fn clamps_resolution_to_encoder_limit() {
        let mut map = Map::new();
        map.insert("resolution_width".into(), Value::from(1640));
        map.insert("resolution_height".into(), Value::from(1232));
        let settings = validate_map(&map);
        assert_eq!(settings.resolution_width, 1296);
        assert_eq!(settings.resolution_height, 972);
    }

    #[test]
    fn oversize_height_alone_snaps_to_fallback_preset() {
        let mut map = Map::new();
        map.insert("resolution_width".into(), Value::from(1920));
        map.insert("resolution_height".into(), Value::from(1232));
        let settings = validate_map(&map);
        assert_eq!(settings.resolution_width, 1296);
        assert_eq!(settings.resolution_height, 972);
    }

    #[test]
    fn keeps_legal_resolution_unchanged() {
        let mut map = Map::new();
        map.insert("resolution_width".into(), Value::from(1536));
        map.insert("resolution_height".into(), Value::from(864));
        let settings = validate_map(&map);
        assert_eq!(settings.resolution_width, 1536);
        assert_eq!(settings.resolution_height, 864);
    }

    #[test]
    fn presets_exclude_oversize_modes() {
        assert!(RESOLUTION_PRESETS
            .iter()
            .all(|p| p.width <= MAX_ENCODER_WIDTH && p.height <= MAX_ENCODER_HEIGHT));
        assert!(RESOLUTION_PRESETS.iter().any(|p| p.value == "1536x864"));
        assert!(!RESOLUTION_PRESETS.iter().any(|p| p.value == "1640x1232"));
    }

    #[test]
    fn validates_overlay_time_settings() {
        let mut map = Map::new();
        map.insert(
            "text_overlay_timezone".into(),
            Value::String("America/New_York".into()),
        );
        map.insert(
            "text_overlay_clock_format".into(),
            Value::String("12h".into()),
        );
        map.insert(
            "text_overlay_date_format".into(),
            Value::String("dd/mm/yyyy".into()),
        );
        let settings = validate_map(&map);
        assert_eq!(settings.text_overlay_timezone, "America/New_York");
        assert_eq!(settings.text_overlay_clock_format, "12h");
        assert_eq!(settings.text_overlay_date_format, "dd/mm/yyyy");

        map.insert(
            "text_overlay_timezone".into(),
            Value::String("../../etc/passwd".into()),
        );
        map.insert(
            "text_overlay_clock_format".into(),
            Value::String("metric".into()),
        );
        map.insert(
            "text_overlay_date_format".into(),
            Value::String("julian".into()),
        );
        let settings = validate_map(&map);
        assert_eq!(settings.text_overlay_timezone, "Etc/UTC");
        assert_eq!(settings.text_overlay_clock_format, "24h");
        assert_eq!(settings.text_overlay_date_format, "yyyy-mm-dd");
    }

    #[test]
    fn validates_time_server() {
        let mut map = Map::new();
        map.insert(
            "time_server".into(),
            Value::String("time.cloudflare.com".into()),
        );
        assert_eq!(validate_map(&map).time_server, "time.cloudflare.com");

        map.insert(
            "time_server".into(),
            Value::String("pool.ntp.org;reboot".into()),
        );
        assert_eq!(validate_map(&map).time_server, "pool.ntp.org");
    }

    #[test]
    fn validates_scheduled_maintenance() {
        let mut map = Map::new();
        map.insert(
            "scheduled_service_restart_enabled".into(),
            Value::String("true".into()),
        );
        map.insert(
            "scheduled_service_restart_time".into(),
            Value::String("3:05".into()),
        );
        map.insert(
            "scheduled_reboot_enabled".into(),
            Value::String("on".into()),
        );
        map.insert(
            "scheduled_reboot_time".into(),
            Value::String("23:59".into()),
        );
        let settings = validate_map(&map);
        assert!(settings.scheduled_service_restart_enabled);
        assert_eq!(settings.scheduled_service_restart_time, "03:05");
        assert!(settings.scheduled_reboot_enabled);
        assert_eq!(settings.scheduled_reboot_time, "23:59");

        map.insert(
            "scheduled_service_restart_time".into(),
            Value::String("99:99".into()),
        );
        map.insert(
            "scheduled_reboot_time".into(),
            Value::String("reboot now".into()),
        );
        let settings = validate_map(&map);
        assert_eq!(settings.scheduled_service_restart_time, "03:00");
        assert_eq!(settings.scheduled_reboot_time, "04:00");
    }

    #[test]
    fn matter_defaults_off_and_parses() {
        assert!(!Settings::default().matter_enabled);
        let mut map = Map::new();
        map.insert("matter_enabled".into(), Value::String("true".into()));
        map.insert("admin_password_hash".into(), Value::String("x".into()));
        assert!(validate_map(&map).matter_enabled);
    }

    #[test]
    fn matter_requires_admin_password() {
        let mut map = Map::new();
        map.insert("matter_enabled".into(), Value::String("true".into()));
        let mut s = validate_map(&map);
        assert!(s.admin_password_hash.is_empty());
        enforce_matter_requires_admin(&mut s);
        assert!(
            !s.matter_enabled,
            "matter must not enable without an admin password"
        );
        s.admin_password_hash = "hash".into();
        s.matter_enabled = true;
        enforce_matter_requires_admin(&mut s);
        assert!(s.matter_enabled);
    }
}
