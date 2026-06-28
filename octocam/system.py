from __future__ import annotations

import shutil
import socket
import subprocess
import time
from pathlib import Path
from typing import Any, Callable


_CACHE: dict[str, tuple[float, Any]] = {}


def status() -> dict[str, Any]:
    return {
        "hostname": socket.gethostname(),
        "ip_addresses": ip_addresses(),
        "uptime": uptime_text(),
        "cpu_temp_c": cpu_temp_c(),
        "wifi": cached("wifi", 5, wifi_status),
        "camera": cached("camera", 10, camera_status),
        "services": {
            "octocam_web": cached("svc:octocam-web", 5, lambda: service_status("octocam-web")),
            "homebridge": cached("svc:homebridge", 5, lambda: service_status("homebridge")),
            "rtsp": cached("svc:octocam-rtsp", 5, lambda: service_status("octocam-rtsp")),
        },
        "logs": cached("logs:octocam-web", 10, lambda: service_logs("octocam-web")),
    }


def cached(key: str, ttl_seconds: int, loader: Callable[[], Any]) -> Any:
    now = time.monotonic()
    cached_at, value = _CACHE.get(key, (0.0, None))
    if now - cached_at < ttl_seconds:
        return value

    value = loader()
    _CACHE[key] = (now, value)
    return value


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
    if not shutil.which("nmcli"):
        return {"ssid": None, "state": "unavailable", "message": "NetworkManager CLI not found."}

    try:
        result = subprocess.run(
            ["nmcli", "-t", "-f", "ACTIVE,SSID", "dev", "wifi"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"ssid": None, "state": "error", "message": str(error)}

    for line in result.stdout.splitlines():
        active, _, ssid = line.partition(":")
        if active == "yes":
            return {"ssid": ssid or None, "state": "connected", "message": "Connected"}

    return {"ssid": None, "state": "disconnected", "message": "No active Wi-Fi connection."}


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
