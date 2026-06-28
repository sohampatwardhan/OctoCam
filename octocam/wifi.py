from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path
from typing import Any


DEFAULT_CACHE = {"scanned_at": None, "networks": []}


def default_cache_path() -> Path:
    configured = os.environ.get("OCTOCAM_WIFI_CACHE_PATH")
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".cache" / "octocam" / "wifi-networks.json"


def load_network_cache(path: Path | None = None) -> dict[str, Any]:
    cache_path = path or default_cache_path()
    try:
        with cache_path.open("r", encoding="utf-8") as handle:
            raw = json.load(handle)
    except (FileNotFoundError, json.JSONDecodeError):
        return DEFAULT_CACHE.copy()

    if not isinstance(raw, dict):
        return DEFAULT_CACHE.copy()

    networks = raw.get("networks", [])
    if not isinstance(networks, list):
        networks = []

    return {
        "scanned_at": raw.get("scanned_at"),
        "networks": [network for network in networks if valid_cached_network(network)],
    }


def save_network_cache(networks: list[dict[str, Any]], path: Path | None = None) -> None:
    cache_path = path or default_cache_path()
    cache_path.parent.mkdir(parents=True, exist_ok=True)
    data = {"scanned_at": int(time.time()), "networks": networks}

    with cache_path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def scan_and_cache_networks(path: Path | None = None) -> dict[str, Any]:
    networks = scan_networks()
    save_network_cache(networks, path)
    return load_network_cache(path)


def scan_networks() -> list[dict[str, Any]]:
    result = subprocess.run(
        ["nmcli", "-t", "-f", "SSID,SECURITY,SIGNAL", "dev", "wifi", "list", "--rescan", "yes"],
        check=False,
        capture_output=True,
        text=True,
        timeout=20,
    )

    if result.returncode != 0:
        raise RuntimeError((result.stderr or result.stdout).strip() or "Wi-Fi scan failed.")

    return dedupe_networks(parse_nmcli_wifi_list(result.stdout))


def connect_to_network(ssid: str, password: str, security: str) -> tuple[bool, str]:
    ssid = ssid.strip()
    security = normalize_security(security)

    if not ssid:
        return False, "Missing Wi-Fi network name."

    command = ["nmcli", "dev", "wifi", "connect", ssid]
    if security != "open":
        if not password:
            return False, f"{security.upper()} network requires a password."
        command.extend(["password", password])

    result = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        timeout=45,
    )
    output = (result.stdout or result.stderr).strip()
    if result.returncode == 0:
        disable_setup_ap()
    return result.returncode == 0, output or "NetworkManager returned no output."


def parse_nmcli_wifi_list(output: str) -> list[dict[str, Any]]:
    networks = []
    for line in output.splitlines():
        if not line:
            continue

        fields = split_escaped(line)
        if len(fields) < 3:
            continue

        ssid = unescape_nmcli(fields[0]).strip()
        if not ssid:
            continue

        raw_security = unescape_nmcli(fields[1]).strip()
        signal = parse_signal(fields[2])
        networks.append(
            {
                "ssid": ssid,
                "security": normalize_security(raw_security),
                "raw_security": raw_security,
                "signal": signal,
            }
        )
    return networks


def disable_setup_ap() -> None:
    ap_ssid = os.environ.get("OCTOCAM_SETUP_AP_SSID", "OctoCam-Setup")
    subprocess.run(
        ["nmcli", "connection", "modify", ap_ssid, "connection.autoconnect", "no"],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    subprocess.run(
        ["nmcli", "connection", "down", ap_ssid],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )


def dedupe_networks(networks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    best: dict[str, dict[str, Any]] = {}
    for network in networks:
        ssid = network["ssid"]
        current = best.get(ssid)
        if current is None or network["signal"] > current["signal"]:
            best[ssid] = network

    return sorted(best.values(), key=lambda item: (-item["signal"], item["ssid"].lower()))


def normalize_security(value: str) -> str:
    normalized = value.upper().replace("-", "").replace("_", "")
    if not normalized or normalized == "--":
        return "open"
    if "WPA3" in normalized or "SAE" in normalized:
        return "wpa3"
    if "WPA2" in normalized or "RSN" in normalized:
        return "wpa2"
    if "WPA" in normalized:
        return "wpa"
    if "WEP" in normalized:
        return "wep"
    return "unknown"


def parse_signal(value: str) -> int:
    try:
        return max(0, min(100, int(value)))
    except ValueError:
        return 0


def split_escaped(value: str) -> list[str]:
    fields: list[str] = []
    current: list[str] = []
    escaped = False

    for char in value:
        if escaped:
            current.append(char)
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == ":":
            fields.append("".join(current))
            current = []
        else:
            current.append(char)

    fields.append("".join(current))
    return fields


def unescape_nmcli(value: str) -> str:
    return value.replace("\\:", ":").replace("\\\\", "\\")


def valid_cached_network(network: Any) -> bool:
    return (
        isinstance(network, dict)
        and isinstance(network.get("ssid"), str)
        and network.get("security") in {"open", "wep", "wpa", "wpa2", "wpa3", "unknown"}
    )
