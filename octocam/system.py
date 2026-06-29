from __future__ import annotations

import shutil
import socket
import subprocess
import time
import os
import re
from pathlib import Path
from typing import Any, Callable


_CACHE: dict[str, tuple[float, Any]] = {}


def status() -> dict[str, Any]:
    return {
        "hostname": socket.gethostname(),
        "ip_addresses": ip_addresses(),
        "uptime": uptime_text(),
        "cpu_temp_c": cpu_temp_c(),
        "resources": cached("resources", 5, resource_status),
        "wifi": cached("wifi", 5, wifi_status),
        "camera": cached("camera", 10, camera_status),
        "services": {
            "octocam_web": cached("svc:octocam-web", 5, lambda: service_status("octocam-web")),
            "homebridge": cached("svc:homebridge", 5, lambda: service_status("homebridge")),
            "rtsp": cached("svc:octocam-rtsp", 5, lambda: service_status("octocam-rtsp")),
        },
        "logs": cached_list("logs:octocam-web", 10, lambda: service_logs("octocam-web")),
    }


def cached(key: str, ttl_seconds: int, loader: Callable[[], Any]) -> Any:
    now = time.monotonic()
    if key in _CACHE:
        cached_at, value = _CACHE[key]
        if now - cached_at < ttl_seconds:
            return value

    try:
        value = loader()
    except Exception as error:
        value = {"state": "error", "message": str(error)}

    _CACHE[key] = (now, value)
    return value


def cached_list(key: str, ttl_seconds: int, loader: Callable[[], list[str]]) -> list[str]:
    value = cached(key, ttl_seconds, loader)
    if isinstance(value, list):
        return value
    return []


def ip_addresses() -> list[str]:
    try:
        result = subprocess.run(
            ["hostname", "-I"],
            check=False,
            capture_output=True,
            text=True,
            timeout=2,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []

    return [item for item in result.stdout.split() if item]


def cpu_temp_c() -> float | None:
    path = Path("/sys/class/thermal/thermal_zone0/temp")
    try:
        return round(int(path.read_text(encoding="utf-8").strip()) / 1000, 1)
    except (FileNotFoundError, PermissionError, ValueError):
        return None


def resource_status() -> dict[str, Any]:
    memory = memory_status()
    return {
        "cpu_usage_percent": cpu_usage_percent(),
        "load_average": load_average(),
        "memory": memory,
        "memory_summary": memory_summary(memory),
    }


def cpu_usage_percent() -> float | None:
    first = read_cpu_times()
    if first is None:
        return None
    time.sleep(0.12)
    second = read_cpu_times()
    if second is None:
        return None

    idle_delta = second["idle"] - first["idle"]
    total_delta = second["total"] - first["total"]
    if total_delta <= 0:
        return None
    return round((1 - idle_delta / total_delta) * 100, 1)


def read_cpu_times() -> dict[str, int] | None:
    try:
        fields = Path("/proc/stat").read_text(encoding="utf-8").splitlines()[0].split()
    except (FileNotFoundError, PermissionError, IndexError):
        return None

    if not fields or fields[0] != "cpu":
        return None

    try:
        values = [int(value) for value in fields[1:]]
    except ValueError:
        return None

    idle = values[3] + (values[4] if len(values) > 4 else 0)
    return {"idle": idle, "total": sum(values)}


def load_average() -> str | None:
    try:
        one, five, fifteen = os.getloadavg()
    except OSError:
        return None
    return f"{one:.2f}, {five:.2f}, {fifteen:.2f}"


def memory_status() -> dict[str, Any]:
    values: dict[str, int] = {}
    try:
        lines = Path("/proc/meminfo").read_text(encoding="utf-8").splitlines()
    except (FileNotFoundError, PermissionError):
        return {}

    for line in lines:
        key, separator, rest = line.partition(":")
        if not separator:
            continue
        parts = rest.strip().split()
        if not parts:
            continue
        try:
            values[key] = int(parts[0])
        except ValueError:
            continue

    total = values.get("MemTotal", 0)
    available = values.get("MemAvailable", 0)
    used = max(0, total - available)
    swap_total = values.get("SwapTotal", 0)
    swap_free = values.get("SwapFree", 0)
    swap_used = max(0, swap_total - swap_free)

    return {
        "total_mb": kib_to_mb(total),
        "available_mb": kib_to_mb(available),
        "used_mb": kib_to_mb(used),
        "used_percent": round((used / total) * 100, 1) if total else None,
        "swap_total_mb": kib_to_mb(swap_total),
        "swap_used_mb": kib_to_mb(swap_used),
        "swap_used_percent": round((swap_used / swap_total) * 100, 1) if swap_total else None,
    }


def memory_summary(memory: dict[str, Any]) -> str | None:
    used = memory.get("used_mb")
    total = memory.get("total_mb")
    percent = memory.get("used_percent")
    if used is None or total is None or percent is None:
        return None
    return f"{used} / {total} MB ({percent}%)"


def kib_to_mb(value: int) -> int:
    return round(value / 1024)


def uptime_text() -> str | None:
    path = Path("/proc/uptime")
    try:
        seconds = int(float(path.read_text(encoding="utf-8").split()[0]))
    except (FileNotFoundError, PermissionError, ValueError, IndexError):
        return None

    days, remainder = divmod(seconds, 86400)
    hours, remainder = divmod(remainder, 3600)
    minutes, _ = divmod(remainder, 60)

    if days:
        return f"{days}d {hours}h {minutes}m"
    if hours:
        return f"{hours}h {minutes}m"
    return f"{minutes}m"


def wifi_status() -> dict[str, Any]:
    for detector in (nmcli_wifi_status, iw_wifi_status, wpa_cli_wifi_status):
        result = detector()
        if result and result["state"] == "connected":
            return enrich_wifi_status(result)

    if not any(shutil.which(command) for command in ("nmcli", "iw", "wpa_cli")):
        return {"ssid": None, "state": "unavailable", "message": "No Wi-Fi status tool found."}

    return {"ssid": None, "state": "disconnected", "message": "No active Wi-Fi connection."}


def nmcli_wifi_status() -> dict[str, Any] | None:
    if not shutil.which("nmcli"):
        return None

    try:
        result = subprocess.run(
            ["nmcli", "-t", "-f", "ACTIVE,SSID", "dev", "wifi"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"ssid": None, "state": "error", "message": str(error), "source": "nmcli"}

    for line in result.stdout.splitlines():
        fields = split_nmcli_fields(line)
        if len(fields) < 2:
            continue
        active, ssid = fields[0], fields[1]
        if active == "yes":
            return connected_wifi(ssid, "nmcli")

    return None


def iw_wifi_status() -> dict[str, Any] | None:
    if not shutil.which("iw"):
        return None

    for interface in wireless_interfaces():
        try:
            result = subprocess.run(
                ["iw", "dev", interface, "link"],
                check=False,
                capture_output=True,
                text=True,
                timeout=3,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            return {"ssid": None, "state": "error", "message": str(error), "source": "iw"}

        for line in result.stdout.splitlines():
            stripped = line.strip()
            connected = re.match(r"Connected to ([0-9a-fA-F:]+) \(on ([^)]+)\)", stripped)
            if connected:
                return connected_wifi("", f"iw:{connected.group(2)}", connected.group(2), connected.group(1))
            if stripped.startswith("SSID:"):
                return connected_wifi(stripped.removeprefix("SSID:").strip(), f"iw:{interface}", interface)

    return None


def wpa_cli_wifi_status() -> dict[str, Any] | None:
    if not shutil.which("wpa_cli"):
        return None

    interfaces = wireless_interfaces() or [None]
    for interface in interfaces:
        command = ["wpa_cli", "status"] if interface is None else ["wpa_cli", "-i", interface, "status"]
        try:
            result = subprocess.run(
                command,
                check=False,
                capture_output=True,
                text=True,
                timeout=3,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            return {"ssid": None, "state": "error", "message": str(error), "source": "wpa_cli"}

        fields = key_value_lines(result.stdout)
        if fields.get("wpa_state") == "COMPLETED" and fields.get("ssid"):
            return connected_wifi(fields["ssid"], f"wpa_cli:{interface or 'default'}", interface)

    return None


def wireless_interfaces() -> list[str]:
    interfaces: list[str] = []

    if shutil.which("iw"):
        try:
            result = subprocess.run(
                ["iw", "dev"],
                check=False,
                capture_output=True,
                text=True,
                timeout=3,
            )
        except (OSError, subprocess.TimeoutExpired):
            result = None

        if result:
            for line in result.stdout.splitlines():
                stripped = line.strip()
                if stripped.startswith("Interface "):
                    interfaces.append(stripped.removeprefix("Interface ").strip())

    if not interfaces:
        net_path = Path("/sys/class/net")
        try:
            interfaces = [
                path.name
                for path in net_path.iterdir()
                if path.name.startswith(("wlan", "wl"))
            ]
        except (FileNotFoundError, PermissionError):
            interfaces = []

    return interfaces


def connected_wifi(
    ssid: str,
    source: str,
    interface: str | None = None,
    bssid: str | None = None,
) -> dict[str, Any]:
    return {
        "ssid": ssid or None,
        "state": "connected",
        "message": "Connected",
        "source": source,
        "interface": interface,
        "bssid": bssid,
    }


def enrich_wifi_status(status: dict[str, Any]) -> dict[str, Any]:
    interface = status.get("interface")
    if not interface:
        interfaces = wireless_interfaces()
        interface = interfaces[0] if interfaces else None

    if not interface:
        return status

    enriched = status.copy()
    enriched.update({key: value for key, value in iw_link_details(interface).items() if value is not None})
    enriched.update({key: value for key, value in wpa_status_details(interface).items() if value is not None})
    enriched.update({key: value for key, value in iw_interface_details(interface).items() if value is not None})
    enriched.update({key: value for key, value in route_details().items() if value is not None})
    enriched["interface"] = interface

    frequency = enriched.get("frequency_mhz")
    if isinstance(frequency, int):
        enriched["channel"] = frequency_to_channel(frequency)
        enriched["band"] = frequency_band(frequency)

    generation = enriched.get("wifi_generation")
    if generation is not None:
        enriched["wifi_generation_label"] = wifi_generation_label(str(generation))

    return enriched


def iw_link_details(interface: str) -> dict[str, Any]:
    if not shutil.which("iw"):
        return {}

    try:
        result = subprocess.run(
            ["iw", "dev", interface, "link"],
            check=False,
            capture_output=True,
            text=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}

    details: dict[str, Any] = {}
    for line in result.stdout.splitlines():
        stripped = line.strip()
        connected = re.match(r"Connected to ([0-9a-fA-F:]+) \(on ([^)]+)\)", stripped)
        if connected:
            details["bssid"] = connected.group(1)
            details["interface"] = connected.group(2)
        elif stripped.startswith("SSID:"):
            details["ssid"] = stripped.removeprefix("SSID:").strip()
        elif stripped.startswith("freq:"):
            details["frequency_mhz"] = parse_int(stripped.removeprefix("freq:").strip().split(".", 1)[0])
        elif stripped.startswith("signal:"):
            details["signal_dbm"] = stripped.removeprefix("signal:").strip()
        elif stripped.startswith("rx bitrate:"):
            details["rx_bitrate"] = stripped.removeprefix("rx bitrate:").strip()
        elif stripped.startswith("tx bitrate:"):
            details["tx_bitrate"] = stripped.removeprefix("tx bitrate:").strip()
    return details


def wpa_status_details(interface: str) -> dict[str, Any]:
    if not shutil.which("wpa_cli"):
        return {}

    try:
        result = subprocess.run(
            ["wpa_cli", "-i", interface, "status"],
            check=False,
            capture_output=True,
            text=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}

    fields = key_value_lines(result.stdout)
    return {
        "bssid": fields.get("bssid"),
        "ssid": fields.get("ssid"),
        "frequency_mhz": parse_int(fields.get("freq")),
        "wifi_generation": fields.get("wifi_generation"),
        "security": fields.get("key_mgmt"),
        "pairwise_cipher": fields.get("pairwise_cipher"),
        "group_cipher": fields.get("group_cipher"),
        "ip_address": fields.get("ip_address"),
        "mac_address": fields.get("address"),
    }


def iw_interface_details(interface: str) -> dict[str, Any]:
    if not shutil.which("iw"):
        return {}

    try:
        result = subprocess.run(
            ["iw", "dev", interface, "info"],
            check=False,
            capture_output=True,
            text=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}

    details: dict[str, Any] = {}
    for line in result.stdout.splitlines():
        stripped = line.strip()
        if stripped.startswith("addr "):
            details["mac_address"] = stripped.removeprefix("addr ").strip()
        elif stripped.startswith("channel "):
            match = re.search(r"channel (\d+) \((\d+) MHz\), width: ([^,]+)", stripped)
            if match:
                details["channel"] = int(match.group(1))
                details["frequency_mhz"] = int(match.group(2))
                details["channel_width"] = match.group(3)
        elif stripped.startswith("txpower "):
            details["tx_power"] = stripped.removeprefix("txpower ").strip()
    return details


def route_details() -> dict[str, Any]:
    if not shutil.which("ip"):
        return {}

    try:
        result = subprocess.run(
            ["ip", "route", "show", "default"],
            check=False,
            capture_output=True,
            text=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}

    for line in result.stdout.splitlines():
        fields = line.split()
        if not fields or fields[0] != "default":
            continue

        details: dict[str, Any] = {}
        if "via" in fields:
            details["default_gateway"] = fields[fields.index("via") + 1]
        if "dev" in fields:
            details["default_interface"] = fields[fields.index("dev") + 1]
        return details

    return {}


def frequency_to_channel(frequency_mhz: int) -> int | None:
    if frequency_mhz == 2484:
        return 14
    if 2412 <= frequency_mhz <= 2472:
        return (frequency_mhz - 2407) // 5
    if 5000 <= frequency_mhz <= 5895:
        return (frequency_mhz - 5000) // 5
    if 5955 <= frequency_mhz <= 7115:
        return (frequency_mhz - 5950) // 5
    return None


def frequency_band(frequency_mhz: int) -> str:
    if 2400 <= frequency_mhz < 2500:
        return "2.4 GHz"
    if 5000 <= frequency_mhz < 5925:
        return "5 GHz"
    if 5925 <= frequency_mhz < 7125:
        return "6 GHz"
    return f"{frequency_mhz} MHz"


def wifi_generation_label(value: str) -> str:
    labels = {
        "4": "Wi-Fi 4 (802.11n)",
        "5": "Wi-Fi 5 (802.11ac)",
        "6": "Wi-Fi 6 (802.11ax)",
        "7": "Wi-Fi 7 (802.11be)",
    }
    return labels.get(value, f"Wi-Fi {value}")


def parse_int(value: str | None) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def key_value_lines(output: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in output.splitlines():
        key, separator, value = line.partition("=")
        if separator:
            fields[key.strip()] = value.strip()
    return fields


def split_nmcli_fields(value: str) -> list[str]:
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


def service_status(unit: str) -> dict[str, Any]:
    if not shutil.which("systemctl"):
        return {"unit": unit, "state": "unavailable", "enabled": None}

    state = systemctl_value(["is-active", unit])
    enabled = systemctl_value(["is-enabled", unit])
    return {
        "unit": unit,
        "state": state or "unknown",
        "enabled": enabled if enabled not in {"", "unknown"} else None,
    }


def set_service_enabled(unit: str, enabled: bool) -> dict[str, Any]:
    if not shutil.which("systemctl"):
        return {"unit": unit, "changed": False, "message": "systemctl not found"}

    action = "enable" if enabled else "disable"
    state_action = "start" if enabled else "stop"
    commands = (["systemctl", action, unit], ["systemctl", state_action, unit])

    for command in commands:
        try:
            result = subprocess.run(
                command,
                check=False,
                capture_output=True,
                text=True,
                timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            return {"unit": unit, "changed": False, "message": str(error)}

        if result.returncode != 0:
            message = (result.stderr or result.stdout).strip()
            return {"unit": unit, "changed": False, "message": message or "systemctl failed"}

    clear_cache(f"svc:{unit}")
    return {"unit": unit, "changed": True, "message": "ok"}


def configure_rtsp_service(settings: dict[str, Any]) -> dict[str, Any]:
    config_result = write_mediamtx_config(settings)
    service_result = set_service_enabled("octocam-rtsp", bool(settings["rtsp_enabled"]))
    return {"config": config_result, "service": service_result}


def write_mediamtx_config(settings: dict[str, Any]) -> dict[str, Any]:
    config_path = Path(os.environ.get("OCTOCAM_MEDIAMTX_CONFIG_PATH", "/etc/mediamtx.yml"))
    path_sections = [
        mediamtx_camera_path(
            str(settings["rtsp_path"]).strip("/") or "main",
            secondary=False,
            width=int(settings["resolution_width"]),
            height=int(settings["resolution_height"]),
            fps=int(settings["framerate"]),
            bitrate_kbps=int(settings["bitrate_kbps"]),
            max_readers=int(settings.get("rtsp_max_clients", 1)),
        )
    ]

    if settings.get("sub_stream_enabled", True):
        path_sections.append(
            mediamtx_camera_path(
                str(settings["sub_rtsp_path"]).strip("/") or "sub",
                secondary=True,
                width=int(settings["sub_resolution_width"]),
                height=int(settings["sub_resolution_height"]),
                fps=int(settings["sub_framerate"]),
                bitrate_kbps=int(settings["sub_bitrate_kbps"]),
                max_readers=int(settings.get("sub_rtsp_max_clients", 2)),
            )
        )

    content = "\n".join(
        (
            "logLevel: info",
            "",
            "rtsp: true",
            "rtspAddress: :8554",
            "rtspTransports: [tcp]",
            "",
            "rtmp: false",
            "hls: true",
            "hlsAddress: :8888",
            "hlsAllowOrigins: ['*']",
            "hlsVariant: mpegts",
            "",
            "webrtc: true",
            "webrtcAddress: :8889",
            "webrtcAllowOrigins: ['*']",
            "srt: false",
            "moq: false",
            "",
            "paths:",
            *path_sections,
            "",
        )
    )

    try:
        config_path.write_text(content, encoding="utf-8")
    except OSError as error:
        return {"path": str(config_path), "changed": False, "message": str(error)}

    return {"path": str(config_path), "changed": True, "message": "ok"}


def mediamtx_camera_path(
    name: str,
    *,
    secondary: bool,
    width: int,
    height: int,
    fps: int,
    bitrate_kbps: int,
    max_readers: int,
) -> str:
    return "\n".join(
        (
            f"  {yaml_quote(name)}:",
            "    source: rpiCamera",
            f"    rpiCameraSecondary: {str(secondary).lower()}",
            "    rpiCameraCodec: hardwareH264",
            f"    rpiCameraWidth: {width}",
            f"    rpiCameraHeight: {height}",
            f"    rpiCameraFPS: {fps}",
            f"    rpiCameraBitrate: {bitrate_kbps * 1000}",
            f"    maxReaders: {max_readers}",
        )
    )


def yaml_quote(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def clear_cache(key: str) -> None:
    _CACHE.pop(key, None)


def service_logs(unit: str, lines: int = 40) -> list[str]:
    if not shutil.which("journalctl"):
        return []

    try:
        result = subprocess.run(
            ["journalctl", "-u", unit, "-n", str(lines), "--no-pager", "--output", "short-iso"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []

    return [line for line in result.stdout.splitlines() if line][-lines:]


def systemctl_value(args: list[str]) -> str:
    try:
        result = subprocess.run(
            ["systemctl", *args],
            check=False,
            capture_output=True,
            text=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        return "unknown"

    return (result.stdout or result.stderr).strip().splitlines()[0] if (result.stdout or result.stderr).strip() else "unknown"


def camera_status() -> dict[str, Any]:
    command = first_available_command(("rpicam-hello", "libcamera-hello"))
    if command is None:
        return {
            "available": False,
            "tool": None,
            "message": "No rpicam/libcamera command found.",
        }

    try:
        result = subprocess.run(
            [command, "--list-cameras"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"available": False, "tool": command, "message": str(error)}

    output = (result.stdout + result.stderr).strip()
    return {
        "available": result.returncode == 0 and "Available cameras" in output,
        "tool": command,
        "message": output or "Camera command returned no output.",
    }


def first_available_command(names: tuple[str, ...]) -> str | None:
    for name in names:
        if shutil.which(name):
            return name
    return None
