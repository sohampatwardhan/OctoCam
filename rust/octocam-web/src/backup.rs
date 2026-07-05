//! OctoCam configuration backup envelope: serialize the portable settings
//! fields + authorized SSH public keys to a versioned JSON file, and restore
//! one by overlaying only the portable fields onto the current device settings.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::settings::{self, Settings};

/// Current backup schema version. Restore rejects a file whose version is
/// greater than this.
pub const BACKUP_VERSION: u32 = 1;

/// The portable settings field names — the single source of truth for what
/// `build_backup` exports and what `parse_restore` overlays. This is an explicit
/// allow-list: a field added to `Settings` later is NOT ported until it is
/// listed here (or added to `EXCLUDED_FIELDS`). `field_lists_cover_all_settings`
/// fails until every field is classified.
pub const PORTABLE_FIELDS: &[&str] = &[
    "device_name",
    "room",
    "camera_label",
    "camera_enabled",
    "resolution_width",
    "resolution_height",
    "framerate",
    "bitrate_kbps",
    "rtsp_enabled",
    "rtsp_max_clients",
    "rtsp_path",
    "sub_stream_enabled",
    "sub_resolution_width",
    "sub_resolution_height",
    "sub_framerate",
    "sub_bitrate_kbps",
    "sub_rtsp_max_clients",
    "sub_rtsp_path",
    "rotation",
    "hflip",
    "vflip",
    "brightness",
    "contrast",
    "homekit_enabled",
    "matter_enabled",
    "motion_enabled",
    "motion_sensitivity",
];

/// Fields deliberately NOT ported — preserved from the target device on restore.
/// `admin_password_hash` is never even written to the file.
pub const EXCLUDED_FIELDS: &[&str] = &[
    "admin_password_hash",
    "setup_complete",
    "homekit_paired",
    "wifi_ssid",
];

/// Serialize a `Settings` to a JSON object map. Infallible in practice —
/// `Settings` is all primitives/strings — but returns an empty map rather than
/// panicking if serialization ever changes shape.
fn settings_map(settings: &Settings) -> Map<String, Value> {
    match serde_json::to_value(settings) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// The downloadable backup envelope. `settings` holds only the portable fields.
#[derive(Serialize)]
pub struct Backup {
    pub octocam_backup_version: u32,
    /// Unix epoch seconds (see spec: no time crate — an integer needs no dep).
    pub exported_at: u64,
    /// Informational copy of the device name for humans reading the file.
    pub device_name: String,
    pub settings: Map<String, Value>,
    pub ssh_authorized_keys: Vec<String>,
}

/// Build a backup envelope from the current settings and authorized key lines.
pub fn build_backup(settings: &Settings, exported_at: u64, ssh_authorized_keys: Vec<String>) -> Backup {
    let full = settings_map(settings);
    let mut portable = Map::new();
    for &field in PORTABLE_FIELDS {
        if let Some(value) = full.get(field) {
            portable.insert(field.to_string(), value.clone());
        }
    }
    Backup {
        octocam_backup_version: BACKUP_VERSION,
        exported_at,
        device_name: settings.device_name.clone(),
        settings: portable,
        ssh_authorized_keys,
    }
}

/// Download filename: `octocam-backup-<slug>-<epoch>.json`. The device name is
/// slugified (lowercase ascii-alnum, other runs collapse to nothing via the
/// per-char map + trim). Epoch is used in place of a date to avoid a time crate.
pub fn backup_filename(device_name: &str, exported_at: u64) -> String {
    let mapped: String = device_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug: String = mapped
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(40).collect();
    let slug = if slug.is_empty() { "octocam".to_string() } else { slug };
    format!("octocam-backup-{slug}-{exported_at}.json")
}

/// Why a restore upload was rejected. Coarse on purpose — the handler maps this
/// to a redirect query param; detailed causes are not surfaced to the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreError {
    /// Not valid JSON, not a JSON object, or `settings` is not an object.
    BadJson,
    /// `octocam_backup_version` is missing or newer than `BACKUP_VERSION`.
    BadVersion,
}

/// Parse and validate an uploaded backup file's bytes against the current
/// on-disk settings.
///
/// Returns the settings to save (portable fields overlaid on `current`, all
/// clamped/sanitized by `validate_map`, with `enforce_matter_requires_admin`
/// applied) and the raw SSH key strings to merge.
///
/// Crucially: the map handed to `validate_map` is seeded from `current` and only
/// the portable keys are overlaid from the upload. `validate_map` starts from
/// `Settings::default()` for any absent field, so validating the upload alone
/// would reset the excluded fields (empty admin hash, etc.). Seeding from current
/// keeps the excluded fields correct and makes them unreachable from the upload.
pub fn parse_restore(bytes: &[u8], current: &Settings) -> Result<(Settings, Vec<String>), RestoreError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| RestoreError::BadJson)?;
    let Value::Object(root) = value else {
        return Err(RestoreError::BadJson);
    };

    match root.get("octocam_backup_version").and_then(Value::as_u64) {
        Some(version) if version <= BACKUP_VERSION as u64 => {}
        _ => return Err(RestoreError::BadVersion),
    }

    let Some(Value::Object(uploaded)) = root.get("settings") else {
        return Err(RestoreError::BadJson);
    };

    // Seed from current settings, then overlay ONLY the portable keys.
    let mut seed = settings_map(current);
    for &field in PORTABLE_FIELDS {
        if let Some(value) = uploaded.get(field) {
            seed.insert(field.to_string(), value.clone());
        }
    }

    let mut restored = settings::validate_map(&seed);
    settings::enforce_matter_requires_admin(&mut restored);

    let keys = match root.get("ssh_authorized_keys") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };

    Ok((restored, keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn field_lists_cover_all_settings() {
        let all: BTreeSet<String> = settings_map(&Settings::default())
            .keys()
            .cloned()
            .collect();
        let classified: BTreeSet<String> = PORTABLE_FIELDS
            .iter()
            .chain(EXCLUDED_FIELDS.iter())
            .map(|field| field.to_string())
            .collect();
        assert_eq!(
            all, classified,
            "every Settings field must be in PORTABLE_FIELDS or EXCLUDED_FIELDS"
        );
    }

    #[test]
    fn build_backup_includes_only_portable_fields() {
        let mut settings = Settings::default();
        settings.admin_password_hash = "secret-hash".to_string();
        settings.device_name = "Nursery Cam".to_string();
        settings.homekit_paired = true;
        settings.wifi_ssid = "HomeNet".to_string();

        let backup = build_backup(&settings, 1_751_716_800, vec!["ssh-ed25519 AAAA test".to_string()]);

        assert_eq!(backup.octocam_backup_version, BACKUP_VERSION);
        assert_eq!(backup.exported_at, 1_751_716_800);
        assert_eq!(backup.device_name, "Nursery Cam");
        assert_eq!(backup.settings.get("device_name").and_then(|v| v.as_str()), Some("Nursery Cam"));
        assert!(backup.settings.get("admin_password_hash").is_none());
        assert!(backup.settings.get("homekit_paired").is_none());
        assert!(backup.settings.get("wifi_ssid").is_none());
        assert!(backup.settings.get("setup_complete").is_none());
        assert_eq!(backup.ssh_authorized_keys, vec!["ssh-ed25519 AAAA test".to_string()]);
    }

    #[test]
    fn backup_filename_slugifies_device_name() {
        assert_eq!(
            backup_filename("Nursery Cam!", 1_751_716_800),
            "octocam-backup-nursery-cam-1751716800.json"
        );
        assert_eq!(
            backup_filename("", 42),
            "octocam-backup-octocam-42.json"
        );
    }

    fn envelope(settings_json: Value, keys: Value, version: Value) -> Vec<u8> {
        let mut root = Map::new();
        root.insert("octocam_backup_version".to_string(), version);
        root.insert("settings".to_string(), settings_json);
        root.insert("ssh_authorized_keys".to_string(), keys);
        serde_json::to_vec(&Value::Object(root)).unwrap()
    }

    #[test]
    fn parse_restore_overlays_portable_and_preserves_excluded() {
        let mut current = Settings::default();
        current.admin_password_hash = "keep-me".to_string();
        current.setup_complete = true;
        current.homekit_paired = true;
        current.wifi_ssid = "TargetNet".to_string();
        current.device_name = "Old Name".to_string();

        let mut s = Map::new();
        s.insert("device_name".to_string(), Value::from("New Name"));
        s.insert("framerate".to_string(), Value::from(20));
        s.insert("admin_password_hash".to_string(), Value::from("attacker"));
        s.insert("setup_complete".to_string(), Value::from(false));
        s.insert("homekit_paired".to_string(), Value::from(false));
        s.insert("wifi_ssid".to_string(), Value::from("AttackerNet"));

        let bytes = envelope(Value::Object(s), Value::Array(vec![]), Value::from(1));
        let (restored, keys) = parse_restore(&bytes, &current).expect("valid backup");

        assert_eq!(restored.device_name, "New Name");
        assert_eq!(restored.framerate, 20);
        assert_eq!(restored.admin_password_hash, "keep-me");
        assert!(restored.setup_complete);
        assert!(restored.homekit_paired);
        assert_eq!(restored.wifi_ssid, "TargetNet");
        assert!(keys.is_empty());
    }

    #[test]
    fn parse_restore_clamps_out_of_range_values() {
        let current = Settings::default();
        let mut s = Map::new();
        s.insert("framerate".to_string(), Value::from(999));
        let bytes = envelope(Value::Object(s), Value::Array(vec![]), Value::from(1));
        let (restored, _keys) = parse_restore(&bytes, &current).unwrap();
        assert_eq!(restored.framerate, 60);
    }

    #[test]
    fn parse_restore_reads_ssh_keys_array() {
        let current = Settings::default();
        let keys = Value::Array(vec![Value::from("ssh-ed25519 AAAA a"), Value::from("ssh-rsa BBBB b")]);
        let bytes = envelope(Value::Object(Map::new()), keys, Value::from(1));
        let (_restored, keys) = parse_restore(&bytes, &current).unwrap();
        assert_eq!(keys, vec!["ssh-ed25519 AAAA a".to_string(), "ssh-rsa BBBB b".to_string()]);
    }

    #[test]
    fn parse_restore_rejects_bad_version_and_shape() {
        let current = Settings::default();
        let bytes = envelope(Value::Object(Map::new()), Value::Array(vec![]), Value::from(2));
        assert!(matches!(parse_restore(&bytes, &current), Err(RestoreError::BadVersion)));
        let mut root = Map::new();
        root.insert("settings".to_string(), Value::Object(Map::new()));
        let bytes = serde_json::to_vec(&Value::Object(root)).unwrap();
        assert!(matches!(parse_restore(&bytes, &current), Err(RestoreError::BadVersion)));
        let bytes = serde_json::to_vec(&Value::from("nope")).unwrap();
        assert!(matches!(parse_restore(&bytes, &current), Err(RestoreError::BadJson)));
        assert!(matches!(parse_restore(b"{not json", &current), Err(RestoreError::BadJson)));
    }

    #[test]
    fn parse_restore_matter_off_when_no_admin_password() {
        let current = Settings::default();
        let mut s = Map::new();
        s.insert("matter_enabled".to_string(), Value::from(true));
        let bytes = envelope(Value::Object(s), Value::Array(vec![]), Value::from(1));
        let (restored, _keys) = parse_restore(&bytes, &current).unwrap();
        assert!(!restored.matter_enabled);
    }
}
