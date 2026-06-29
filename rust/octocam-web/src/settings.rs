use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{env, fs, io, path::PathBuf};

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
    pub homekit_enabled: bool,
    pub homekit_paired: bool,
    pub motion_enabled: bool,
    pub motion_sensitivity: i32,
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
        value: "1640x1232",
        label: "1640 x 1232 (4:3)",
        width: 1640,
        height: 1232,
    },
    ResolutionPreset {
        value: "1920x1440",
        label: "1920 x 1440 (4:3)",
        width: 1920,
        height: 1440,
    },
    ResolutionPreset {
        value: "3280x2464",
        label: "3280 x 2464 (4:3 full sensor)",
        width: 3280,
        height: 2464,
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
            homekit_enabled: false,
            homekit_paired: false,
            motion_enabled: false,
            motion_sensitivity: 50,
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
    settings.homekit_enabled = bool_value(&map, "homekit_enabled", settings.homekit_enabled);
    settings.homekit_paired = bool_value(&map, "homekit_paired", settings.homekit_paired);
    settings.motion_enabled = bool_value(&map, "motion_enabled", settings.motion_enabled);
    settings.motion_sensitivity = int_value(
        &map,
        "motion_sensitivity",
        settings.motion_sensitivity,
        1,
        100,
    );
    settings
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
}
