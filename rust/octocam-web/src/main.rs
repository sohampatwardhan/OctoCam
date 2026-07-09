mod backup;
mod camera;
mod matter;
mod mediamtx;
mod proc;
mod security;
mod settings;
mod ssh_keys;
mod streams;
mod system;
mod wifi;
mod wifi_setup;
mod motion;

use askama::Template;
use axum::{
    extract::{DefaultBodyLimit, Form, Multipart, Path as AxumPath, Query, State},
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
    motion_detected: Arc<std::sync::atomic::AtomicBool>,
    motion_tx: tokio::sync::broadcast::Sender<bool>,
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

#[derive(Clone, Debug)]
struct TimeZoneView {
    value: String,
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
    return_path: &'static str,
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
    time_zones: Vec<TimeZoneView>,
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
    restart_days: Vec<WeekdayOption>,
    reboot_days: Vec<WeekdayOption>,
    saved: bool,
    active_page: &'static str,
    restore_message: String,
    has_restore_message: bool,
    restore_is_error: bool,
}

struct WeekdayOption {
    slug: &'static str,
    label: &'static str,
    short_label: &'static str,
    checked: bool,
}

#[derive(Template)]
#[template(path = "ssh_keys.html")]
struct SshKeysTemplate {
    page_title: String,
    settings: Settings,
    system: system::SystemView,
    active_page: &'static str,
    keys: Vec<ssh_keys::AuthorizedKey>,
    has_keys: bool,
    read_error: bool,
    message: String,
    has_message: bool,
    message_is_error: bool,
    warn_fingerprint: String,
    has_warn: bool,
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
    error: String,
    has_error: bool,
}

#[derive(Deserialize)]
struct SavedQuery {
    saved: Option<String>,
    wifi_message: Option<String>,
}

#[derive(Deserialize)]
struct SshKeysQuery {
    status: Option<String>,
    warn: Option<String>,
}

#[derive(Deserialize)]
struct SystemQuery {
    saved: Option<String>,
    restore: Option<String>,
    keys: Option<String>,
}

#[derive(Deserialize)]
struct SshKeyAddForm {
    public_key: String,
}

#[derive(Deserialize)]
struct SshKeyRevokeForm {
    fingerprint: String,
    confirm: Option<String>,
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
            let config_changed = match mediamtx::write_mediamtx_config(&settings, &config_path) {
                Ok(changed) => changed,
                Err(error) => {
                    eprintln!("mediamtx config reconcile failed: {error}");
                    false
                }
            };
            let timezone_changed = match mediamtx::write_timezone_dropin(
                &settings,
                &mediamtx::default_timezone_dropin_path(),
            ) {
                Ok(changed) => {
                    if changed {
                        let _ = system::daemon_reload();
                    }
                    changed
                }
                Err(error) => {
                    eprintln!("rtsp timezone reconcile failed: {error}");
                    false
                }
            };
            if config_changed || timezone_changed {
                let marker = std::path::Path::new("/run/octocam-rtsp-reconciled");
                if !marker.exists() {
                    let _ = std::fs::write(marker, b"1");
                    let _ = system::restart_service("octocam-rtsp");
                }
            }
        })
        .await;
    }

    {
        let settings = settings::load_settings(&state.config_path);
        let _ = run_blocking(move || {
            if let Err(error) = system::configure_time_server(&settings.time_server) {
                eprintln!("time server reconcile failed: {error}");
            }
            if let Err(error) = system::configure_maintenance_timers(&settings) {
                eprintln!("scheduled maintenance reconcile failed: {error}");
            }
        })
        .await;
    }

    {
        let settings = settings::load_settings(&state.config_path);
        if !settings.setup_complete && captive_portal_listener_enabled() {
            spawn_captive_portal_listener();
        }
    }

    spawn_internal_listener(state.clone());

    motion::spawn_motion_detector(
        state.config_path.clone(),
        state.motion_detected.clone(),
        state.motion_tx.clone(),
    );

    let app = Router::new()
        .route("/", get(dashboard_redirect))
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
        .route("/backup", get(backup_download))
        .route(
            "/restore",
            post(restore_upload).layer(DefaultBodyLimit::max(MAX_RESTORE_BYTES)),
        )
        .route("/logs", get(logs))
        .route("/terminal", get(terminal))
        .route("/ssh-keys", get(ssh_keys_page))
        .route("/ssh-keys/add", post(ssh_keys_add))
        .route("/ssh-keys/revoke", post(ssh_keys_revoke))
        .route("/dashboard", get(stream))
        .route("/stream", get(dashboard_redirect))
        .route("/setup", get(setup).post(complete_setup))
        .route("/wifi/scan", post(scan_wifi))
        .route("/wifi/connect", post(connect_wifi))
        .route("/wifi/delete", post(delete_wifi_profile))
        .route("/settings", get(settings_page).post(update_settings))
        .route("/time/sync", post(sync_time))
        .route("/power", post(power_action))
        .route("/login", get(login).post(authenticate))
        .route("/logout", post(logout))
        .route("/hotspot-detect.html", get(captive_probe))
        .route("/generate_204", get(captive_probe))
        .route("/api/settings", get(api_settings))
        .route("/api/status", get(api_status))
        .route("/api/motion/events", get(api_motion_events))
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
        let (motion_tx, _) = tokio::sync::broadcast::channel(32);
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
            motion_detected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            motion_tx,
        }
    }
}

async fn identity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    render_identity_page(state, headers, uri, query, "/identity").await
}

async fn settings_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SavedQuery>,
) -> AppResult {
    render_identity_page(state, headers, uri, query, "/settings").await
}

async fn render_identity_page(
    state: Arc<AppState>,
    headers: HeaderMap,
    uri: Uri,
    query: SavedQuery,
    return_path: &'static str,
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
        return_path,
    })
}

async fn dashboard_redirect() -> Redirect {
    Redirect::to("/dashboard")
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
    let (status, time_zone_values) =
        run_blocking(|| (system::status(), system::available_time_zones())).await?;
    let time_zones = time_zone_views(time_zone_values, &settings.text_overlay_timezone);
    render(StreamSettingsTemplate {
        page_title: "Stream".to_string(),
        resolution_presets: preset_views(RESOLUTION_PRESETS, &settings.current_resolution()),
        sub_resolution_presets: preset_views(
            SUB_RESOLUTION_PRESETS,
            &settings.current_sub_resolution(),
        ),
        time_zones,
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
    Query(query): Query<SystemQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    let (restore_message, restore_is_error) = match query.restore.as_deref() {
        Some("ok") => {
            let added = query.keys.as_deref().unwrap_or("0");
            (
                format!("Configuration restored. {added} SSH key(s) added."),
                false,
            )
        }
        Some("ok_keys_failed") => (
            "Configuration restored, but SSH keys could not be written.".to_string(),
            true,
        ),
        Some("invalid") => ("That file is not a valid OctoCam backup.".to_string(), true),
        Some("too_large") => ("That backup file is too large.".to_string(), true),
        Some("empty") => ("No backup file was uploaded.".to_string(), true),
        Some("csrf") => (
            "Restore blocked: request came from another origin.".to_string(),
            true,
        ),
        _ => (String::new(), false),
    };
    let restart_days = weekday_options(&settings.scheduled_service_restart_days);
    let reboot_days = weekday_options(&settings.scheduled_reboot_days);
    render(SystemTemplate {
        page_title: "System info".to_string(),
        settings,
        system: system::view(&status),
        restart_days,
        reboot_days,
        saved: query.saved.as_deref() == Some("1"),
        active_page: "system",
        has_restore_message: !restore_message.is_empty(),
        restore_message,
        restore_is_error,
    })
}

fn weekday_options(selected_days: &str) -> Vec<WeekdayOption> {
    settings::WEEKDAYS
        .iter()
        .map(|(slug, label, short_label)| WeekdayOption {
            slug: *slug,
            label: *label,
            short_label: *short_label,
            checked: selected_days
                .split(',')
                .map(str::trim)
                .any(|selected| selected.eq_ignore_ascii_case(*label)),
        })
        .collect()
}

async fn backup_download(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    let settings = settings::load_settings(&state.config_path);
    // Pre-setup lockout: never expose config before the device has an admin
    // password (require_admin_login is a no-op while the hash is empty).
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }

    // SSH keys are best-effort: a read failure must not block the settings backup.
    let ssh_keys = run_blocking(ssh_keys::export_lines)
        .await?
        .unwrap_or_default();

    let exported_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let backup = backup::build_backup(&settings, exported_at, ssh_keys);
    let body =
        serde_json::to_string_pretty(&backup).map_err(|error| AppError(error.to_string()))?;
    let filename = backup::backup_filename(&settings.device_name, exported_at);

    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// Cap the restore upload well under the global body limit — a settings + keys
/// envelope is a few KB; 256 KB is generous and bounds memory.
const MAX_RESTORE_BYTES: usize = 256 * 1024;

async fn restore_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    mut multipart: Multipart,
) -> AppResult {
    let current = settings::load_settings(&state.config_path);
    if !current.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    // Restore can inject root SSH keys — match the ssh_keys handlers' CSRF guard,
    // which update_settings does not have.
    if cross_origin(&headers) {
        return Ok(Redirect::to("/system?restore=csrf").into_response());
    }

    // Read the first uploaded field's bytes. The route-scoped DefaultBodyLimit
    // (see route registration) rejects an oversize body before we get here.
    let field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => return Ok(Redirect::to("/system?restore=empty").into_response()),
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return Ok(Redirect::to("/system?restore=too_large").into_response());
        }
        Err(error) => return Err(AppError(error.to_string())),
    };
    let data = match field.bytes().await {
        Ok(data) => data,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return Ok(Redirect::to("/system?restore=too_large").into_response());
        }
        Err(error) => return Err(AppError(error.to_string())),
    };
    let bytes = data.to_vec();
    if bytes.len() > MAX_RESTORE_BYTES {
        return Ok(Redirect::to("/system?restore=too_large").into_response());
    }

    let (restored, keys) = match backup::parse_restore(&bytes, &current) {
        Ok(result) => result,
        Err(_) => return Ok(Redirect::to("/system?restore=invalid").into_response()),
    };

    settings::save_settings(&state.config_path, &restored)
        .map_err(|error| AppError(error.to_string()))?;
    apply_settings_side_effects(&state, &restored).await?;

    // Best-effort key merge; a key-write failure does not roll back the settings
    // (both are individually atomic and settings are already committed).
    let state_dir = ssh_keys_state_dir(&state);
    let redirect = match run_blocking(move || ssh_keys::merge(&state_dir, &keys)).await? {
        Ok((added, _skipped)) => format!("/system?restore=ok&keys={added}"),
        Err(_) => "/system?restore=ok_keys_failed".to_string(),
    };
    Ok(Redirect::to(&redirect).into_response())
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

/// State directory that holds the service-user-owned temp file used to stage an
/// atomic authorized_keys rewrite (the parent of the settings file).
fn ssh_keys_state_dir(state: &AppState) -> PathBuf {
    state
        .config_path
        .parent()
        .map(|dir| dir.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/var/lib/octocam"))
}

/// Reject a state-changing POST that a browser reports came from a different
/// origin. If neither `Origin` nor `Referer` is present we allow it — the
/// session cookie is `SameSite=Lax`, which already blocks cross-site POST-form
/// submissions. This is contained defense-in-depth for the root-key surface,
/// not an app-wide CSRF-token scheme.
fn cross_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let source = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER))
        .and_then(|value| value.to_str().ok());
    let Some(source) = source else {
        return false;
    };
    let source_host = source
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
        .unwrap_or("");
    source_host != host
}

/// Canned, escape-safe message for a redirect status code. Detailed causes are
/// logged server-side; only the enumerated code travels in the URL.
fn ssh_key_message(status: Option<&str>, has_warn: bool) -> (String, bool) {
    if has_warn {
        return (
            "This is the last key authorized for root SSH — removing it ends remote \
             SSH access to this device. Confirm below only if you're sure."
                .to_string(),
            true,
        );
    }
    match status {
        Some("added") => ("SSH key authorized.".to_string(), false),
        Some("revoked") => ("SSH key revoked.".to_string(), false),
        Some("duplicate") => ("That key is already authorized.".to_string(), true),
        Some("bad_key") => (
            "That isn't a single valid public key. Paste one line like \
             'ssh-ed25519 AAAA… comment' — options and multi-line input aren't accepted."
                .to_string(),
            true,
        ),
        Some("too_long") => ("That key is too large to store.".to_string(), true),
        Some("write_failed") => (
            "Couldn't update root's authorized_keys — check the service user's sudo access."
                .to_string(),
            true,
        ),
        Some("read_failed") => (
            "Couldn't read root's authorized_keys — check the service user's sudo access."
                .to_string(),
            true,
        ),
        Some("csrf") => (
            "Request rejected because it came from another site.".to_string(),
            true,
        ),
        _ => (String::new(), false),
    }
}

async fn ssh_keys_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<SshKeysQuery>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete {
        return Ok(Redirect::to("/setup").into_response());
    }
    let status = run_blocking(system::status).await?;
    // ssh_keys::list returns Result<_, KeyError>, so run_blocking yields a
    // nested Result; both the inner Err and a join failure surface as read_error.
    let (keys, read_error) = match run_blocking(ssh_keys::list).await {
        Ok(Ok(keys)) => (keys, false),
        Ok(Err(_)) => (Vec::new(), true),
        Err(_join) => (Vec::new(), true),
    };
    let warn_fingerprint = query.warn.unwrap_or_default();
    let has_warn = !warn_fingerprint.is_empty();
    let (message, message_is_error) = ssh_key_message(query.status.as_deref(), has_warn);
    render(SshKeysTemplate {
        page_title: "SSH keys".to_string(),
        settings,
        system: system::view(&status),
        active_page: "ssh_keys",
        has_keys: !keys.is_empty(),
        keys,
        read_error,
        has_message: !message.is_empty(),
        message,
        message_is_error,
        warn_fingerprint,
        has_warn,
    })
}

async fn ssh_keys_add(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Form(form): Form<SshKeyAddForm>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    if cross_origin(&headers) {
        return Ok(Redirect::to("/ssh-keys?status=csrf").into_response());
    }
    let state_dir = ssh_keys_state_dir(&state);
    let public_key = form.public_key;
    let status = match run_blocking(move || ssh_keys::add(&state_dir, &public_key)).await {
        Ok(Ok(())) => "added",
        Ok(Err(error)) => error.code(),
        Err(_join) => "write_failed",
    };
    Ok(Redirect::to(&format!("/ssh-keys?status={status}")).into_response())
}

async fn ssh_keys_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Form(form): Form<SshKeyRevokeForm>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }
    if cross_origin(&headers) {
        return Ok(Redirect::to("/ssh-keys?status=csrf").into_response());
    }
    let state_dir = ssh_keys_state_dir(&state);
    let confirm = form.confirm.as_deref() == Some("1");
    let target = form.fingerprint;
    let warn_target = target.clone();
    let redirect = match run_blocking(move || ssh_keys::revoke(&state_dir, &target, confirm)).await
    {
        Ok(Ok(ssh_keys::RevokeOutcome::Revoked)) => "/ssh-keys?status=revoked".to_string(),
        Ok(Ok(ssh_keys::RevokeOutcome::Warn)) => {
            format!("/ssh-keys?warn={}", urlencoding::encode(&warn_target))
        }
        Ok(Err(error)) => format!("/ssh-keys?status={}", error.code()),
        Err(_join) => "/ssh-keys?status=write_failed".to_string(),
    };
    Ok(Redirect::to(&redirect).into_response())
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
    let initial_stream = if settings.sub_stream_enabled {
        "sub"
    } else {
        "main"
    }
    .to_string();
    let main_busy = false;
    let (viewers_main_text, viewers_sub_text) = match &viewers {
        Some(report) => (
            format!("{} / {}", report.main.total, report.main.capacity),
            format!("{} / {}", report.sub.total, report.sub.capacity),
        ),
        None => ("unavailable".to_string(), "unavailable".to_string()),
    };
    render(StreamTemplate {
        page_title: "Dashboard".to_string(),
        browser_stream_urls: stream_urls_for(&settings, host, "webrtc"),
        system: system::view(&status),
        settings,
        active_page: "dashboard",
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
        let (ssid, password, security) = (
            wifi_ssid.clone(),
            wifi_password.clone(),
            wifi_security.clone(),
        );
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
    let (ssid, password, security) = (
        wifi_ssid.clone(),
        wifi_password.clone(),
        wifi_security.clone(),
    );
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
    settings::enforce_hksv_requires_motion(&mut validated);
    merge_settings(&mut current, validated);
    settings::save_settings(&state.config_path, &current)
        .map_err(|error| AppError(error.to_string()))?;
    apply_settings_side_effects(&state, &current).await?;
    Ok(Redirect::to(&format!("{return_to}?saved=1")).into_response())
}

async fn sync_time(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Form(mut form): Form<HashMap<String, String>>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, false)? {
        return Ok(response);
    }

    let return_to = clean_return_path(
        &form
            .remove("_return_to")
            .unwrap_or_else(|| "/stream-settings".to_string()),
    );
    let mut current = settings::load_settings(&state.config_path);
    if let Some(time_server) = form.remove("time_server") {
        let mut next_map = settings_to_map(&current)?;
        next_map.insert("time_server".to_string(), Value::String(time_server));
        let mut validated = settings::validate_map(&next_map);
        validated.setup_complete = current.setup_complete;
        settings::enforce_matter_requires_admin(&mut validated);
        settings::enforce_hksv_requires_motion(&mut validated);
        merge_settings(&mut current, validated);
        settings::save_settings(&state.config_path, &current)
            .map_err(|error| AppError(error.to_string()))?;
    }
    let time_server = current.time_server.clone();
    run_blocking(move || system::sync_clock(&time_server))
        .await?
        .map_err(AppError)?;
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
            let _ = proc::run(
                Command::new(command).args(command_args),
                proc::SERVICE_TIMEOUT,
            );
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
        motion_detected: bool,
    }
    Ok(Json(StatusResponse {
        status: status?,
        viewers,
        motion_detected: state.motion_detected.load(std::sync::atomic::Ordering::Relaxed),
    })
    .into_response())
}

async fn api_motion_events(
    State(state): State<Arc<AppState>>,
) -> axum::response::sse::Sse<impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use tokio_stream::wrappers::BroadcastStream;
    use tokio_stream::StreamExt;

    let rx = state.motion_tx.subscribe();
    let initial_val = state.motion_detected.load(std::sync::atomic::Ordering::Relaxed);
    let initial_event = Event::default().data(serde_json::json!({ "motion_detected": initial_val }).to_string());

    let stream = BroadcastStream::new(rx)
        .map(|msg| match msg {
            Ok(val) => Ok(Event::default().data(serde_json::json!({ "motion_detected": val }).to_string())),
            Err(_) => Ok(Event::default().comment("keepalive")),
        });

    let stream = tokio_stream::once(Ok(initial_event)).chain(stream);

    Sse::new(stream).keep_alive(KeepAlive::default())
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
        has_error: !error.is_empty(),
        error,
    }
}

/// Reconfigure the downstream services from the current settings: mediamtx RTSP,
/// the HomeKit accessory daemon, and the Matter sidecar. Shared by
/// `update_settings` and `restore_upload` so the two paths cannot drift. Assumes
/// settings have already been persisted with `save_settings`.
async fn apply_settings_side_effects(
    state: &Arc<AppState>,
    settings: &Settings,
) -> Result<(), AppError> {
    let _ = mediamtx::configure_rtsp_service(settings, &state.mediamtx_config_path);
    let timezone = settings.text_overlay_timezone.clone();
    let _ = run_blocking(move || system::set_timezone(&timezone))
        .await?
        .map_err(AppError)?;
    let time_server = settings.time_server.clone();
    let _ = run_blocking(move || system::configure_time_server(&time_server))
        .await?
        .map_err(AppError)?;
    let maintenance_settings = settings.clone();
    let _ = run_blocking(move || system::configure_maintenance_timers(&maintenance_settings))
        .await?
        .map_err(AppError)?;
    let homekit_settings = settings.clone();
    run_blocking(move || configure_homekit_service(&homekit_settings)).await?;
    let matter_settings = settings.clone();
    let (matter_env, matter_id) = (
        state.matter_env_path.clone(),
        state.matter_identity_path.clone(),
    );
    run_blocking(move || {
        matter::configure_matter_service(&matter_settings, &matter_env, &matter_id)
    })
    .await?;
    Ok(())
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

fn time_zone_views(mut values: Vec<String>, current: &str) -> Vec<TimeZoneView> {
    if !values.iter().any(|value| value == current) {
        values.push(current.to_string());
    }
    values.sort();
    values.dedup();
    values
        .into_iter()
        .map(|value| TimeZoneView {
            selected: value == current,
            value,
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
    format!("http://{SETUP_AP_GATEWAY}/setup")
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

fn captive_portal_listener_enabled() -> bool {
    env::var("OCTOCAM_ENABLE_CAPTIVE_PORTAL_LISTENER")
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !matches!(normalized.as_str(), "0" | "false" | "no" | "off")
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captive_redirect_targets_the_ap_gateway() {
        // Never echo the probe's Host header (captive.apple.com etc.) — the client
        // cannot resolve it on the uplink-less AP. Always the gateway IP literal.
        assert_eq!(captive_redirect_target(), "http://10.42.0.1/setup");
    }
}
