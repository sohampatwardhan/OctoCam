from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DEFAULT_SETTINGS: dict[str, Any] = {
    "setup_complete": False,
    "admin_password_hash": "",
    "device_name": "OctoCam",
    "room": "Living Room",
    "camera_label": "OctoCam",
    "wifi_ssid": "",
    "camera_enabled": True,
    "resolution_width": 1280,
    "resolution_height": 720,
    "framerate": 15,
    "bitrate_kbps": 2500,
    "rtsp_enabled": True,
    "rtsp_max_clients": 1,
    "rtsp_path": "main",
    "sub_stream_enabled": True,
    "sub_resolution_width": 640,
    "sub_resolution_height": 480,
    "sub_framerate": 10,
    "sub_bitrate_kbps": 600,
    "sub_rtsp_max_clients": 2,
    "sub_rtsp_path": "sub",
    "rotation": 0,
    "hflip": False,
    "vflip": False,
    "brightness": 0,
    "contrast": 1.0,
    "homekit_enabled": False,
    "homekit_paired": False,
    "motion_enabled": False,
    "motion_sensitivity": 50,
}

SENSITIVE_FIELDS = {"admin_password_hash"}

RESOLUTION_PRESETS: tuple[dict[str, int | str], ...] = (
    {"value": "640x480", "label": "640 x 480 (4:3)", "width": 640, "height": 480},
    {"value": "800x600", "label": "800 x 600 (4:3)", "width": 800, "height": 600},
    {"value": "1024x768", "label": "1024 x 768 (4:3)", "width": 1024, "height": 768},
    {"value": "1296x972", "label": "1296 x 972 (4:3)", "width": 1296, "height": 972},
    {"value": "1640x1232", "label": "1640 x 1232 (4:3)", "width": 1640, "height": 1232},
    {"value": "1920x1440", "label": "1920 x 1440 (4:3)", "width": 1920, "height": 1440},
    {"value": "3280x2464", "label": "3280 x 2464 (4:3 full sensor)", "width": 3280, "height": 2464},
    {"value": "1280x720", "label": "1280 x 720 (16:9 cropped)", "width": 1280, "height": 720},
    {"value": "1920x1080", "label": "1920 x 1080 (16:9 cropped)", "width": 1920, "height": 1080},
)

SUB_RESOLUTION_PRESETS: tuple[dict[str, int | str], ...] = (
    {"value": "320x240", "label": "320 x 240 (4:3)", "width": 320, "height": 240},
    {"value": "640x480", "label": "640 x 480 (4:3)", "width": 640, "height": 480},
    {"value": "800x600", "label": "800 x 600 (4:3)", "width": 800, "height": 600},
    {"value": "1024x768", "label": "1024 x 768 (4:3)", "width": 1024, "height": 768},
    {"value": "640x360", "label": "640 x 360 (16:9 cropped)", "width": 640, "height": 360},
    {"value": "854x480", "label": "854 x 480 (16:9 cropped)", "width": 854, "height": 480},
)


@dataclass(frozen=True)
class FieldSpec:
    name: str
    value_type: type
    minimum: int | float | None = None
    maximum: int | float | None = None
    choices: tuple[Any, ...] | None = None


FIELD_SPECS = {
    "setup_complete": FieldSpec("setup_complete", bool),
    "admin_password_hash": FieldSpec("admin_password_hash", str),
    "device_name": FieldSpec("device_name", str),
    "room": FieldSpec("room", str),
    "camera_label": FieldSpec("camera_label", str),
    "wifi_ssid": FieldSpec("wifi_ssid", str),
    "camera_enabled": FieldSpec("camera_enabled", bool),
    "resolution_width": FieldSpec("resolution_width", int, 320, 3280),
    "resolution_height": FieldSpec("resolution_height", int, 240, 2464),
    "framerate": FieldSpec("framerate", int, 1, 60),
    "bitrate_kbps": FieldSpec("bitrate_kbps", int, 250, 25000),
    "rtsp_enabled": FieldSpec("rtsp_enabled", bool),
    "rtsp_max_clients": FieldSpec("rtsp_max_clients", int, 1, 4),
    "rtsp_path": FieldSpec("rtsp_path", str),
    "sub_stream_enabled": FieldSpec("sub_stream_enabled", bool),
    "sub_resolution_width": FieldSpec("sub_resolution_width", int, 320, 1920),
    "sub_resolution_height": FieldSpec("sub_resolution_height", int, 240, 1440),
    "sub_framerate": FieldSpec("sub_framerate", int, 1, 30),
    "sub_bitrate_kbps": FieldSpec("sub_bitrate_kbps", int, 150, 5000),
    "sub_rtsp_max_clients": FieldSpec("sub_rtsp_max_clients", int, 1, 4),
    "sub_rtsp_path": FieldSpec("sub_rtsp_path", str),
    "rotation": FieldSpec("rotation", int, choices=(0, 90, 180, 270)),
    "hflip": FieldSpec("hflip", bool),
    "vflip": FieldSpec("vflip", bool),
    "brightness": FieldSpec("brightness", int, -100, 100),
    "contrast": FieldSpec("contrast", float, 0.0, 4.0),
    "homekit_enabled": FieldSpec("homekit_enabled", bool),
    "homekit_paired": FieldSpec("homekit_paired", bool),
    "motion_enabled": FieldSpec("motion_enabled", bool),
    "motion_sensitivity": FieldSpec("motion_sensitivity", int, 1, 100),
}


def default_config_path() -> Path:
    configured = os.environ.get("OCTOCAM_CONFIG_PATH")
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".config" / "octocam" / "settings.json"


def load_settings(path: Path | None = None) -> dict[str, Any]:
    config_path = path or default_config_path()
    settings = DEFAULT_SETTINGS.copy()

    try:
        with config_path.open("r", encoding="utf-8") as handle:
            raw = json.load(handle)
    except FileNotFoundError:
        return settings
    except json.JSONDecodeError:
        return settings

    if isinstance(raw, dict):
        settings.update(validate_settings(raw))
    return settings


def save_settings(settings: dict[str, Any], path: Path | None = None) -> None:
    config_path = path or default_config_path()
    config_path.parent.mkdir(parents=True, exist_ok=True)

    cleaned = DEFAULT_SETTINGS.copy()
    cleaned.update(validate_settings(settings))

    with config_path.open("w", encoding="utf-8") as handle:
        json.dump(cleaned, handle, indent=2, sort_keys=True)
        handle.write("\n")


def public_settings(settings: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in settings.items() if key not in SENSITIVE_FIELDS}


def validate_settings(raw: dict[str, Any]) -> dict[str, Any]:
    raw = raw.copy()
    apply_resolution_preset(raw, "resolution", "resolution_width", "resolution_height", RESOLUTION_PRESETS)
    apply_resolution_preset(
        raw,
        "sub_resolution",
        "sub_resolution_width",
        "sub_resolution_height",
        SUB_RESOLUTION_PRESETS,
    )

    validated: dict[str, Any] = {}
    for key, spec in FIELD_SPECS.items():
        if key not in raw:
            continue
        validated[key] = coerce_value(raw[key], spec)
    return validated


def apply_resolution_preset(
    raw: dict[str, Any],
    field: str,
    width_field: str,
    height_field: str,
    presets: tuple[dict[str, int | str], ...],
) -> None:
    value = raw.pop(field, "")
    if not isinstance(value, str):
        return

    for preset in presets:
        if preset["value"] == value:
            raw[width_field] = preset["width"]
            raw[height_field] = preset["height"]
            return


def coerce_value(value: Any, spec: FieldSpec) -> Any:
    if spec.value_type is bool:
        coerced = coerce_bool(value)
    elif spec.value_type is int:
        try:
            coerced = int(value)
        except (TypeError, ValueError):
            return DEFAULT_SETTINGS[spec.name]
    elif spec.value_type is float:
        try:
            coerced = float(value)
        except (TypeError, ValueError):
            return DEFAULT_SETTINGS[spec.name]
    else:
        coerced = str(value).strip()

    if isinstance(coerced, str):
        if spec.name in SENSITIVE_FIELDS:
            return coerced[:256]
        if spec.name in {"rtsp_path", "sub_rtsp_path"}:
            coerced = coerced.strip().strip("/")
            coerced = "".join(char for char in coerced if char.isalnum() or char in "-_./")
            return coerced[:80] or DEFAULT_SETTINGS[spec.name]
        return coerced[:80] or DEFAULT_SETTINGS[spec.name]

    if spec.choices is not None and coerced not in spec.choices:
        return DEFAULT_SETTINGS[spec.name]

    if spec.minimum is not None and coerced < spec.minimum:
        return spec.minimum

    if spec.maximum is not None and coerced > spec.maximum:
        return spec.maximum

    return coerced


def coerce_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.lower() in {"1", "true", "yes", "on"}
    return bool(value)
