from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from flask import Flask, Response, jsonify, redirect, render_template, request, session, url_for

from octocam.camera import capture_jpeg
from octocam.security import hash_password, valid_password, verify_password
from octocam.settings import (
    DEFAULT_SETTINGS,
    RESOLUTION_PRESETS,
    SUB_RESOLUTION_PRESETS,
    load_settings,
    public_settings,
    save_settings,
    validate_settings,
)
from octocam.system import configure_rtsp_service, status as system_status
from octocam.wifi import connect_to_network, load_network_cache, scan_and_cache_networks


PROJECT_ROOT = Path(__file__).resolve().parent.parent


def load_secret_key() -> str:
    path = os.environ.get("OCTOCAM_SECRET_KEY_FILE", "/var/lib/octocam/secret-key")
    try:
        with open(path, "r", encoding="utf-8") as handle:
            value = handle.read().strip()
            if value:
                return value
    except OSError:
        pass
    return "octocam-local-dev"


app = Flask(
    __name__,
    template_folder=str(PROJECT_ROOT / "templates"),
    static_folder=str(PROJECT_ROOT / "static"),
)
app.config["SECRET_KEY"] = os.environ.get("OCTOCAM_SECRET_KEY", load_secret_key())


@app.before_request
def require_admin_login() -> Response | None:
    settings = load_settings()
    allowed_paths = {"/login"}

    if request.path.startswith("/static/"):
        return None

    if not settings["setup_complete"]:
        return None

    if not settings["admin_password_hash"]:
        return None

    if request.path in allowed_paths:
        return None

    if session.get("authenticated") is True:
        return None

    if request.path.startswith("/api/") or request.path == "/snapshot.jpg":
        return Response("Authentication required.\n", status=401)

    return redirect(url_for("login", next=request.path))


@app.get("/")
def index() -> str:
    settings = load_settings()
    if not settings["setup_complete"]:
        return redirect(url_for("setup"))

    return render_template(
        "index.html",
        settings=settings,
        defaults=DEFAULT_SETTINGS,
        resolution_presets=RESOLUTION_PRESETS,
        sub_resolution_presets=SUB_RESOLUTION_PRESETS,
        rtsp_urls=stream_urls_for(settings, "rtsp"),
        hls_urls=stream_urls_for(settings, "hls"),
        system=system_status(),
        saved=request.args.get("saved") == "1",
    )


@app.get("/advanced")
def advanced() -> str:
    settings = load_settings()
    if not settings["setup_complete"]:
        return redirect(url_for("setup"))

    return render_template(
        "advanced.html",
        settings=settings,
        rtsp_urls=stream_urls_for(settings, "rtsp"),
        hls_urls=stream_urls_for(settings, "hls"),
        system=system_status(),
    )


@app.get("/stream")
def stream() -> str:
    settings = load_settings()
    if not settings["setup_complete"]:
        return redirect(url_for("setup"))

    return render_template(
        "stream.html",
        settings=settings,
        rtsp_urls=stream_urls_for(settings, "rtsp"),
        hls_urls=stream_urls_for(settings, "hls"),
        webrtc_urls=stream_urls_for(settings, "webrtc"),
        browser_stream_urls=stream_urls_for(settings, "browser"),
        system=system_status(),
        refresh_ms=1500,
    )


@app.get("/setup")
def setup() -> str:
    return render_template(
        "setup.html",
        settings=load_settings(),
        resolution_presets=RESOLUTION_PRESETS,
        system=system_status(),
        wifi_cache=load_network_cache(),
        wifi_message=request.args.get("wifi_message"),
        security_message=request.args.get("security_message"),
        saved=False,
    )


@app.post("/setup")
def complete_setup() -> Response:
    current = load_settings()
    submitted: dict[str, Any] = request.form.to_dict()
    admin_password = submitted.pop("admin_password", "")
    admin_password_confirm = submitted.pop("admin_password_confirm", "")
    wifi_ssid = submitted.get("wifi_ssid", "").strip()
    wifi_password = submitted.pop("wifi_password", "")
    wifi_security = cached_security_for(wifi_ssid)

    if not valid_password(admin_password):
        return redirect(url_for("setup", security_message="Admin password must be at least 12 characters."))

    if admin_password != admin_password_confirm:
        return redirect(url_for("setup", security_message="Admin passwords do not match."))

    if wifi_ssid:
        connected, message = connect_to_network(wifi_ssid, wifi_password, wifi_security)
        if not connected:
            return redirect(url_for("setup", wifi_message=message))

    submitted["setup_complete"] = True
    submitted["camera_enabled"] = "camera_enabled" in request.form
    submitted["homekit_enabled"] = "homekit_enabled" in request.form
    submitted["admin_password_hash"] = hash_password(admin_password)

    current.update(validate_settings(submitted))
    save_settings(current)
    session["authenticated"] = True
    return redirect(url_for("index", saved="1"))


@app.post("/wifi/scan")
def scan_wifi() -> Response:
    try:
        scan_and_cache_networks()
        message = "Wi-Fi scan complete."
    except Exception as error:
        message = f"Wi-Fi scan failed: {error}"
    return redirect(url_for("setup", wifi_message=message))


@app.post("/settings")
def update_settings() -> Response:
    current = load_settings()
    submitted: dict[str, Any] = request.form.to_dict()
    admin_password = submitted.pop("admin_password", "")
    admin_password_confirm = submitted.pop("admin_password_confirm", "")

    for checkbox in (
        "camera_enabled",
        "rtsp_enabled",
        "sub_stream_enabled",
        "hflip",
        "vflip",
        "homekit_enabled",
        "homekit_paired",
        "motion_enabled",
    ):
        submitted[checkbox] = checkbox in request.form

    current.update(validate_settings(submitted))

    if admin_password or admin_password_confirm:
        if not valid_password(admin_password):
            return redirect(url_for("index", saved="0"))
        if admin_password != admin_password_confirm:
            return redirect(url_for("index", saved="0"))
        current["admin_password_hash"] = hash_password(admin_password)

    save_settings(current)
    configure_rtsp_service(current)
    return redirect(url_for("index", saved="1"))


@app.get("/login")
def login() -> str:
    return render_template("login.html", failed=request.args.get("failed") == "1")


@app.post("/login")
def authenticate() -> Response:
    settings = load_settings()
    password = request.form.get("admin_password", "")

    if settings["admin_password_hash"] and verify_password(password, settings["admin_password_hash"]):
        session["authenticated"] = True
        next_path = request.args.get("next") or url_for("index")
        return redirect(next_path if next_path.startswith("/") else url_for("index"))

    return redirect(url_for("login", failed="1"))


@app.post("/logout")
def logout() -> Response:
    session.clear()
    return redirect(url_for("login"))


@app.get("/api/settings")
def api_settings() -> Response:
    return jsonify(public_settings(load_settings()))


@app.get("/api/status")
def api_status() -> Response:
    return jsonify(system_status())


@app.get("/api/wifi/networks")
def api_wifi_networks() -> Response:
    return jsonify(load_network_cache())


@app.post("/api/wifi/scan")
def api_wifi_scan() -> Response:
    try:
        return jsonify(scan_and_cache_networks())
    except Exception as error:
        return jsonify({"error": str(error)}), 503


@app.get("/snapshot.jpg")
def snapshot() -> Response:
    settings = load_settings()
    if not settings["camera_enabled"]:
        return Response("Camera is disabled in OctoCam settings.\n", status=409)

    data, error = capture_jpeg(settings)
    if data is None:
        return Response(f"Snapshot unavailable: {error}\n", status=503)
    return Response(data, mimetype="image/jpeg")


def main() -> None:
    host = os.environ.get("OCTOCAM_HOST", "0.0.0.0")
    port = int(os.environ.get("OCTOCAM_PORT", "8080"))
    app.run(host=host, port=port, debug=os.environ.get("OCTOCAM_DEBUG") == "1")


def cached_security_for(ssid: str) -> str:
    for network in load_network_cache()["networks"]:
        if network["ssid"] == ssid:
            return network["security"]
    return "wpa2"


def stream_urls_for(settings: dict[str, Any], protocol: str) -> dict[str, str | None]:
    return {
        "main": stream_url_for(settings, "main", protocol),
        "sub": stream_url_for(settings, "sub", protocol) if settings["sub_stream_enabled"] else None,
    }


def stream_url_for(settings: dict[str, Any], stream: str, protocol: str) -> str:
    path_key = "rtsp_path" if stream == "main" else "sub_rtsp_path"
    path = str(settings[path_key]).strip("/")
    host = request_hostname()

    if protocol == "rtsp":
        return f"rtsp://{host}:8554/{path}"
    if protocol == "hls":
        return f"http://{host}:8888/{path}/index.m3u8"
    if protocol == "webrtc":
        return f"http://{host}:8889/{path}"
    if protocol == "browser":
        return f"http://{host}:8888/{path}/"

    raise ValueError(f"Unsupported stream protocol: {protocol}")


def request_hostname() -> str:
    if request.host.startswith("[") and "]" in request.host:
        return request.host[1:].split("]", 1)[0]
    return request.host.rsplit(":", 1)[0]


if __name__ == "__main__":
    main()
