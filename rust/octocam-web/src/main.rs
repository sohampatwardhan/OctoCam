mod camera;
mod mediamtx;
mod matter;
mod proc;
mod security;
mod settings;
mod streams;
mod system;
mod wifi;
mod wifi_setup;

use askama::Template;
use axum::{
    extract::{Form, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::PathBuf,
    process::{self, Command},
    sync::Arc,
};
use tokio::time::{sleep, Duration};
use tower_http::trace::TraceLayer;

use settings::{preset_views, Settings, RESOLUTION_PRESETS, SUB_RESOLUTION_PRESETS};

const SESSION_COOKIE: &str = "octocam_session";
const STATIC_CACHE_CONTROL: &str = "public, max-age=604800, stale-while-revalidate=86400";
const SERVICE_WORKER_JS: &str = include_str!("../../../static/sw.js");

type AppResult = Result<Response, AppError>;

/// Latest snapshot bytes plus when they were captured; None until first capture.
type SnapshotCache = Arc<tokio::sync::Mutex<Option<(std::time::Instant, Vec<u8>)>>>;

#[derive(Clone)]
struct AppState {
    project_dir: PathBuf,
    config_path: PathBuf,
    wifi_cache_path: PathBuf,
    mediamtx_config_path: PathBuf,
    homekit_status_path: PathBuf,
    matter_identity_path: PathBuf,
    matter_env_path: PathBuf,
    matter_status_path: PathBuf,
    matter_storage_dir: PathBuf,
    secret_key: String,
    snapshot_cache: SnapshotCache,
    /// Set when the loopback snapshot listener could not bind — surfaced on
    /// /matter, since the Matter daemon has no snapshot fallback.
    internal_listener_down: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Debug)]
struct AppError(String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0).into_response()
    }
}

impl From<askama::Error> for AppError {
    fn from(error: askama::Error) -> Self {
        Self(error.to_string())
    }
}

/// Caps how many subprocess-heavy helpers run at once, independent of request volume.
/// Tokio docs explicitly recommend a semaphore to bound spawn_blocking concurrency,
/// since the blocking pool defaults to 512 threads with an unbounded queue.
fn blocking_gate() -> &'static tokio::sync::Semaphore {
    static GATE: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Semaphore::new(4))
}

/// Run a blocking (subprocess-heavy) closure on Tokio's blocking pool so it never
/// occupies a worker/reactor thread, while bounding total concurrency. Maps a panic
/// in the closure (JoinError) or a closed gate to a 500.
async fn run_blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let _permit = blocking_gate()
        .acquire()
        .await
        .map_err(|_| AppError("blocking gate closed".to_string()))?;
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|error| AppError(format!("background task failed: {error}")))
}

#[derive(Clone, Debug)]
struct StreamUrls {
    main: String,
    sub: String,
    has_sub: bool,
}

#[derive(Clone, Debug)]
struct RotationView {
    value: i32,
    selected: bool,
}

#[derive(Template)]
#[template(path = "identity.html")]
struct IdentityTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    saved: bool,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "wifi.html")]
struct WifiTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    stored_profiles: Vec<system::StoredWifiProfile>,
    wifi_networks: Vec<wifi::WifiNetworkView>,
    has_wifi_networks: bool,
    wifi_mac_address: String,
    wifi_message: String,
    has_wifi_message: bool,
    saved: bool,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "stream_settings.html")]
struct StreamSettingsTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    resolution_presets: Vec<settings::PresetView>,
    sub_resolution_presets: Vec<settings::PresetView>,
    saved: bool,
    rotations: Vec<RotationView>,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "rtsp.html")]
struct RtspTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    rtsp_urls: StreamUrls,
    saved: bool,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "homekit.html")]
struct HomeKitTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    homekit: HomeKitView,
    saved: bool,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "matter.html")]
struct MatterTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    matter: matter::MatterView,
    saved: bool,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "admin.html")]
struct AdminTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    saved: bool,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "system.html")]
struct SystemTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "logs.html")]
struct LogsTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "terminal.html")]
struct TerminalTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    active_page: &'static str,
}

#[derive(Template)]
#[template(path = "stream.html")]
struct StreamTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    rtsp_urls: StreamUrls,
    browser_stream_urls: StreamUrls,
    active_page: &'static str,
    initial_stream: String, // "main" | "sub"
    main_busy: bool,        // reserved for the client-side busy note; starts false
    viewers_main_text: String,
    viewers_sub_text: String,
}

#[derive(Template)]
#[template(path = "setup.html")]
struct SetupTemplate {
    settings: Settings,
    resolution_presets: Vec<settings::PresetView>,
    wifi_networks: Vec<wifi::WifiNetworkView>,
    has_wifi_networks: bool,
    wifi_value: String,
    wifi_message: String,
    has_wifi_message: bool,
    security_message: String,
    has_security_message: bool,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    failed: bool,
    next_query: String,
}

#[derive(Deserialize)]
struct SetupQuery {
    wifi_message: Option<String>,
    security_message: Option<String>,
}

#[derive(Deserialize)]
struct LoginQuery {
    failed: Option<String>,
    next: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct HomeKitStatus {
    status: Option<String>,
    paired: Option<bool>,
    pincode: Option<String>,
    setup_uri: Option<String>,
    qr_data_url: Option<String>,
    stream_source: Option<String>,
    rtsp_url: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct HomeKitView {
    status: String,
    paired: bool,
    has_pairing: bool,
    pincode: String,
    setup_uri: String,
    has_qr: bool,
    qr_data_url: String,
    stream_source: String,
    rtsp_url: String,
    error: String,
    has_error: bool,
}

#[derive(Deserialize)]
struct SavedQuery {
    saved: Option<String>,
    wifi_message: Option<String>,
}

#[derive(Deserialize)]
struct PowerForm {
    action: String,
    #[serde(rename = "_return_to")]
    return_to: Option<String>,
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        // Default is 512; far too many 2 MB-stack threads for a 512 MB Pi Zero 2 W.
        .max_blocking_threads(12)
        .build()
        .expect("build Tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    if run_cli_command() {
        return;
    }

    let state = Arc::new(AppState::from_env());

    // Reconcile the mediamtx config with (possibly migrated) settings at startup,
    // restarting the RTSP service only when the rendered config actually changed.
    // The /run marker (tmpfs, cleared each boot) limits the reconcile restart to once
    // per boot so a crash-looping octocam-web cannot flap the camera service.
    {
        let settings = settings::load_settings(&state.config_path);
        let config_path = state.mediamtx_config_path.clone();
        let _ = run_blocking(move || {
            match mediamtx::write_mediamtx_config(&settings, &config_path) {
                Ok(true) => {
                    let marker = std::path::Path::new("/run/octocam-rtsp-reconciled");
                    if !marker.exists() {
                        let _ = std::fs::write(marker, b"1");
                        let _ = system::restart_service("octocam-rtsp");
                    }
                }
                Ok(false) => {}
                Err(error) => eprintln!("mediamtx config reconcile failed: {error}"),
            }
        })
        .await;
    }

    {
        let settings = settings::load_settings(&state.config_path);
        if !settings.setup_complete {
            spawn_captive_portal_listener();
        }
    }

    spawn_internal_listener(state.clone());

    let app = Router::new()
        .route("/", get(identity))
        .route("/identity", get(identity))
        .route("/wifi", get(wifi_page))
        .route("/stream-settings", get(stream_settings))
        .route("/rtsp", get(rtsp_page))
        .route("/homekit", get(homekit))
        .route("/matter", get(matter_page))
        .route("/matter/reset", post(matter_reset))
        .route("/admin", get(admin))
        .route("/advanced", get(system_page))
        .route("/system", get(system_page))
        .route("/logs", get(logs))
        .route("/terminal", get(terminal))
        .route("/stream", get(stream))
        .route("/setup", get(setup).post(complete_setup))
        .route("/wifi/scan", post(scan_wifi))
        .route("/wifi/connect", post(connect_wifi))
        .route("/wifi/delete", post(delete_wifi_profile))
        .route("/settings", post(update_settings))
        .route("/power", post(power_action))
        .route("/login", get(login).post(authenticate))
        .route("/logout", post(logout))
        .route("/api/settings", get(api_settings))
        .route("/api/status", get(api_status))
        .route("/api/wifi/networks", get(api_wifi_networks))
        .route("/api/wifi/scan", post(api_wifi_scan))
        .route("/snapshot.jpg", get(snapshot))
        .route("/sw.js", get(service_worker))
        .route("/static/{*path}", get(static_asset))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let host = env::var("OCTOCAM_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("OCTOCAM_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("valid OCTOCAM_HOST/OCTOCAM_PORT");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind OctoCam web address");
    axum::serve(listener, app)
        .await
        .expect("serve OctoCam web app");
}

fn run_cli_command() -> bool {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return false;
    }

    match args[0].as_str() {
        "--scan-wifi-cache" => {
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(wifi::default_cache_path);
            match wifi::scan_and_cache_networks(&path) {
                Ok(_) => true,
                Err(error) => {
                    eprintln!("Wi-Fi scan failed: {error}");
                    process::exit(1);
                }
            }
        }
        "--wifi-setup" => match wifi_setup::run() {
            Ok(_) => true,
            Err(error) => {
                eprintln!("Wi-Fi setup failed: {error}");
                process::exit(1);
            }
        },
        "--help" | "-h" => {
            println!("Usage: octocam-web [--scan-wifi-cache [path] | --wifi-setup]");
            true
        }
        unknown => {
            eprintln!("Unknown option: {unknown}");
            process::exit(2);
        }
    }
}

impl AppState {
    fn from_env() -> Self {
        let project_dir = env::var_os("OCTOCAM_PROJECT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let config_path = settings::default_config_path();
        let wifi_cache_path = wifi::default_cache_path();
        let mediamtx_config_path = mediamtx::default_config_path();
        let homekit_status_path = env::var_os("OCTOCAM_HOMEKIT_STATUS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/homekit-status.json"));
        let secret_key = load_secret_key();
        Self {
            project_dir,
            config_path,
            wifi_cache_path,
            mediamtx_config_path,
            homekit_status_path,
            matter_identity_path: matter::default_identity_path(),
            matter_env_path: matter::default_env_path(),
            matter_status_path: matter::default_status_path(),
            matter_storage_dir: matter::default_storage_dir(),
            secret_key,
            snapshot_cache: Arc::new(tokio::sync::Mutex::new(None)),
            internal_listener_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

async fn identity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(IdentityTemplate {
        page_title: settings.camera_label.clone(),
        saved: query.saved.as_deref() == Some("1"),
        system: system::view(&status),
        settings,
        active_page: "identity",
    })
}

async fn wifi_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    let cache = wifi::load_network_cache(&state.wifi_cache_path);
    let wifi_networks = wifi::network_views(&cache, status.wifi.ssid.as_deref().unwrap_or(""));
    let wifi_for_profiles = status.wifi.clone();
    let stored_profiles =
        run_blocking(move || system::stored_wifi_profiles(&wifi_for_profiles)).await?;
    render(WifiTemplate {
        page_title: "Wi-Fi".to_string(),
        saved: query.saved.as_deref() == Some("1"),
        stored_profiles,
        has_wifi_networks: !wifi_networks.is_empty(),
        wifi_networks,
        wifi_mac_address: status
            .wifi
            .mac_address
            .clone()
            .unwrap_or_else(|| "Not available".to_string()),
        wifi_message: query.wifi_message.clone().unwrap_or_default(),
        has_wifi_message: query.wifi_message.is_some(),
        settings,
        system: system::view(&status),
        active_page: "wifi",
    })
}

async fn stream_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(StreamSettingsTemplate {
        page_title: "Stream".to_string(),
        resolution_presets: preset_views(RESOLUTION_PRESETS, &settings.current_resolution()),
        sub_resolution_presets: preset_views(
            SUB_RESOLUTION_PRESETS,
            &settings.current_sub_resolution(),
        ),
        rotations: rotation_views(settings.rotation),
        saved: query.saved.as_deref() == Some("1"),
        system: system::view(&status),
        settings,
        active_page: "stream_settings",
    })
}

async fn rtsp_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(RtspTemplate {
        page_title: "RTSP".to_string(),
        rtsp_urls: stream_urls_for(&settings, request_hostname(&headers), "rtsp"),
        saved: query.saved.as_deref() == Some("1"),
        system: system::view(&status),
        settings,
        active_page: "rtsp",
    })
}

async fn homekit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(HomeKitTemplate {
        page_title: "HomeKit".to_string(),
        saved: query.saved.as_deref() == Some("1"),
        homekit: homekit_view(&state.homekit_status_path, &settings),
        settings,
        system: system::view(&status),
        active_page: "homekit",
    })
}

async fn matter_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    // Identity is only materialized once Matter has been enabled; before that
    // the page shows the enable flow without minting a credential.
    let identity = if settings.matter_enabled {
        matter::load_or_generate_identity(&state.matter_identity_path).ok()
    } else {
        None
    };
    let matter_status = matter::read_status(&state.matter_status_path);
    let mut matter_view = matter::view(&settings, identity.as_ref(), &matter_status);
    matter_view.snapshot_endpoint_down = state
        .internal_listener_down
        .load(std::sync::atomic::Ordering::Relaxed);
    render(MatterTemplate {
        page_title: "Matter".to_string(),
        saved: query.saved.as_deref() == Some("1"),
        matter: matter_view,
        settings,
        system: system::view(&status),
        active_page: "matter",
    })
}

async fn matter_reset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    let (storage, env_path, id_path) = (
        state.matter_storage_dir.clone(),
        state.matter_env_path.clone(),
        state.matter_identity_path.clone(),
    );
    run_blocking(move || matter::reset_pairing(&settings, &storage, &env_path, &id_path)).await?;
    Ok(Redirect::to("/matter?saved=1").into_response())
}

async fn admin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(AdminTemplate {
        page_title: "Admin".to_string(),
        saved: query.saved.as_deref() == Some("1"),
        settings,
        system: system::view(&status),
        active_page: "admin",
    })
}

async fn system_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(SystemTemplate {
        page_title: "System info".to_string(),
        settings,
        system: system::view(&status),
        active_page: "system",
    })
}

async fn logs(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(LogsTemplate {
        page_title: "System logs".to_string(),
        settings,
        system: system::view(&status),
        active_page: "logs",
    })
}

async fn terminal(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    render(TerminalTemplate {
        page_title: "Terminal".to_string(),
        settings,
        system: system::view(&status),
        active_page: "terminal",
    })
}

async fn service_worker() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    (headers, SERVICE_WORKER_JS).into_response()
}

async fn static_asset(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    if path.is_empty() || path.contains("..") || path.contains('\\') || path.starts_with('/') {
        return StatusCode::NOT_FOUND.into_response();
    }

    let asset_path = state.project_dir.join("static").join(&path);
    let bytes = match tokio::fs::read(asset_path).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type_for(&path)),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(STATIC_CACHE_CONTROL),
    );
    (headers, bytes).into_response()
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

async fn stream(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let host = request_hostname(&headers);
    let status = run_blocking(system::status).await?;
    let viewers = streams::viewer_report(&settings).await;
    // Sub-first default (product decision, hardening 2026-07-02): the dashboard opens
    // on sub so a forgotten kiosk tab never pins main's only slot. Main is opt-in via
    // the Main button; app.js reroutes that click to sub (with a note) when main is
    // full. `main_busy` therefore starts false — the note is client-toggled.
    let initial_stream = if settings.sub_stream_enabled { "sub" } else { "main" }.to_string();
    let main_busy = false;
    let (viewers_main_text, viewers_sub_text) = match &viewers {
        Some(report) => (
            format!("{} / {}", report.main.total, report.main.capacity),
            format!("{} / {}", report.sub.total, report.sub.capacity),
        ),
        None => ("unavailable".to_string(), "unavailable".to_string()),
    };
    render(StreamTemplate {
        page_title: "Live stream".to_string(),
        rtsp_urls: stream_urls_for(&settings, host.clone(), "rtsp"),
        browser_stream_urls: stream_urls_for(&settings, host, "webrtc"),
        system: system::view(&status),
        settings,
        active_page: "stream",
        initial_stream,
        main_busy,
        viewers_main_text,
        viewers_sub_text,
    })
}

async fn setup(State(state): State<Arc<AppState>>, Query(query): Query<SetupQuery>) -> AppResult {
    let settings = settings::load_settings(&state.config_path);
    let status = run_blocking(system::status).await?;
    let cache = wifi::load_network_cache(&state.wifi_cache_path);
    let selected = status
        .wifi
        .ssid
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| settings.wifi_ssid.clone());
    let wifi_networks = wifi::network_views(&cache, &selected);
    let wifi_value = if selected.is_empty() {
        settings.wifi_ssid.clone()
    } else {
        selected
    };
    render(SetupTemplate {
        resolution_presets: preset_views(RESOLUTION_PRESETS, &settings.current_resolution()),
        has_wifi_networks: !wifi_networks.is_empty(),
        wifi_networks,
        wifi_value,
        wifi_message: query.wifi_message.clone().unwrap_or_default(),
        has_wifi_message: query.wifi_message.is_some(),
        security_message: query.security_message.clone().unwrap_or_default(),
        has_security_message: query.security_message.is_some(),
        settings,
    })
}

async fn complete_setup(
    State(state): State<Arc<AppState>>,
    Form(mut form): Form<HashMap<String, String>>,
) -> AppResult {
    let mut current = settings::load_settings(&state.config_path);
    let admin_password = form.remove("admin_password").unwrap_or_default();
    let admin_password_confirm = form.remove("admin_password_confirm").unwrap_or_default();
    let wifi_password = form.remove("wifi_password").unwrap_or_default();
    let wifi_ssid = form.get("wifi_ssid").cloned().unwrap_or_default();
    let cache = wifi::load_network_cache(&state.wifi_cache_path);
    let wifi_security = wifi::cached_security_for(&cache, &wifi_ssid);

    if admin_password != admin_password_confirm {
        return Ok(
            Redirect::to("/setup?security_message=Admin%20passwords%20do%20not%20match.")
                .into_response(),
        );
    }
    if !wifi_ssid.trim().is_empty() {
        let (ssid, password, security) =
            (wifi_ssid.clone(), wifi_password.clone(), wifi_security.clone());
        let (connected, message) =
            run_blocking(move || wifi::connect_to_network(&ssid, &password, &security)).await?;
        if !connected {
            return Ok(Redirect::to(&format!(
                "/setup?wifi_message={}",
                urlencoding::encode(&message)
            ))
            .into_response());
        }
    }

    form.insert("setup_complete".to_string(), "true".to_string());
    form.insert("camera_enabled".to_string(), "true".to_string());
    form.insert(
        "homekit_enabled".to_string(),
        form.contains_key("homekit_enabled").to_string(),
    );
    form.insert(
        "admin_password_hash".to_string(),
        security::hash_password(&admin_password),
    );
    let validated = settings::validate_form(&form);
    merge_settings(&mut current, validated);
    settings::save_settings(&state.config_path, &current)
        .map_err(|error| AppError(error.to_string()))?;
    let homekit_settings = current.clone();
    run_blocking(move || configure_homekit_service(&homekit_settings)).await?;
    Ok(with_login_cookie(
        Redirect::to("/?saved=1").into_response(),
        &state,
    ))
}

async fn scan_wifi(State(state): State<Arc<AppState>>) -> Response {
    let cache_path = state.wifi_cache_path.clone();
    // scan_wifi returns Response (not AppResult), so handle the result explicitly
    // rather than with `?` (FIX-2).
    let message = match run_blocking(move || wifi::scan_and_cache_networks(&cache_path)).await {
        Ok(Ok(_)) => "Wi-Fi scan complete.".to_string(),
        Ok(Err(error)) => format!("Wi-Fi scan failed: {error}"),
        Err(_join) => "Wi-Fi scan failed.".to_string(),
    };
    Redirect::to(&format!(
        "/setup?wifi_message={}",
        urlencoding::encode(&message)
    ))
    .into_response()
}

async fn connect_wifi(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Form(mut form): Form<HashMap<String, String>>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }

    let wifi_ssid = form
        .remove("wifi_ssid")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| form.remove("wifi_ssid_manual"))
        .filter(|value| !value.trim().is_empty())
        .or_else(|| form.remove("wifi_ssid_scanned"))
        .unwrap_or_default();
    let wifi_password = form.remove("wifi_password").unwrap_or_default();
    if wifi_ssid.trim().is_empty() {
        return Ok(
            Redirect::to("/wifi?wifi_message=Enter%20a%20Wi-Fi%20network%20name.").into_response(),
        );
    }

    let cache = wifi::load_network_cache(&state.wifi_cache_path);
    let wifi_security = form
        .remove("wifi_security")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| wifi::cached_security_for(&cache, &wifi_ssid));
    let (ssid, password, security) = (wifi_ssid.clone(), wifi_password.clone(), wifi_security.clone());
    let (connected, message) =
        run_blocking(move || wifi::connect_to_network(&ssid, &password, &security)).await?;
    if connected {
        Ok(Redirect::to("/wifi?wifi_message=Network%20saved.").into_response())
    } else {
        Ok(Redirect::to(&format!(
            "/wifi?wifi_message={}",
            urlencoding::encode(&message)
        ))
        .into_response())
    }
}

async fn delete_wifi_profile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Form(mut form): Form<HashMap<String, String>>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }

    let profile_name = form.remove("wifi_profile_name").unwrap_or_default();
    let profile_source = form.remove("wifi_profile_source").unwrap_or_default();
    let active_ssid = run_blocking(system::status).await?.wifi.ssid;
    if active_ssid.as_deref() == Some(profile_name.trim()) {
        return Ok(Redirect::to(
            "/wifi?wifi_message=Cannot%20delete%20the%20currently%20connected%20network.",
        )
        .into_response());
    }

    let (name, source) = (profile_name.clone(), profile_source.clone());
    let (deleted, message) =
        run_blocking(move || wifi::forget_saved_profile(&name, &source)).await?;
    if deleted {
        Ok(Redirect::to("/wifi?wifi_message=Wi-Fi%20profile%20deleted.").into_response())
    } else {
        Ok(Redirect::to(&format!(
            "/wifi?wifi_message={}",
            urlencoding::encode(&message)
        ))
        .into_response())
    }
}

async fn update_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Form(mut form): Form<HashMap<String, String>>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }

    let mut current = settings::load_settings(&state.config_path);
    let admin_password = form.remove("admin_password").unwrap_or_default();
    let admin_password_confirm = form.remove("admin_password_confirm").unwrap_or_default();
    let return_to = clean_return_path(
        &form
            .remove("_return_to")
            .unwrap_or_else(|| "/identity".to_string()),
    );
    let checkbox_fields = form.remove("_checkboxes").unwrap_or_default();
    for checkbox in checkbox_fields
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form.insert(
            checkbox.to_string(),
            form.contains_key(checkbox).to_string(),
        );
    }
    let mut next_map = settings_to_map(&current)?;
    for (key, value) in form {
        if key.starts_with('_') {
            continue;
        }
        next_map.insert(key, Value::String(value));
    }
    let mut validated = settings::validate_map(&next_map);
    if admin_password.is_empty() && admin_password_confirm.is_empty() {
        validated.admin_password_hash = current.admin_password_hash.clone();
    } else {
        if admin_password != admin_password_confirm {
            return Ok(Redirect::to(&format!("{return_to}?saved=0")).into_response());
        }
        validated.admin_password_hash = security::hash_password(&admin_password);
    }
    validated.setup_complete = current.setup_complete;
    settings::enforce_matter_requires_admin(&mut validated);
    merge_settings(&mut current, validated);
    settings::save_settings(&state.config_path, &current)
        .map_err(|error| AppError(error.to_string()))?;
    let _ = mediamtx::configure_rtsp_service(&current, &state.mediamtx_config_path);
    let homekit_settings = current.clone();
    run_blocking(move || configure_homekit_service(&homekit_settings)).await?;
    let matter_settings = current.clone();
    let (matter_env, matter_id) = (state.matter_env_path.clone(), state.matter_identity_path.clone());
    run_blocking(move || matter::configure_matter_service(&matter_settings, &matter_env, &matter_id)).await?;
    Ok(Redirect::to(&format!("{return_to}?saved=1")).into_response())
}

async fn power_action(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Form(form): Form<PowerForm>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }

    let return_to = clean_return_path(&form.return_to.unwrap_or_else(|| "/identity".to_string()));
    schedule_power_action(&form.action)?;
    Ok(Redirect::to(&return_to).into_response())
}

fn schedule_power_action(action: &str) -> Result<(), AppError> {
    let args: &[&str] = match action {
        "restart_service" => &["restart", "octocam-web.service"],
        "restart_device" => &["reboot"],
        "shutdown_device" => &["poweroff"],
        _ => {
            return Err(AppError("Unknown power action.".to_string()));
        }
    };
    schedule_systemctl(args)
}

fn schedule_systemctl(args: &[&str]) -> Result<(), AppError> {
    if !system::command_exists("systemctl") {
        return Err(AppError("systemctl not found.".to_string()));
    }

    let (command, command_args) = if system::command_exists("sudo") {
        let mut command_args = vec!["-n".to_string(), "systemctl".to_string()];
        command_args.extend(args.iter().map(|arg| (*arg).to_string()));
        ("sudo".to_string(), command_args)
    } else {
        (
            "systemctl".to_string(),
            args.iter().map(|arg| (*arg).to_string()).collect(),
        )
    };

    tokio::spawn(async move {
        sleep(Duration::from_millis(900)).await;
        let _ = tokio::task::spawn_blocking(move || {
            let _ = proc::run(Command::new(command).args(command_args), proc::SERVICE_TIMEOUT);
        })
        .await;
    });

    Ok(())
}

async fn login(Query(query): Query<LoginQuery>) -> AppResult {
    render(LoginTemplate {
        failed: query.failed.as_deref() == Some("1"),
        next_query: query.next.unwrap_or_default(),
    })
}

async fn authenticate(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LoginQuery>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let settings = settings::load_settings(&state.config_path);
    let password = form.get("admin_password").cloned().unwrap_or_default();
    if !settings.admin_password_hash.is_empty()
        && security::verify_password(&password, &settings.admin_password_hash)
    {
        let next = query
            .next
            .filter(|value| value.starts_with('/'))
            .unwrap_or_else(|| "/".to_string());
        return with_login_cookie(Redirect::to(&next).into_response(), &state);
    }
    Redirect::to("/login?failed=1").into_response()
}

async fn logout() -> Response {
    let mut response = Redirect::to("/login").into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("octocam_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
    );
    response
}

async fn api_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    Ok(Json(settings::public_settings(&settings::load_settings(
        &state.config_path,
    )))
    .into_response())
}

async fn api_status(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    let (status, viewers) = tokio::join!(
        run_blocking(system::status),
        streams::viewer_report(&settings)
    );
    #[derive(Serialize)]
    struct StatusResponse {
        #[serde(flatten)]
        status: system::SystemStatus,
        viewers: Option<streams::ViewerReport>,
    }
    Ok(Json(StatusResponse {
        status: status?,
        viewers,
    })
    .into_response())
}

async fn api_wifi_networks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    Ok(Json(wifi::load_network_cache(&state.wifi_cache_path)).into_response())
}

async fn api_wifi_scan(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    let cache_path = state.wifi_cache_path.clone();
    match run_blocking(move || wifi::scan_and_cache_networks(&cache_path)).await? {
        Ok(cache) => Ok(Json(cache).into_response()),
        Err(error) => Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response()),
    }
}

async fn snapshot(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    serve_snapshot(&state).await
}

/// Shared snapshot core: the authenticated /snapshot.jpg route and the
/// loopback-only internal listener both funnel here, so the camera_enabled
/// gate and the 2s single-flight cache apply identically to both.
async fn serve_snapshot(state: &Arc<AppState>) -> AppResult {
    let settings = settings::load_settings(&state.config_path);
    if !settings.camera_enabled {
        return Ok((
            StatusCode::CONFLICT,
            "Camera is disabled in OctoCam settings.\n",
        )
            .into_response());
    }
    let mut cache = state.snapshot_cache.lock().await;
    if let Some((at, bytes)) = cache.as_ref() {
        if camera::snapshot_is_fresh(*at, std::time::Instant::now()) {
            let bytes = bytes.clone();
            return Ok(([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response());
        }
    }
    // Cold path: hold the lock across capture so concurrent requests coalesce onto
    // one capture (bounded by CAPTURE_TIMEOUT = 8s). Accepted trade-off: a burst of
    // concurrent snapshot requests serializes behind the first — worst case one
    // 8s wait, then everyone is served from cache.
    let settings_for_capture = settings.clone();
    match run_blocking(move || camera::capture_snapshot(&settings_for_capture)).await? {
        Ok(data) => {
            *cache = Some((std::time::Instant::now(), data.clone()));
            Ok(([(header::CONTENT_TYPE, "image/jpeg")], data).into_response())
        }
        Err(error) => Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Snapshot unavailable: {error}\n"),
        )
            .into_response()),
    }
}

fn render<T: Template>(template: T) -> AppResult {
    Ok(Html(template.render()?).into_response())
}

fn homekit_view(path: &PathBuf, settings: &Settings) -> HomeKitView {
    let status = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<HomeKitStatus>(&raw).ok())
        .unwrap_or_default();
    let status_label = status.status.unwrap_or_else(|| {
        if settings.homekit_enabled {
            "starting".to_string()
        } else {
            "disabled".to_string()
        }
    });
    let pincode = status.pincode.unwrap_or_default();
    let setup_uri = status.setup_uri.unwrap_or_default();
    let qr_data_url = status.qr_data_url.unwrap_or_default();
    let error = status.error.unwrap_or_default();
    HomeKitView {
        status: status_label,
        paired: status.paired.unwrap_or(settings.homekit_paired),
        has_pairing: !pincode.is_empty() || !setup_uri.is_empty(),
        pincode,
        setup_uri,
        has_qr: !qr_data_url.is_empty(),
        qr_data_url,
        stream_source: status.stream_source.unwrap_or_else(|| {
            if settings.sub_stream_enabled {
                "sub"
            } else {
                "main"
            }
            .to_string()
        }),
        rtsp_url: status.rtsp_url.unwrap_or_default(),
        has_error: !error.is_empty(),
        error,
    }
}

fn configure_homekit_service(settings: &Settings) {
    const UNIT: &str = "octocam-homekit";
    if settings.homekit_enabled {
        let _ = system::set_service_enabled(UNIT, true);
        let _ = system::restart_service(UNIT);
    } else {
        let _ = system::set_service_enabled(UNIT, false);
    }
}

fn settings_to_map<T: Serialize>(settings: &T) -> Result<Map<String, Value>, AppError> {
    match serde_json::to_value(settings).map_err(|error| AppError(error.to_string()))? {
        Value::Object(map) => Ok(map),
        _ => Err(AppError(
            "settings did not serialize to an object".to_string(),
        )),
    }
}

fn clean_return_path(path: &str) -> String {
    if path.starts_with('/') && !path.starts_with("//") && !path.contains('?') {
        path.to_string()
    } else {
        "/identity".to_string()
    }
}

fn require_admin_login(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    api: bool,
) -> Result<Option<Response>, AppError> {
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete || settings.admin_password_hash.is_empty() {
        return Ok(None);
    }
    if authenticated(state, headers) {
        return Ok(None);
    }
    if api {
        return Ok(Some(
            (StatusCode::UNAUTHORIZED, "Authentication required.\n").into_response(),
        ));
    }
    let next = urlencoding::encode(uri.path());
    Ok(Some(
        Redirect::to(&format!("/login?next={next}")).into_response(),
    ))
}

fn authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(cookie_header) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    cookie_header
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .any(|(name, value)| {
            name == SESSION_COOKIE && security::verify_session(&state.secret_key, value)
        })
}

fn with_login_cookie(mut response: Response, state: &AppState) -> Response {
    let cookie = format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax",
        security::sign_session(&state.secret_key)
    );
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

fn load_secret_key() -> String {
    let path = env::var("OCTOCAM_SECRET_KEY_FILE")
        .unwrap_or_else(|_| "/var/lib/octocam/secret-key".to_string());
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "octocam-local-dev".to_string())
}

fn stream_urls_for(settings: &Settings, host: String, protocol: &str) -> StreamUrls {
    StreamUrls {
        main: stream_url_for(settings, "main", protocol, &host),
        sub: if settings.sub_stream_enabled {
            stream_url_for(settings, "sub", protocol, &host)
        } else {
            String::new()
        },
        has_sub: settings.sub_stream_enabled,
    }
}

fn stream_url_for(settings: &Settings, stream: &str, protocol: &str, host: &str) -> String {
    let path = if stream == "main" {
        &settings.rtsp_path
    } else {
        &settings.sub_rtsp_path
    }
    .trim_matches('/');
    match protocol {
        "rtsp" => format!("rtsp://{host}:8554/{path}"),
        "hls" => format!("http://{host}:8888/{path}/index.m3u8"),
        "webrtc" => format!("http://{host}:8889/{path}"),
        "browser" => format!("http://{host}:8888/{path}/"),
        _ => String::new(),
    }
}

fn request_hostname(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("octocam.local");
    if host.starts_with('[') && host.contains(']') {
        host.trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or(host)
            .to_string()
    } else {
        host.rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(host)
            .to_string()
    }
}

fn rotation_views(current: i32) -> Vec<RotationView> {
    [0, 90, 180, 270]
        .into_iter()
        .map(|value| RotationView {
            value,
            selected: value == current,
        })
        .collect()
}

fn merge_settings(current: &mut Settings, next: Settings) {
    *current = next;
}

/// NetworkManager shared-mode gateway address of the OctoCam-Setup AP.
const SETUP_AP_GATEWAY: &str = "10.42.0.1";

/// Captive probes carry Host headers like captive.apple.com, which the joined
/// client CANNOT resolve on our uplink-less AP — echoing the Host would produce a
/// dead redirect. Always send clients to the AP gateway IP literal.
fn captive_redirect_target() -> String {
    format!("http://{SETUP_AP_GATEWAY}:8080/setup")
}

async fn captive_probe() -> Response {
    // The listener keeps running until the process restarts, even after setup
    // completes. Re-check per request so a completed setup stops hijacking
    // port 80 — plain 404 instead of redirecting everything to /setup.
    let settings = settings::load_settings(&settings::default_config_path());
    if settings.setup_complete {
        return StatusCode::NOT_FOUND.into_response();
    }
    Redirect::temporary(&captive_redirect_target()).into_response()
}

/// Loopback-only endpoint for local daemons (the Matter camera-app fetches
/// snapshots here). Binding a separate 127.0.0.1 listener is the guard —
/// structurally unreachable from the LAN, no header/peer-address parsing —
/// while serve_snapshot keeps the camera_enabled check (hardening FIX-3).
async fn internal_snapshot(State(state): State<Arc<AppState>>) -> AppResult {
    serve_snapshot(&state).await
}

fn spawn_internal_listener(state: Arc<AppState>) {
    tokio::spawn(async move {
        let port = env::var("OCTOCAM_INTERNAL_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(8081);
        // First-boot bind races are a known failure class on this hardware
        // (cf. 460ee33): retry briefly before declaring the endpoint down.
        for attempt in 1..=3u32 {
            match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                Ok(listener) => {
                    state
                        .internal_listener_down
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    let app = Router::new()
                        .route("/internal/snapshot.jpg", get(internal_snapshot))
                        .with_state(state.clone());
                    let _ = axum::serve(listener, app).await;
                    return;
                }
                Err(error) => {
                    tracing::error!(
                        "internal snapshot listener bind failed (127.0.0.1:{port}, attempt {attempt}/3): {error}"
                    );
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
        state
            .internal_listener_down
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::error!(
            "internal snapshot listener unavailable (127.0.0.1:{port}); Matter snapshots will fail until octocam-web restarts"
        );
    });
}

fn spawn_captive_portal_listener() {
    tokio::spawn(async {
        let app = Router::new()
            .route("/hotspot-detect.html", get(captive_probe))
            .route("/generate_204", get(captive_probe))
            // axum 0.8: fallback takes a Handler, not a MethodRouter — no get() wrapper.
            .fallback(captive_probe);
        match tokio::net::TcpListener::bind("0.0.0.0:80").await {
            Ok(listener) => {
                let _ = axum::serve(listener, app).await;
            }
            Err(error) => {
                eprintln!("captive portal listener unavailable (port 80): {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captive_redirect_targets_the_ap_gateway() {
        // Never echo the probe's Host header (captive.apple.com etc.) — the client
        // cannot resolve it on the uplink-less AP. Always the gateway IP literal.
        assert_eq!(captive_redirect_target(), "http://10.42.0.1:8080/setup");
    }
}
