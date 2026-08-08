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

/// A stored credential that refuses to print itself.
///
/// `Settings` derives `Debug`, so any `tracing` call that formats the whole
/// struct — a routine thing to add while debugging — would otherwise dump the
/// broker password into the journal. Making redaction a property of the type
/// means that cannot happen by accident, rather than depending on every future
/// author remembering not to.
///
/// `#[serde(transparent)]` keeps the on-disk and wire representation a plain
/// string, so existing settings files and API payloads are unaffected.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "Secret(empty)" } else { "Secret(redacted)" })
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Secret(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Secret(value.to_string())
    }
}

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
    pub scheduled_service_restart_days: String,
    pub scheduled_reboot_enabled: bool,
    pub scheduled_reboot_time: String,
    pub scheduled_reboot_days: String,
    pub noir_mode: bool,
    pub motion_zones: u64,
    pub hksv_enabled: bool,

    // MQTT publishing to a Home Assistant broker. Disabled by default so the
    // device makes no outbound broker connection until an admin opts in.
    pub mqtt_enabled: bool,
    pub mqtt_host: String,
    pub mqtt_port: i32,
    pub mqtt_username: String,
    /// Broker credential. Deliberately absent from `public_settings`, from
    /// `backup::PORTABLE_FIELDS`, and from every log statement: it is the one
    /// value here that grants access to a system outside this device. Stored
    /// unencrypted, which is a reviewed decision — the settings file is
    /// root-owned and already holds `admin_password_hash`, so encrypting with a
    /// key on the same disk would add ceremony rather than protection.
    pub mqtt_password: Secret,
    pub mqtt_tls: bool,
    pub mqtt_client_id: String,
    pub mqtt_base_topic: String,
    pub mqtt_discovery_prefix: String,
    /// This camera's stable MQTT identity, generated once and then never
    /// derived from anything mutable. Home Assistant keys entities off it, so
    /// deriving it from the device name would orphan the entity on every
    /// rename, and deriving it from a MAC or IP would break on hardware or
    /// network change. Excluded from backups so restoring onto a second camera
    /// cannot make two devices claim one entity.
    pub mqtt_node_id: String,
}

#[derive(Clone, Debug)]
pub struct ResolutionPreset {
    pub value: &'static str,
    pub label: &'static str,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Serialize)]
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
            scheduled_service_restart_days: default_weekdays(),
            scheduled_reboot_enabled: false,
            scheduled_reboot_time: "04:00".to_string(),
            scheduled_reboot_days: default_weekdays(),
            noir_mode: false,
            motion_zones: u64::MAX,
            hksv_enabled: false,
            mqtt_enabled: false,
            mqtt_host: String::new(),
            mqtt_port: 1883,
            mqtt_username: String::new(),
            mqtt_password: Secret::default(),
            mqtt_tls: false,
            mqtt_client_id: String::new(),
            mqtt_base_topic: "octocam".to_string(),
            mqtt_discovery_prefix: "homeassistant".to_string(),
            // Empty until `ensure_mqtt_node_id` mints one. Default must stay
            // deterministic so tests and comparisons are stable.
            mqtt_node_id: String::new(),
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

/// Rejects an MQTT submission that would produce an unusable broker configuration.
///
/// Runs on the *merged* settings map (stored values plus the incoming patch),
/// before anything is written, so returning `Err` leaves the stored
/// configuration untouched. This exists separately from `validate_map` because
/// that function clamps out-of-range integers into range — a submitted port of
/// `0` would silently become `1` and a caller could never tell a correction
/// from a mistake.
///
/// Only outright unusable input is rejected. A disabled MQTT configuration is
/// never rejected, so a user can save a partially filled form and come back to
/// it.
pub fn validate_mqtt_submission(merged: &Map<String, Value>) -> Result<(), String> {
    if let Some(value) = merged.get("mqtt_port") {
        let port = value
            .as_i64()
            .or_else(|| value.as_str().and_then(|raw| raw.trim().parse::<i64>().ok()));
        match port {
            Some(port) if (1..=65535).contains(&port) => {}
            _ => return Err("MQTT port must be between 1 and 65535.".to_string()),
        }
    }

    let enabled = merged
        .get("mqtt_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if enabled {
        let host = merged
            .get("mqtt_host")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if host.is_empty() {
            return Err("MQTT host is required when MQTT is enabled.".to_string());
        }
    }
    Ok(())
}

/// Mints this camera's MQTT identity if it does not have one yet, returning
/// whether the caller now needs to persist the settings.
///
/// Called at startup rather than at first enable so the identity is fixed
/// before any topic string is built from it. Random rather than derived: see
/// the `mqtt_node_id` field for why nothing mutable can be used as a source.
pub fn ensure_mqtt_node_id(settings: &mut Settings) -> bool {
    if !settings.mqtt_node_id.is_empty() {
        return false;
    }
    use rand::Rng;
    let mut rng = rand::thread_rng();
    settings.mqtt_node_id = (0..8)
        .map(|_| {
            let n: u8 = rng.gen_range(0..16);
            std::char::from_digit(n as u32, 16).unwrap_or('0')
        })
        .collect();
    true
}

pub fn public_settings(settings: &Settings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.remove("admin_password_hash");
        // The broker credential never leaves the device. The UI still needs to
        // know whether one exists so it can show "set" without the value, so
        // swap the secret for a boolean rather than dropping it silently.
        let password_set = map
            .get("mqtt_password")
            .and_then(Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false);
        map.remove("mqtt_password");
        map.insert("mqtt_password_set".to_string(), Value::Bool(password_set));
        // motion_zones is a u64 bitmask; as a bare JSON number it can exceed
        // JS's Number.MAX_SAFE_INTEGER (2^53-1) and lose precision in the
        // browser. Emit it as a decimal string instead — u64_value() above
        // already accepts a Value::String on the way back in via PUT.
        if let Some(zones) = map.get("motion_zones").and_then(|v| v.as_u64()) {
            map.insert("motion_zones".to_string(), Value::String(zones.to_string()));
        }
    }
    value
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
    settings.motion_zones = u64_value(&map, "motion_zones", settings.motion_zones);
    settings.mqtt_enabled = bool_value(&map, "mqtt_enabled", settings.mqtt_enabled);
    settings.mqtt_host = string_value(&map, "mqtt_host", &settings.mqtt_host, 255);
    settings.mqtt_port = int_value(&map, "mqtt_port", settings.mqtt_port, 1, 65535);
    settings.mqtt_username = string_value(&map, "mqtt_username", &settings.mqtt_username, 128);
    settings.mqtt_password =
        Secret::from(string_value(&map, "mqtt_password", settings.mqtt_password.expose(), 256));
    settings.mqtt_tls = bool_value(&map, "mqtt_tls", settings.mqtt_tls);
    settings.mqtt_client_id = string_value(&map, "mqtt_client_id", &settings.mqtt_client_id, 128);
    settings.mqtt_base_topic = string_value(&map, "mqtt_base_topic", &settings.mqtt_base_topic, 128);
    settings.mqtt_discovery_prefix = string_value(
        &map,
        "mqtt_discovery_prefix",
        &settings.mqtt_discovery_prefix,
        128,
    );
    settings.mqtt_node_id = string_value(&map, "mqtt_node_id", &settings.mqtt_node_id, 64);
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
    settings.scheduled_service_restart_days = weekdays_value(
        &map,
        "scheduled_service_restart_days",
        "scheduled_service_restart_day_",
        &settings.scheduled_service_restart_days,
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
    settings.scheduled_reboot_days = weekdays_value(
        &map,
        "scheduled_reboot_days",
        "scheduled_reboot_day_",
        &settings.scheduled_reboot_days,
    );
    settings.noir_mode = bool_value(&map, "noir_mode", settings.noir_mode);
    settings.hksv_enabled = bool_value(&map, "hksv_enabled", settings.hksv_enabled);
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

/// HKSV recording is triggered by the motion sensor; without motion detection
/// there is nothing to start a recording. Force HKSV off when motion is off so
/// the bridge never advertises a recording capability it can't trigger.
pub fn enforce_hksv_requires_motion(settings: &mut Settings) {
    if !settings.motion_enabled {
        settings.hksv_enabled = false;
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

pub const WEEKDAYS: &[(&str, &str, &str)] = &[
    ("mon", "Mon", "Mon"),
    ("tue", "Tue", "Tue"),
    ("wed", "Wed", "Wed"),
    ("thu", "Thu", "Thu"),
    ("fri", "Fri", "Fri"),
    ("sat", "Sat", "Sat"),
    ("sun", "Sun", "Sun"),
];

fn default_weekdays() -> String {
    WEEKDAYS
        .iter()
        .map(|(_, systemd, _)| *systemd)
        .collect::<Vec<_>>()
        .join(",")
}

fn weekdays_value(
    map: &Map<String, Value>,
    key: &str,
    checkbox_prefix: &str,
    default: &str,
) -> String {
    let has_checkbox_values = WEEKDAYS
        .iter()
        .any(|(slug, _, _)| map.contains_key(&format!("{checkbox_prefix}{slug}")));
    let selected = if has_checkbox_values {
        WEEKDAYS
            .iter()
            .filter_map(|(slug, systemd, _)| {
                bool_value(map, &format!("{checkbox_prefix}{slug}"), false).then_some(*systemd)
            })
            .collect::<Vec<_>>()
    } else {
        let raw = string_value(map, key, default, 80);
        WEEKDAYS
            .iter()
            .filter_map(|(_, systemd, _)| {
                raw.split(',')
                    .map(str::trim)
                    .any(|part| part.eq_ignore_ascii_case(systemd))
                    .then_some(*systemd)
            })
            .collect::<Vec<_>>()
    };
    if selected.is_empty() {
        default.to_string()
    } else {
        selected.join(",")
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

fn u64_value(map: &Map<String, Value>, key: &str, default: u64) -> u64 {
    match map.get(key) {
        Some(Value::Number(value)) => value.as_u64().unwrap_or(default),
        Some(Value::String(value)) => {
            let val_str = value.trim();
            if let Some(hex_str) = val_str.strip_prefix("0x") {
                u64::from_str_radix(hex_str, 16).unwrap_or(default)
            } else if let Ok(val) = val_str.parse::<u64>() {
                val
            } else {
                u64::from_str_radix(val_str, 16).unwrap_or(default)
            }
        }
        _ => default,
    }
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
    fn mqtt_defaults_are_disabled_with_home_assistant_prefix() {
        let settings = Settings::default();
        assert!(!settings.mqtt_enabled, "MQTT must be opt-in (R1.2)");
        assert_eq!(settings.mqtt_port, 1883);
        assert_eq!(settings.mqtt_discovery_prefix, "homeassistant", "R1.3");
        assert_eq!(settings.mqtt_base_topic, "octocam");
        assert!(settings.mqtt_node_id.is_empty(), "identity is minted, not defaulted");
    }

    #[test]
    fn settings_written_before_mqtt_existed_still_load() {
        // A stored file from before this feature has none of the mqtt_* keys.
        let raw = serde_json::json!({ "device_name": "OctoCam", "motion_enabled": true });
        let map = raw.as_object().expect("object").clone();
        let settings = validate_map(&map);
        assert_eq!(settings.device_name, "OctoCam");
        assert!(!settings.mqtt_enabled);
        assert_eq!(settings.mqtt_discovery_prefix, "homeassistant");
    }

    #[test]
    fn out_of_range_port_is_rejected_so_stored_settings_are_untouched() {
        for bad in [0, 65536, -1] {
            let map = serde_json::json!({ "mqtt_port": bad })
                .as_object()
                .expect("object")
                .clone();
            assert!(
                validate_mqtt_submission(&map).is_err(),
                "port {bad} must be rejected (R1.4)"
            );
        }
        let ok = serde_json::json!({ "mqtt_port": 8883 })
            .as_object()
            .expect("object")
            .clone();
        assert!(validate_mqtt_submission(&ok).is_ok());
    }

    #[test]
    fn enabling_mqtt_without_a_host_is_rejected() {
        let map = serde_json::json!({ "mqtt_enabled": true, "mqtt_host": "   " })
            .as_object()
            .expect("object")
            .clone();
        assert!(validate_mqtt_submission(&map).is_err(), "R1.5");

        // Disabled with an empty host is a legitimate half-filled form.
        let disabled = serde_json::json!({ "mqtt_enabled": false, "mqtt_host": "" })
            .as_object()
            .expect("object")
            .clone();
        assert!(validate_mqtt_submission(&disabled).is_ok());
    }

    #[test]
    fn public_settings_hides_the_broker_password_but_reports_that_one_exists() {
        let mut settings = Settings::default();
        settings.mqtt_password = Secret::from("hunter2");
        let value = public_settings(&settings);
        let map = value.as_object().expect("object");

        assert!(map.get("mqtt_password").is_none(), "R6.1");
        assert_eq!(map.get("mqtt_password_set"), Some(&Value::Bool(true)), "R6.2");
        assert!(
            !serde_json::to_string(&value).unwrap().contains("hunter2"),
            "the password must not survive anywhere in the response body (R6.1)"
        );

        let empty = public_settings(&Settings::default());
        assert_eq!(
            empty.as_object().unwrap().get("mqtt_password_set"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn omitting_the_password_preserves_it_and_clearing_it_empties_it() {
        // The update handler merges stored settings under the incoming patch,
        // so an omitted key arrives carrying the stored value.
        let mut stored = Settings::default();
        stored.mqtt_password = Secret::from("stored-secret");
        let mut merged = serde_json::to_value(&stored)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        merged.insert("mqtt_host".to_string(), Value::String("broker.local".into()));
        assert_eq!(validate_map(&merged).mqtt_password.expose(), "stored-secret", "R6.3");

        merged.insert("mqtt_password".to_string(), Value::String(String::new()));
        assert_eq!(validate_map(&merged).mqtt_password.expose(), "", "R6.4");
    }

    #[test]
    fn the_broker_password_cannot_leak_through_a_debug_dump() {
        // The realistic leak is not a literal password in a format string, it
        // is someone adding `tracing::debug!("{:?}", settings)` while chasing
        // an unrelated bug. The type has to refuse, not the author.
        let mut settings = Settings::default();
        settings.mqtt_password = Secret::from("hunter2");
        let dumped = format!("{settings:?}");
        assert!(
            !dumped.contains("hunter2"),
            "Debug output must not carry the credential (R6.6): {dumped}"
        );
        assert!(dumped.contains("Secret(redacted)"));
    }

    #[test]
    fn node_id_is_minted_once_and_then_stable() {
        let mut settings = Settings::default();
        assert!(ensure_mqtt_node_id(&mut settings), "first call mints an id");
        let first = settings.mqtt_node_id.clone();
        assert!(!first.is_empty());

        assert!(!ensure_mqtt_node_id(&mut settings), "second call is a no-op");
        assert_eq!(settings.mqtt_node_id, first, "identity must be stable (R2.3)");

        // Renaming the device must not disturb it.
        settings.device_name = "Renamed".to_string();
        assert!(!ensure_mqtt_node_id(&mut settings));
        assert_eq!(settings.mqtt_node_id, first);
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
        assert_eq!(
            settings.scheduled_service_restart_days,
            "Mon,Tue,Wed,Thu,Fri,Sat,Sun"
        );
        assert!(settings.scheduled_reboot_enabled);
        assert_eq!(settings.scheduled_reboot_time, "23:59");
        assert_eq!(
            settings.scheduled_reboot_days,
            "Mon,Tue,Wed,Thu,Fri,Sat,Sun"
        );

        map.insert(
            "scheduled_service_restart_day_mon".into(),
            Value::String("true".into()),
        );
        map.insert(
            "scheduled_service_restart_day_tue".into(),
            Value::String("false".into()),
        );
        map.insert(
            "scheduled_service_restart_day_fri".into(),
            Value::String("on".into()),
        );
        map.insert(
            "scheduled_reboot_days".into(),
            Value::String("Sun,Wed,nope,Mon".into()),
        );
        let settings = validate_map(&map);
        assert_eq!(settings.scheduled_service_restart_days, "Mon,Fri");
        assert_eq!(settings.scheduled_reboot_days, "Mon,Wed,Sun");

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

    #[test]
    fn hksv_requires_motion() {
        // HKSV on + motion off  -> HKSV forced off.
        let mut s = Settings {
            hksv_enabled: true,
            motion_enabled: false,
            ..Settings::default()
        };
        enforce_hksv_requires_motion(&mut s);
        assert!(!s.hksv_enabled, "HKSV must not stay enabled without motion");

        // HKSV on + motion on -> stays on.
        let mut s2 = Settings {
            hksv_enabled: true,
            motion_enabled: true,
            ..Settings::default()
        };
        enforce_hksv_requires_motion(&mut s2);
        assert!(s2.hksv_enabled);
    }

    #[test]
    fn public_settings_emits_motion_zones_as_decimal_string() {
        // u64::MAX as a bare JSON number would lose precision once it hits a
        // JS Number (max safe integer is 2^53-1) — public_settings() must
        // serialize it as a decimal string instead.
        let settings = Settings {
            motion_zones: u64::MAX,
            ..Settings::default()
        };
        let value = public_settings(&settings);
        assert_eq!(
            value.get("motion_zones"),
            Some(&Value::String(u64::MAX.to_string()))
        );

        let settings = Settings {
            motion_zones: 42,
            ..Settings::default()
        };
        let value = public_settings(&settings);
        assert_eq!(
            value.get("motion_zones"),
            Some(&Value::String("42".to_string()))
        );
    }

    #[test]
    fn parses_hksv_enabled() {
        let mut map = Map::new();
        map.insert("hksv_enabled".into(), Value::String("true".into()));
        let s = validate_map(&map);
        assert!(s.hksv_enabled, "hksv_enabled should parse from the form map");

        // Absent key keeps the default (false).
        let s_default = validate_map(&Map::new());
        assert!(!s_default.hksv_enabled);
    }
}
