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
}
