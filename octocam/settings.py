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
    "rtsp_path": "octocam",
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
    "rtsp_path": FieldSpec("rtsp_path", str),
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
    validated: dict[str, Any] = {}
    for key, spec in FIELD_SPECS.items():
        if key not in raw:
            continue
        validated[key] = coerce_value(raw[key], spec)
    return validated


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
        if spec.name == "rtsp_path":
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
