mod api;
mod backup;
mod camera;
mod db;
mod matter;
mod mediamtx;
mod proc;
mod security;
mod settings;
mod spa;
mod ssh_keys;
mod streams;
mod system;
mod wifi;
mod wifi_setup;
mod motion;
mod mqtt;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
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

type AppResult = Result<Response, AppError>;

/// Latest snapshot bytes plus when they were captured; None until first capture.
type SnapshotCache = Arc<tokio::sync::Mutex<Option<(std::time::Instant, Vec<u8>)>>>;

#[derive(Clone)]
struct AppState {
    config_path: PathBuf,
    wifi_cache_path: PathBuf,
    mediamtx_config_path: PathBuf,
    homekit_status_path: PathBuf,
    matter_identity_path: PathBuf,
    matter_env_path: PathBuf,
    matter_status_path: PathBuf,
    matter_storage_dir: PathBuf,
    #[allow(dead_code)]
    db_path: PathBuf,
    db: db::Database,
    secret_key: String,
    snapshot_cache: SnapshotCache,
    /// Set when the loopback snapshot listener could not bind — surfaced on
    /// /matter, since the Matter daemon has no snapshot fallback.
    internal_listener_down: Arc<std::sync::atomic::AtomicBool>,
    motion_detected: Arc<std::sync::atomic::AtomicBool>,
    motion_tx: tokio::sync::broadcast::Sender<motion::MotionUpdate>,
    /// Liveness of the motion detector. Distinguishes "nothing is moving" from
    /// "the detector cannot see anything" — see motion::MotionHealth.
    motion_health: Arc<motion::MotionHealth>,
    /// What the MQTT publisher is currently doing, for the settings page.
    mqtt_status: Arc<std::sync::Mutex<mqtt::MqttStatus>>,
    /// Signals the publisher that settings changed. Every settings writer must
    /// send on this, not just the settings PUT — a restored backup can change
    /// broker configuration too, and would otherwise be ignored until restart.
    mqtt_reload_tx: tokio::sync::broadcast::Sender<()>,
}

#[derive(Debug)]
struct AppError(String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0).into_response()
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
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
struct TimeZoneView {
    value: String,
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
        state.motion_health.clone(),
    );

    // Mint this camera's MQTT identity before anything can build a topic from
    // it, and persist immediately so the identity survives a restart.
    {
        let mut settings = settings::load_settings(&state.config_path);
        if settings::ensure_mqtt_node_id(&mut settings) {
            if let Err(error) = settings::save_settings(&state.config_path, &settings) {
                tracing::warn!("could not persist generated MQTT node id: {error}");
            }
        }
    }
    mqtt::spawn_mqtt_publisher(
        state.config_path.clone(),
        state.motion_detected.clone(),
        state.motion_health.clone(),
        state.motion_tx.subscribe(),
        state.mqtt_reload_tx.subscribe(),
        state.mqtt_status.clone(),
    );

    let app = Router::new()
        .route("/", get(spa::spa_index))
        .route("/backup", get(backup_download))
        .route("/hotspot-detect.html", get(captive_probe))
        .route("/generate_204", get(captive_probe))
        .route("/api/login", post(api_login))
        .route("/api/logout", post(api_logout))
        .route("/api/setup", get(api_setup_get).post(api_setup_post))
        .route("/api/settings", get(api_settings).put(api_settings_update))
        .route("/api/status", get(api_status))
        .route("/api/identity", get(api_identity))
        .route("/api/rtsp", get(api_rtsp))
        .route("/api/system", get(api_system))
        .route("/api/stream-options", get(api_stream_options))
        .route("/api/logs", get(api_logs))
        .route("/api/homekit", get(api_homekit))
        .route("/api/matter", get(api_matter))
        .route("/api/matter/reset", post(api_matter_reset))
        .route("/api/power", post(api_power))
        .route("/api/time/sync", post(api_time_sync))
        .route("/api/me", get(api_me))
        .route("/api/motion/events", get(api_motion_events))
        .route("/api/mqtt/status", get(api_mqtt_status))
        .route("/api/wifi/networks", get(api_wifi_networks))
        .route("/api/wifi/scan", post(api_wifi_scan))
        .route("/api/wifi/connect", post(api_wifi_connect))
        .route("/api/wifi/delete", delete(api_wifi_delete))
        .route("/api/wifi/saved", get(api_wifi_saved))
        .route("/api/passkey/register/start", post(api_passkey_register_start))
        .route("/api/passkey/register/finish", post(api_passkey_register_finish))
        .route("/api/passkey/login/start", post(api_passkey_login_start))
        .route("/api/passkey/login/finish", post(api_passkey_login_finish))
        .route("/api/passkeys", get(api_passkeys_list))
        .route("/api/passkey/{id}", delete(api_passkey_delete))
        .route("/api/passkey/{id}/rename", post(api_passkey_rename))
        .route("/api/users", get(api_users_list))
        .route("/api/users/add", post(api_users_add))
        .route("/api/users/{id}", delete(api_users_delete))
        .route("/api/ssh-keys", get(api_ssh_keys_list).post(api_ssh_keys_add).delete(api_ssh_keys_delete))
        .route(
            "/api/restore",
            post(api_restore).layer(DefaultBodyLimit::max(MAX_RESTORE_BYTES)),
        )
        .route("/snapshot.jpg", get(snapshot))
        .fallback(spa::spa_asset)
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
        let config_path = settings::default_config_path();
        let wifi_cache_path = wifi::default_cache_path();
        let mediamtx_config_path = mediamtx::default_config_path();
        let homekit_status_path = env::var_os("OCTOCAM_HOMEKIT_STATUS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/homekit-status.json"));
        let db_path = env::var_os("OCTOCAM_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/octocam.db"));
        let db = db::Database::init(&db_path).expect("initialize SQLite database");

        let settings = settings::load_settings(&config_path);
        if !settings.admin_password_hash.is_empty() {
            let _ = db.migrate_legacy_password(&settings.admin_password_hash);
        }

        let secret_key = load_secret_key();
        let (motion_tx, _) = tokio::sync::broadcast::channel(32);
        Self {
            config_path,
            wifi_cache_path,
            mediamtx_config_path,
            homekit_status_path,
            matter_identity_path: matter::default_identity_path(),
            matter_env_path: matter::default_env_path(),
            matter_status_path: matter::default_status_path(),
            matter_storage_dir: matter::default_storage_dir(),
            db_path,
            db,
            secret_key,
            snapshot_cache: Arc::new(tokio::sync::Mutex::new(None)),
            internal_listener_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            motion_detected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            motion_tx,
            motion_health: Arc::new(motion::MotionHealth::default()),
            mqtt_status: Arc::new(std::sync::Mutex::new(mqtt::MqttStatus::default())),
            mqtt_reload_tx: {
                let (tx, _) = tokio::sync::broadcast::channel(8);
                tx
            },
        }
    }
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

/// JSON-facing config restore for the React System page: validates admin auth
/// and CSRF, parses the uploaded backup via `backup::parse_restore`, persists
/// it, applies the same side effects as `api_settings_update`, and best-effort
/// merges any bundled SSH keys via `ssh_keys::merge`. Returns a `fetch()`-able
/// `{success, keys_added, keys_failed}` / `{error, code}` body.
async fn api_restore(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    mut multipart: Multipart,
) -> api::ApiResult {
    let current = settings::load_settings(&state.config_path);
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    // Restore can inject root SSH keys — match restore_upload's CSRF guard,
    // which update_settings/api_settings_update do not have.
    if cross_origin(&headers) {
        return Err(api::ApiError::new(StatusCode::FORBIDDEN, "Cross-origin request rejected")
            .with_code("csrf"));
    }

    // Read the first uploaded field's bytes. The route-scoped DefaultBodyLimit
    // (see route registration) rejects an oversize body before we get here.
    let field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => {
            return Err(api::ApiError::bad_request("No backup file uploaded").with_code("empty"));
        }
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return Err(api::ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Backup file is too large")
                .with_code("too_large"));
        }
        Err(error) => return Err(api::ApiError::internal(error.to_string())),
    };
    let data = match field.bytes().await {
        Ok(data) => data,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return Err(api::ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Backup file is too large")
                .with_code("too_large"));
        }
        Err(error) => return Err(api::ApiError::internal(error.to_string())),
    };
    let bytes = data.to_vec();
    if bytes.len() > MAX_RESTORE_BYTES {
        return Err(api::ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Backup file is too large")
            .with_code("too_large"));
    }

    let (restored, keys) = match backup::parse_restore(&bytes, &current) {
        Ok(result) => result,
        Err(_) => {
            return Err(api::ApiError::bad_request("Backup file is invalid").with_code("invalid"));
        }
    };

    notify_settings_changed(&state);
    settings::save_settings(&state.config_path, &restored)
        .map_err(|error| api::ApiError::internal(error.to_string()))?;
    apply_settings_side_effects(&state, &restored)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;

    // Best-effort key merge; a key-write failure does not roll back the settings
    // (both are individually atomic and settings are already committed) — mirrors
    // restore_upload's `ok_keys_failed` case, just as a count instead of a status.
    let requested_keys = keys.len();
    let state_dir = ssh_keys_state_dir(&state);
    let (keys_added, keys_failed) = match run_blocking(move || ssh_keys::merge(&state_dir, &keys))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        Ok((added, skipped)) => (added, skipped),
        Err(_) => (0, requested_keys),
    };

    Ok(api::ok_json(serde_json::json!({
        "success": true,
        "keys_added": keys_added,
        "keys_failed": keys_failed,
    })))
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

/// JSON-facing view of an `ssh_keys::AuthorizedKey`.
#[derive(Serialize)]
struct SshKeyDto {
    key_type: String,
    comment: String,
    fingerprint: String,
    preview: String,
}

async fn api_ssh_keys_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    // ssh_keys::list takes no args and returns Result<_, KeyError>, so
    // run_blocking yields a nested Result (join failure, then read failure).
    let keys = run_blocking(ssh_keys::list)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?
        .map_err(|_| api::ApiError::service_unavailable("Could not read authorized keys"))?;
    let dtos: Vec<SshKeyDto> = keys
        .into_iter()
        .map(|k| SshKeyDto {
            key_type: k.key_type,
            comment: k.comment,
            fingerprint: k.fingerprint,
            preview: k.preview,
        })
        .collect();
    Ok(api::ok_json(dtos))
}

#[derive(Deserialize)]
struct SshKeyAddReq {
    public_key: String,
}

async fn api_ssh_keys_add(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(req): Json<SshKeyAddReq>,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    // cross_origin() returns true when the request IS cross-site, so reject
    // when it's true (matches the form handler's `if cross_origin(&headers)`).
    if cross_origin(&headers) {
        return Err(api::ApiError::new(
            StatusCode::FORBIDDEN,
            "Cross-origin request rejected",
        ));
    }
    let dir = ssh_keys_state_dir(&state);
    run_blocking(move || ssh_keys::add(&dir, &req.public_key))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?
        .map_err(|e| api::ApiError::bad_request(e.code()))?;
    Ok(api::ok_json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct SshKeyDeleteReq {
    fingerprint: String,
    #[serde(default)]
    confirm: bool,
}

async fn api_ssh_keys_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(req): Json<SshKeyDeleteReq>,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    if cross_origin(&headers) {
        return Err(api::ApiError::new(
            StatusCode::FORBIDDEN,
            "Cross-origin request rejected",
        ));
    }
    let dir = ssh_keys_state_dir(&state);
    let outcome = run_blocking(move || ssh_keys::revoke(&dir, &req.fingerprint, req.confirm))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?
        .map_err(|e| api::ApiError::bad_request(e.code()))?;
    match outcome {
        ssh_keys::RevokeOutcome::Warn => Err(api::ApiError::conflict(
            "This is the last key; resend with confirm=true to remove it",
        )
        .with_code("last_key")),
        ssh_keys::RevokeOutcome::Revoked => Ok(api::ok_json(serde_json::json!({ "success": true }))),
    }
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

/// JSON counterpart of the removed Askama login/authenticate handlers. Mirrors
/// the same credential check exactly (`verify_password(password, hash)` order)
/// but returns a JSON body instead of a redirect, and 401s on bad credentials.
#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

async fn api_login(State(state): State<Arc<AppState>>, Json(req): Json<LoginReq>) -> Response {
    match state.db.get_user_by_username(&req.username) {
        Ok(Some(user)) if security::verify_password(&req.password, &user.password_hash) => {
            let body = Json(serde_json::json!({
                "success": true,
                "username": user.username,
                "role": user.role,
                "is_admin": user.is_admin(),
            }))
            .into_response();
            with_login_cookie_for_user(body, &state, user.id, &user.username)
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Invalid credentials" })),
        )
            .into_response(),
    }
}

/// Clears the session cookie (same cookie string the old Askama `/logout`
/// route used, kept verbatim for any lingering clients).
async fn api_logout() -> Response {
    let mut response = Json(serde_json::json!({ "success": true })).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("octocam_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
    );
    response
}

/// No auth — used by the pre-setup wizard to decide whether to show the
/// setup flow at all.
async fn api_setup_get(State(state): State<Arc<AppState>>) -> Response {
    let settings = settings::load_settings(&state.config_path);
    let needed = !settings.setup_complete || !state.db.has_users().unwrap_or(false);
    api::ok_json(serde_json::json!({ "setup_required": needed }))
}

/// Completes first-run setup: password-match check, blocking Wi-Fi join (if an
/// SSID was given), hash + create the first admin user, inject the
/// setup_complete/camera_enabled/homekit_enabled/admin_password_hash fields
/// into the settings map, validate/merge/save, configure the HomeKit
/// service, then set the session cookie.
///
/// The body arrives as typed JSON (bools/numbers), so we merge it into a
/// `Map<String, Value>` and call `validate_map` directly — the same
/// native-JSON path `api_settings_update` already uses above.
/// `homekit_enabled` uses *presence* of the key (not its value), matching
/// HTML checkbox semantics.
async fn api_setup_post(
    State(state): State<Arc<AppState>>,
    Json(mut body): Json<serde_json::Map<String, serde_json::Value>>,
) -> Response {
    let mut current = settings::load_settings(&state.config_path);

    let admin_username = body
        .remove("admin_username")
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "admin".to_string());
    let admin_password = body
        .remove("admin_password")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let admin_password_confirm = body
        .remove("admin_password_confirm")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let wifi_password = body
        .remove("wifi_password")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let wifi_ssid = body
        .get("wifi_ssid")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_default();
    let cache = wifi::load_network_cache(&state.wifi_cache_path);
    let wifi_security = wifi::cached_security_for(&cache, &wifi_ssid);

    if admin_password != admin_password_confirm {
        return Json(serde_json::json!({
            "success": false,
            "field": "admin_password_confirm",
            "message": "Admin passwords do not match.",
        }))
        .into_response();
    }

    if !wifi_ssid.trim().is_empty() {
        let (ssid, password, security) = (
            wifi_ssid.clone(),
            wifi_password.clone(),
            wifi_security.clone(),
        );
        let connected = match run_blocking(move || wifi::connect_to_network(&ssid, &password, &security))
            .await
        {
            Ok(pair) => pair,
            Err(error) => return api::ApiError::internal(error.0).into_response(),
        };
        let (connected, message) = connected;
        if !connected {
            return Json(serde_json::json!({
                "success": false,
                "field": "wifi",
                "message": message,
            }))
            .into_response();
        }
    }

    let password_hash = security::hash_password(&admin_password);
    let user = match state.db.create_user(&admin_username, &password_hash, "admin") {
        Ok(user) => user,
        Err(error) => return api::ApiError::internal(error.to_string()).into_response(),
    };

    body.insert("setup_complete".to_string(), Value::Bool(true));
    body.insert("camera_enabled".to_string(), Value::Bool(true));
    let homekit_enabled = body.contains_key("homekit_enabled");
    body.insert("homekit_enabled".to_string(), Value::Bool(homekit_enabled));
    body.insert("admin_password_hash".to_string(), Value::String(password_hash));
    let validated = settings::validate_map(&body);
    merge_settings(&mut current, validated);
    notify_settings_changed(&state);
    if let Err(error) = settings::save_settings(&state.config_path, &current) {
        return api::ApiError::internal(error.to_string()).into_response();
    }

    let homekit_settings = current.clone();
    if let Err(error) = run_blocking(move || configure_homekit_service(&homekit_settings)).await {
        return api::ApiError::internal(error.0).into_response();
    }

    let body_json = Json(serde_json::json!({ "success": true })).into_response();
    with_login_cookie_for_user(body_json, &state, user.id, &user.username)
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

/// Updates settings from a JSON body, with two branches:
///   - non-admin: may only change their own password (admin_password /
///     admin_password_confirm), everything else is ignored.
///   - admin: full dynamic-map merge, run through the settings invariant
///     pipeline (validate_map -> enforce_matter_requires_admin ->
///     enforce_hksv_requires_motion -> merge_settings).
///
/// Incoming JSON values are merged directly into the settings map without
/// any string coercion: `settings::validate_map`'s field readers
/// (`bool_value`/`int_value`/`string_value`/...) already accept
/// `Value::Bool`/`Value::Number`/`Value::String` natively (see settings.rs),
/// so a JSON client sending real booleans/numbers works out of the box.
/// Keys prefixed with `_` are skipped (reserved for client-side control
/// fields).
async fn api_settings_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(mut body): Json<serde_json::Map<String, serde_json::Value>>,
) -> api::ApiResult {
    if let Some(resp) = require_user_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let user = authenticated_user(&state, &headers);
    let is_admin = user.as_ref().map(|u| u.is_admin()).unwrap_or(false);

    let admin_username = body
        .remove("admin_username")
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.trim().is_empty());
    let admin_password = body
        .remove("admin_password")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let admin_password_confirm = body
        .remove("admin_password_confirm")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();

    if !is_admin {
        if admin_password.is_empty() || admin_password != admin_password_confirm {
            return Err(api::ApiError::bad_request(
                "Password fields are empty or do not match.",
            ));
        }
        if let Some(user) = user {
            let new_hash = security::hash_password(&admin_password);
            let _ = state.db.update_password(user.id, &new_hash);
        }
        return Ok(api::ok_json(serde_json::json!({ "success": true })));
    }

    let mut current = settings::load_settings(&state.config_path);
    if !admin_password.is_empty() || admin_username.is_some() {
        if admin_password != admin_password_confirm && !admin_password.is_empty() {
            return Err(api::ApiError::bad_request(
                "Password fields do not match.",
            ));
        }
        if let Some(user) = &user {
            let new_hash = if !admin_password.is_empty() {
                security::hash_password(&admin_password)
            } else {
                user.password_hash.clone()
            };
            let _ = state.db.update_password(user.id, &new_hash);
        }
    }

    let mut next_map = settings_to_map(&current).map_err(|e| api::ApiError::internal(e.0))?;
    for (key, value) in body {
        if key.starts_with('_') {
            continue;
        }
        next_map.insert(key, value);
    }
    settings::validate_mqtt_submission(&next_map).map_err(api::ApiError::bad_request)?;
    let mut validated = settings::validate_map(&next_map);
    if admin_password.is_empty() && admin_password_confirm.is_empty() {
        validated.admin_password_hash = current.admin_password_hash.clone();
    } else {
        if admin_password != admin_password_confirm {
            return Err(api::ApiError::bad_request(
                "Password fields do not match.",
            ));
        }
        validated.admin_password_hash = security::hash_password(&admin_password);
    }
    validated.setup_complete = current.setup_complete;
    settings::enforce_matter_requires_admin(&mut validated);
    settings::enforce_hksv_requires_motion(&mut validated);
    merge_settings(&mut current, validated);
    settings::save_settings(&state.config_path, &current)
        .map_err(|error| api::ApiError::internal(error.to_string()))?;
    notify_settings_changed(&state);
    apply_settings_side_effects(&state, &current)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;

    let saved = settings::load_settings(&state.config_path);
    Ok(api::ok_json(serde_json::json!({
        "success": true,
        "settings": settings::public_settings(&saved),
    })))
}

#[derive(Serialize)]
struct BrowserStreamUrls {
    main: String,
    sub: String,
    has_sub: bool,
}

async fn api_status(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_user_login(&state, &headers, &uri, true)? {
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
        /// Whether `motion_detected` is trustworthy right now.
        motion_health: motion::MotionHealthView,
        browser_stream_urls: BrowserStreamUrls,
    }
    let urls = stream_urls_for(&settings, request_hostname(&headers), "webrtc");
    Ok(Json(StatusResponse {
        status: status?,
        viewers,
        motion_detected: state.motion_detected.load(std::sync::atomic::Ordering::Relaxed),
        motion_health: state.motion_health.snapshot(),
        browser_stream_urls: BrowserStreamUrls {
            main: urls.main,
            sub: urls.sub,
            has_sub: urls.has_sub,
        },
    })
    .into_response())
}

async fn api_identity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let settings = settings::load_settings(&state.config_path);
    let status = run_blocking(system::status)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    Ok(api::ok_json(serde_json::json!({
        "settings": settings::public_settings(&settings),
        "system": status,
    })))
}

#[derive(Serialize)]
struct RtspUrls {
    main: String,
    sub: String,
    has_sub: bool,
}

async fn api_rtsp(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let settings = settings::load_settings(&state.config_path);
    let urls = stream_urls_for(&settings, request_hostname(&headers), "rtsp");
    Ok(api::ok_json(RtspUrls {
        main: urls.main,
        sub: urls.sub,
        has_sub: urls.has_sub,
    }))
}

async fn api_system(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let status = run_blocking(system::status)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    Ok(api::ok_json(status))
}

async fn api_stream_options(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let settings = settings::load_settings(&state.config_path);
    let time_zone_values = run_blocking(system::available_time_zones)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    let timezones: Vec<String> =
        time_zone_views(time_zone_values, &settings.text_overlay_timezone)
            .into_iter()
            .map(|zone| zone.value)
            .collect();
    Ok(api::ok_json(serde_json::json!({
        "resolution_presets": preset_views(RESOLUTION_PRESETS, &settings.current_resolution()),
        "sub_resolution_presets": preset_views(
            SUB_RESOLUTION_PRESETS,
            &settings.current_sub_resolution(),
        ),
        "timezones": timezones,
        "rotations": [0, 90, 180, 270],
    })))
}

async fn api_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let status = run_blocking(system::status)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    Ok(api::ok_json(serde_json::json!({ "lines": status.logs })))
}

async fn api_homekit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let settings = settings::load_settings(&state.config_path);
    let view = homekit_view(&state.homekit_status_path, &settings);
    Ok(api::ok_json(serde_json::json!({
        "status": view.status,
        "paired": view.paired,
        "has_pairing": view.has_pairing,
        "pincode": view.pincode,
        "setup_uri": view.setup_uri,
        "has_qr": view.has_qr,
        "qr_data_url": view.qr_data_url,
        "error": view.error,
        "has_error": view.has_error,
    })))
}

async fn api_matter(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let settings = settings::load_settings(&state.config_path);
    let identity = if settings.matter_enabled {
        matter::load_or_generate_identity(&state.matter_identity_path).ok()
    } else {
        None
    };
    let matter_status = matter::read_status(&state.matter_status_path);
    let mut view = matter::view(&settings, identity.as_ref(), &matter_status);
    view.snapshot_endpoint_down = state
        .internal_listener_down
        .load(std::sync::atomic::Ordering::Relaxed);
    Ok(api::ok_json(serde_json::json!({
        "status": view.status,
        "commissioned": view.commissioned,
        "fabric_count": view.fabric_count,
        "orphaned_fabrics": view.orphaned_fabrics,
        "manual_code": view.manual_code,
        "qr_svg": view.qr_svg,
        "qr_payload": view.qr_payload,
        "stream_source": view.stream_source,
        "error": view.error,
        "has_error": view.has_error,
        "ipv6_ok": view.ipv6_ok,
        "admin_password_set": view.admin_password_set,
        "snapshot_endpoint_down": view.snapshot_endpoint_down,
    })))
}

async fn api_matter_reset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let settings = settings::load_settings(&state.config_path);
    let (storage, env_path, id_path) = (
        state.matter_storage_dir.clone(),
        state.matter_env_path.clone(),
        state.matter_identity_path.clone(),
    );
    run_blocking(move || matter::reset_pairing(&settings, &storage, &env_path, &id_path))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    Ok(api::ok_json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct PowerReq {
    action: String,
}

async fn api_power(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(req): Json<PowerReq>,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    if !matches!(
        req.action.as_str(),
        "restart_service" | "restart_device" | "shutdown_device"
    ) {
        return Err(api::ApiError::bad_request(format!(
            "Unknown power action: {}",
            req.action
        )));
    }
    schedule_power_action(&req.action).map_err(|e| api::ApiError::internal(e.0))?;
    // Fire-and-forget (mirrors the form handler): the systemctl call runs after
    // a short delay; we can only confirm it was scheduled.
    Ok(api::ok_json(serde_json::json!({
        "success": true,
        "scheduled": req.action,
    })))
}

#[derive(Deserialize)]
struct TimeSyncReq {
    time_server: Option<String>,
}

async fn api_time_sync(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(req): Json<TimeSyncReq>,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let mut current = settings::load_settings(&state.config_path);
    if let Some(time_server) = req.time_server.clone() {
        let mut next_map = settings_to_map(&current).map_err(|e| api::ApiError::internal(e.0))?;
        next_map.insert("time_server".to_string(), Value::String(time_server));
        let mut validated = settings::validate_map(&next_map);
        validated.setup_complete = current.setup_complete;
        settings::enforce_matter_requires_admin(&mut validated);
        settings::enforce_hksv_requires_motion(&mut validated);
        merge_settings(&mut current, validated);
        notify_settings_changed(&state);
        settings::save_settings(&state.config_path, &current)
            .map_err(|error| api::ApiError::internal(error.to_string()))?;
    }
    let time_server = current.time_server.clone();
    run_blocking(move || system::sync_clock(&time_server))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?
        .map_err(api::ApiError::internal)?;
    Ok(api::ok_json(serde_json::json!({ "success": true })))
}

async fn api_me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let settings = settings::load_settings(&state.config_path);
    let setup_required = !settings.setup_complete
        || !state.db.has_users().unwrap_or(false);
    match authenticated_user(&state, &headers) {
        Some(user) => api::ok_json(serde_json::json!({
            "authenticated": true,
            "id": user.id,
            "username": user.username,
            "role": user.role,
            "is_admin": user.is_admin(),
            "setup_required": setup_required,
        })),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "authenticated": false,
                "setup_required": setup_required,
            })),
        )
            .into_response(),
    }
}

/// Current MQTT publisher state, for the settings page.
///
/// Admin-only and read-only: it exposes broker connectivity and the reason a
/// connection is failing, which is operational detail a non-admin has no need
/// for. There is deliberately no way to drive the publisher from here.
async fn api_mqtt_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    let snapshot = state
        .mqtt_status
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    Ok(Json(snapshot).into_response())
}

/// Tells the MQTT publisher its configuration may have changed.
///
/// Called by every settings writer rather than only the settings PUT. Restore
/// is the one that matters: broker fields travel in backups, so a restored
/// backup silently changes MQTT configuration, and without this the publisher
/// would keep using the old broker until the next service restart.
fn notify_settings_changed(state: &AppState) {
    let _ = state.mqtt_reload_tx.send(());
}

async fn api_motion_events(
    State(state): State<Arc<AppState>>,
) -> axum::response::sse::Sse<impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use tokio_stream::wrappers::BroadcastStream;
    use tokio_stream::StreamExt;

    let rx = state.motion_tx.subscribe();
    // Seed with current truth so a subscriber that connects during an outage
    // learns the sensor is blind immediately, rather than assuming it is well
    // until the next transition.
    let initial = motion::MotionUpdate {
        motion_detected: state.motion_detected.load(std::sync::atomic::Ordering::Relaxed),
        motion_available: state.motion_health.snapshot().available,
    };
    let initial_event = Event::default().data(serde_json::to_string(&initial).unwrap_or_default());

    let stream = BroadcastStream::new(rx)
        .map(|msg| match msg {
            Ok(update) => Ok(Event::default()
                .data(serde_json::to_string(&update).unwrap_or_default())),
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

#[derive(Deserialize)]
struct WifiConnectReq {
    ssid: String,
    #[serde(default)]
    password: String,
    security: Option<String>,
}

async fn api_wifi_connect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(req): Json<WifiConnectReq>,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    if req.ssid.trim().is_empty() {
        return Err(api::ApiError::bad_request("ssid is required"));
    }

    let cache = wifi::load_network_cache(&state.wifi_cache_path);
    let security = req
        .security
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| wifi::cached_security_for(&cache, &req.ssid));

    let (ssid, password) = (req.ssid.clone(), req.password.clone());
    let (connected, message) = run_blocking(move || wifi::connect_to_network(&ssid, &password, &security))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    if connected {
        Ok(api::ok_json(serde_json::json!({ "success": true, "message": message })))
    } else {
        Err(api::ApiError::bad_request(message))
    }
}

#[derive(Deserialize)]
struct WifiDeleteReq {
    name: String,
    #[serde(default)]
    source: Option<String>,
}

async fn api_wifi_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(req): Json<WifiDeleteReq>,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    if req.name.trim().is_empty() {
        return Err(api::ApiError::bad_request("name is required"));
    }

    let active_ssid = run_blocking(system::status)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?
        .wifi
        .ssid;
    if active_ssid.as_deref() == Some(req.name.trim()) {
        return Err(api::ApiError::bad_request(
            "Cannot delete the currently connected network.",
        ));
    }

    let (name, source) = (req.name.clone(), req.source.clone().unwrap_or_default());
    let (deleted, message) = run_blocking(move || wifi::forget_saved_profile(&name, &source))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    if deleted {
        Ok(api::ok_json(serde_json::json!({ "success": true, "message": message })))
    } else {
        Err(api::ApiError::bad_request(message))
    }
}

async fn api_wifi_saved(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> api::ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true)
        .map_err(|e| api::ApiError::internal(e.0))?
    {
        return Ok(resp);
    }
    let status = run_blocking(system::status)
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    let profiles = run_blocking(move || system::stored_wifi_profiles(&status.wifi))
        .await
        .map_err(|e| api::ApiError::internal(e.0))?;
    Ok(api::ok_json(profiles))
}

use base64::{engine::general_purpose::URL_SAFE, Engine};

#[derive(Deserialize)]
struct PasskeyRegStartReq {
    #[allow(dead_code)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct PasskeyRegFinishReq {
    challenge_id: String,
    id: String,
    #[allow(dead_code)]
    rawId: Option<String>,
    response: PasskeyRegFinishResponse,
    name: String,
}

#[derive(Deserialize)]
struct PasskeyRegFinishResponse {
    #[allow(dead_code)]
    clientDataJSON: String,
    attestationObject: String,
}

#[derive(Deserialize)]
struct PasskeyLoginFinishReq {
    challenge_id: String,
    id: String,
    #[allow(dead_code)]
    rawId: Option<String>,
    #[allow(dead_code)]
    response: PasskeyLoginFinishResponse,
}

#[derive(Deserialize)]
struct PasskeyLoginFinishResponse {
    #[allow(dead_code)]
    clientDataJSON: String,
    #[allow(dead_code)]
    authenticatorData: String,
    #[allow(dead_code)]
    signature: String,
    #[allow(dead_code)]
    userHandle: Option<String>,
}

async fn api_passkey_register_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult {
    let Some((user_id, username)) = authenticated(&state, &headers) else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };

    let challenge_bytes = security::generate_random_bytes(32);
    let challenge_id = security::encode_base64_url(&security::generate_random_bytes(16));
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + 300;

    let _ = state
        .db
        .save_challenge(&challenge_id, &challenge_bytes, Some(user_id), "register", expires_at);

    let host = request_hostname(&headers);
    let challenge_b64 = security::encode_base64_url(&challenge_bytes);
    let user_id_b64 = security::encode_base64_url(user_id.to_string().as_bytes());

    Ok(Json(serde_json::json!({
        "challenge_id": challenge_id,
        "publicKey": {
            "rp": { "name": "OctoCam", "id": host },
            "user": {
                "id": user_id_b64,
                "name": username,
                "displayName": username
            },
            "challenge": challenge_b64,
            "pubKeyCredParams": [
                { "type": "public-key", "alg": -7 },
                { "type": "public-key", "alg": -257 }
            ],
            "authenticatorSelection": {
                "residentKey": "required",
                "requireResidentKey": true,
                "userVerification": "preferred"
            },
            "timeout": 60000
        }
    }))
    .into_response())
}

async fn api_passkey_register_finish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyRegFinishReq>,
) -> AppResult {
    let Some((user_id, _)) = authenticated(&state, &headers) else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };

    let Ok(Some((_challenge, challenge_user_id, purpose))) =
        state.db.get_challenge(&payload.challenge_id)
    else {
        return Ok(Json(serde_json::json!({ "success": false, "error": "Invalid or expired challenge" })).into_response());
    };

    if purpose != "register" || challenge_user_id != Some(user_id) {
        return Ok(Json(serde_json::json!({ "success": false, "error": "Challenge mismatch" })).into_response());
    }

    let _ = state.db.delete_challenge(&payload.challenge_id);

    let cred_id_bytes = security::decode_base64_url(&payload.id)
        .or_else(|_| security::decode_base64_url(payload.rawId.as_deref().unwrap_or_default()))
        .unwrap_or_else(|_| payload.id.as_bytes().to_vec());

    let pubkey_bytes = security::decode_base64_url(&payload.response.attestationObject)
        .unwrap_or_default();

    match state
        .db
        .add_passkey(user_id, &cred_id_bytes, &pubkey_bytes, &payload.name, None)
    {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true })).into_response()),
        Err(err) => Ok(Json(serde_json::json!({ "success": false, "error": err.to_string() })).into_response()),
    }
}

async fn api_passkey_login_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult {
    let challenge_bytes = security::generate_random_bytes(32);
    let challenge_id = security::encode_base64_url(&security::generate_random_bytes(16));
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + 300;

    let _ = state
        .db
        .save_challenge(&challenge_id, &challenge_bytes, None, "login", expires_at);

    let host = request_hostname(&headers);
    let challenge_b64 = security::encode_base64_url(&challenge_bytes);
    let passkeys = state.db.list_all_passkeys().unwrap_or_default();
    let allow_credentials: Vec<serde_json::Value> = passkeys
        .iter()
        .map(|pk| {
            serde_json::json!({
                "type": "public-key",
                "id": security::encode_base64_url(&pk.credential_id)
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "challenge_id": challenge_id,
        "publicKey": {
            "rpId": host,
            "challenge": challenge_b64,
            "timeout": 60000,
            "userVerification": "preferred",
            "allowCredentials": allow_credentials
        }
    }))
    .into_response())
}

async fn api_passkey_login_finish(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PasskeyLoginFinishReq>,
) -> AppResult {
    let Ok(Some((_challenge, _, purpose))) = state.db.get_challenge(&payload.challenge_id) else {
        return Ok(Json(serde_json::json!({ "success": false, "error": "Invalid or expired challenge" })).into_response());
    };

    if purpose != "login" {
        return Ok(Json(serde_json::json!({ "success": false, "error": "Challenge mismatch" })).into_response());
    }

    let _ = state.db.delete_challenge(&payload.challenge_id);

    let cred_id_bytes = security::decode_base64_url(&payload.id)
        .or_else(|_| security::decode_base64_url(payload.rawId.as_deref().unwrap_or_default()))
        .unwrap_or_else(|_| payload.id.as_bytes().to_vec());

    let Some(passkey) = state.db.get_passkey_by_credential_id(&cred_id_bytes)? else {
        return Ok(Json(serde_json::json!({ "success": false, "error": "Passkey not found" })).into_response());
    };

    let Some(user) = state.db.get_user_by_id(passkey.user_id)? else {
        return Ok(Json(serde_json::json!({ "success": false, "error": "User account not found" })).into_response());
    };

    let _ = state.db.update_passkey_counter(passkey.id, passkey.counter + 1);

    let response = Json(serde_json::json!({ "success": true, "redirect": "/" })).into_response();
    Ok(with_login_cookie_for_user(response, &state, user.id, &user.username))
}

async fn api_passkeys_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult {
    let Some((user_id, _)) = authenticated(&state, &headers) else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };

    let passkeys = state.db.list_passkeys_for_user(user_id).unwrap_or_default();
    Ok(Json(passkeys).into_response())
}

async fn api_passkey_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> AppResult {
    let Some((user_id, _)) = authenticated(&state, &headers) else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };

    match state.db.delete_passkey(id, user_id) {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true })).into_response()),
        Err(err) => Ok(Json(serde_json::json!({ "success": false, "error": err.to_string() })).into_response()),
    }
}

#[derive(Deserialize)]
struct RenamePasskeyReq {
    name: String,
}

async fn api_passkey_rename(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<RenamePasskeyReq>,
) -> AppResult {
    let Some((user_id, _)) = authenticated(&state, &headers) else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    let name = payload.name.trim();
    if name.is_empty() {
        return Ok(Json(serde_json::json!({ "success": false, "error": "Name required" })).into_response());
    }

    match state.db.update_passkey_name(id, user_id, name) {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true })).into_response()),
        Err(err) => Ok(Json(serde_json::json!({ "success": false, "error": err.to_string() })).into_response()),
    }
}

#[derive(Deserialize)]
struct CreateUserReq {
    username: String,
    password: String,
    role: Option<String>,
}

async fn api_users_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    let users = state.db.list_users().unwrap_or_default();
    let sanitized: Vec<serde_json::Value> = users
        .into_iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "username": u.username,
                "role": u.role,
                "created_at": u.created_at
            })
        })
        .collect();
    Ok(Json(sanitized).into_response())
}

async fn api_users_add(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    Json(payload): Json<CreateUserReq>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    let username = payload.username.trim();
    if username.is_empty() || payload.password.trim().is_empty() {
        return Ok(Json(serde_json::json!({ "success": false, "error": "Username and password required" })).into_response());
    }
    let role = payload.role.as_deref().unwrap_or("viewer");
    let role_normalized = if role == "admin" { "admin" } else { "viewer" };
    let hash = security::hash_password(&payload.password);
    match state.db.create_user(username, &hash, role_normalized) {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true })).into_response()),
        Err(err) => Ok(Json(serde_json::json!({ "success": false, "error": err.to_string() })).into_response()),
    }
}

async fn api_users_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: Uri,
    AxumPath(id): AxumPath<i64>,
) -> AppResult {
    if let Some(response) = require_admin_login(&state, &headers, &uri, true)? {
        return Ok(response);
    }
    let current_user = authenticated_user(&state, &headers);
    if let Some(u) = current_user {
        if u.id == id {
            return Ok(Json(serde_json::json!({ "success": false, "error": "Cannot delete your own active user account" })).into_response());
        }
    }
    match state.db.delete_user(id) {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true })).into_response()),
        Err(err) => Ok(Json(serde_json::json!({ "success": false, "error": err.to_string() })).into_response()),
    }
}

async fn snapshot(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> AppResult {
    if let Some(response) = require_user_login(&state, &headers, &uri, true)? {
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
/// `api_settings_update` and `api_restore` so the two paths cannot drift. Assumes
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

fn authenticated_user(state: &AppState, headers: &HeaderMap) -> Option<db::User> {
    let (user_id, _) = authenticated(state, headers)?;
    state.db.get_user_by_id(user_id).ok().flatten()
}

fn require_login(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    api: bool,
    require_admin: bool,
) -> Result<Option<Response>, AppError> {
    let settings = settings::load_settings(&state.config_path);
    if !settings.setup_complete || !state.db.has_users().unwrap_or(false) {
        return Ok(None);
    }
    let user = authenticated_user(state, headers);
    if let Some(user) = &user {
        if require_admin && !user.is_admin() {
            if api {
                return Ok(Some(
                    (StatusCode::FORBIDDEN, "Admin privilege required.\n").into_response(),
                ));
            }
            return Ok(Some(
                Redirect::to("/dashboard?error=access_denied").into_response(),
            ));
        }
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

fn require_admin_login(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    api: bool,
) -> Result<Option<Response>, AppError> {
    require_login(state, headers, uri, api, true)
}

fn require_user_login(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    api: bool,
) -> Result<Option<Response>, AppError> {
    require_login(state, headers, uri, api, false)
}

fn authenticated(state: &AppState, headers: &HeaderMap) -> Option<(i64, String)> {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())?;
    cookie_header
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| {
            if name == SESSION_COOKIE {
                security::verify_session_for_user(&state.secret_key, value)
            } else {
                None
            }
        })
}

#[allow(dead_code)]
fn with_login_cookie(response: Response, state: &AppState) -> Response {
    with_login_cookie_for_user(response, state, 1, "admin")
}

fn with_login_cookie_for_user(
    mut response: Response,
    state: &AppState,
    user_id: i64,
    username: &str,
) -> Response {
    let cookie = format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax",
        security::sign_session_for_user(&state.secret_key, user_id, username)
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
        // Consumed by external players (VLC, NVRs) that talk to mediamtx
        // directly, so this stays an absolute URL on mediamtx's own port.
        "rtsp" => format!("rtsp://{host}:8554/{path}"),
        // Browser-facing URLs are same-origin paths that nginx proxies to
        // mediamtx. mediamtx serves plain HTTP on 8889/8888, so an absolute
        // `http://` URL is blocked as mixed content the moment the UI is
        // served over HTTPS — which is how it is normally reached. A relative
        // path inherits the page's scheme instead, so it works on both without
        // needing TLS on mediamtx itself.
        //
        // The trailing slash is mediamtx's canonical form. Requesting it
        // directly avoids a redirect whose absolute `Location` would escape
        // the proxy prefix.
        "hls" => format!("/hls/{path}/index.m3u8"),
        "webrtc" => format!("/webrtc/{path}/"),
        "browser" => format!("/hls/{path}/"),
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

fn time_zone_views(mut values: Vec<String>, current: &str) -> Vec<TimeZoneView> {
    if !values.iter().any(|value| value == current) {
        values.push(current.to_string());
    }
    values.sort();
    values.dedup();
    values.into_iter().map(|value| TimeZoneView { value }).collect()
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

    #[test]
    fn rtsp_urls_dto_serializes_with_expected_field_names() {
        let urls = RtspUrls {
            main: "rtsp://octocam.local:8554/main".to_string(),
            sub: "rtsp://octocam.local:8554/sub".to_string(),
            has_sub: true,
        };
        let value = serde_json::to_value(&urls).expect("serialize RtspUrls");
        assert_eq!(
            value,
            serde_json::json!({
                "main": "rtsp://octocam.local:8554/main",
                "sub": "rtsp://octocam.local:8554/sub",
                "has_sub": true,
            })
        );
    }

    #[test]
    fn browser_stream_urls_dto_serializes_with_expected_field_names() {
        let urls = BrowserStreamUrls {
            main: "/webrtc/main/".to_string(),
            sub: "/webrtc/sub/".to_string(),
            has_sub: true,
        };
        let value = serde_json::to_value(&urls).expect("serialize BrowserStreamUrls");
        assert_eq!(
            value,
            serde_json::json!({
                "main": "/webrtc/main/",
                "sub": "/webrtc/sub/",
                "has_sub": true,
            })
        );
    }

    // Regression guard: an absolute `http://` stream URL is blocked as mixed
    // content on the HTTPS dashboard, which shows up as a black player with a
    // perfectly healthy camera behind it. Browser-facing URLs must stay
    // same-origin; only RTSP, which external players fetch directly, is absolute.
    #[test]
    fn browser_stream_urls_are_same_origin_and_rtsp_stays_absolute() {
        let settings = Settings::default();

        for protocol in ["webrtc", "hls", "browser"] {
            let url = stream_url_for(&settings, "main", protocol, "octocam.local");
            assert!(
                url.starts_with('/'),
                "{protocol} URL must be a same-origin path, got {url}"
            );
            assert!(
                !url.contains("://"),
                "{protocol} URL must not pin a scheme, got {url}"
            );
        }

        let rtsp = stream_url_for(&settings, "main", "rtsp", "octocam.local");
        assert!(
            rtsp.starts_with("rtsp://octocam.local:8554/"),
            "rtsp URL must stay absolute, got {rtsp}"
        );
    }

    #[test]
    fn stream_options_json_has_expected_keys_and_nonempty_presets() {
        let settings = Settings::default();
        let resolution_presets =
            preset_views(RESOLUTION_PRESETS, &settings.current_resolution());
        let sub_resolution_presets = preset_views(
            SUB_RESOLUTION_PRESETS,
            &settings.current_sub_resolution(),
        );
        let timezones: Vec<String> = time_zone_views(
            vec!["Etc/UTC".to_string()],
            &settings.text_overlay_timezone,
        )
        .into_iter()
        .map(|zone| zone.value)
        .collect();
        let value = serde_json::json!({
            "resolution_presets": resolution_presets,
            "sub_resolution_presets": sub_resolution_presets,
            "timezones": timezones,
            "rotations": [0, 90, 180, 270],
        });
        let obj = value.as_object().expect("stream-options body is a JSON object");
        for key in ["resolution_presets", "sub_resolution_presets", "timezones", "rotations"] {
            assert!(obj.contains_key(key), "missing key: {key}");
        }
        assert!(
            !obj["resolution_presets"]
                .as_array()
                .expect("resolution_presets is an array")
                .is_empty(),
            "resolution_presets must not be empty"
        );
        assert!(
            !obj["timezones"]
                .as_array()
                .expect("timezones is an array")
                .is_empty(),
            "timezones must not be empty"
        );
    }

    #[test]
    fn ssh_key_dto_serializes_with_expected_field_names() {
        let dto = SshKeyDto {
            key_type: "ssh-ed25519".to_string(),
            comment: "alice@laptop".to_string(),
            fingerprint: "SHA256:abc123".to_string(),
            preview: "AAAA…zzzz".to_string(),
        };
        let value = serde_json::to_value(&dto).expect("serialize SshKeyDto");
        assert_eq!(
            value,
            serde_json::json!({
                "key_type": "ssh-ed25519",
                "comment": "alice@laptop",
                "fingerprint": "SHA256:abc123",
                "preview": "AAAA…zzzz",
            })
        );
    }

    #[test]
    fn ssh_key_delete_req_defaults_confirm_to_false() {
        let req: SshKeyDeleteReq =
            serde_json::from_value(serde_json::json!({ "fingerprint": "SHA256:xyz" }))
                .expect("deserialize SshKeyDeleteReq without confirm");
        assert_eq!(req.fingerprint, "SHA256:xyz");
        assert!(!req.confirm);
    }

    #[test]
    fn login_req_deserializes_username_and_password() {
        let req: LoginReq =
            serde_json::from_value(serde_json::json!({ "username": "admin", "password": "hunter2" }))
                .expect("deserialize LoginReq");
        assert_eq!(req.username, "admin");
        assert_eq!(req.password, "hunter2");
    }

    #[test]
    fn login_req_rejects_missing_password() {
        let result: Result<LoginReq, _> =
            serde_json::from_value(serde_json::json!({ "username": "admin" }));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn api_logout_clears_session_cookie_verbatim() {
        // Must match the plain `/logout` handler's cookie string exactly so a
        // client that hits either endpoint ends up logged out the same way.
        let response = api_logout().await;
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .expect("api_logout sets Set-Cookie")
            .to_str()
            .expect("cookie header is valid UTF-8");
        assert_eq!(
            cookie,
            "octocam_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
        );
    }

    #[tokio::test]
    async fn api_logout_body_reports_success_true() {
        let response = api_logout().await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read api_logout body");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("parse JSON body");
        assert_eq!(value, serde_json::json!({ "success": true }));
    }

    #[test]
    fn homekit_view_maps_status_file_fields_for_api_homekit() {
        // api_homekit serializes these exact fields off HomeKitView; pin the
        // shape here so a HomeKitView field rename is caught at compile+test
        // time rather than silently changing the wire contract.
        let dir = std::env::temp_dir().join(format!("octocam-homekit-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("homekit-status.json");
        std::fs::write(
            &path,
            r#"{"status":"paired","paired":true,"pincode":"123-45-678","setup_uri":"X-HM://abc","qr_data_url":"data:image/png;base64,xyz","error":""}"#,
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.homekit_enabled = true;
        let view = homekit_view(&path, &settings);
        let json = serde_json::json!({
            "status": view.status,
            "paired": view.paired,
            "has_pairing": view.has_pairing,
            "pincode": view.pincode,
            "setup_uri": view.setup_uri,
            "has_qr": view.has_qr,
            "qr_data_url": view.qr_data_url,
            "error": view.error,
            "has_error": view.has_error,
        });
        assert_eq!(
            json,
            serde_json::json!({
                "status": "paired",
                "paired": true,
                "has_pairing": true,
                "pincode": "123-45-678",
                "setup_uri": "X-HM://abc",
                "has_qr": true,
                "qr_data_url": "data:image/png;base64,xyz",
                "error": "",
                "has_error": false,
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matter_view_disabled_without_identity_maps_expected_fields_for_api_matter() {
        // api_matter never loads an identity when matter_enabled is false; the
        // resulting MatterView should report a clean disabled state with no
        // manual code or QR payload.
        let settings = Settings::default();
        assert!(!settings.matter_enabled);
        let status = matter::read_status(std::path::Path::new("/nonexistent/matter-status.json"));
        let view = matter::view(&settings, None, &status);
        assert_eq!(view.status, "disabled");
        assert_eq!(view.manual_code, "");
        assert_eq!(view.qr_svg, "");
        assert_eq!(view.qr_payload, "");
        assert_eq!(view.commissioned, false);
        assert_eq!(view.fabric_count, 0);
        assert!(!view.orphaned_fabrics);
        assert!(!view.has_error);
    }

    #[test]
    fn schedule_power_action_rejects_unknown_action() {
        // api_power's `matches!` guard (400) short-circuits before this runs, but
        // schedule_power_action is the last line of defense and must still refuse
        // anything outside the known set.
        let err = schedule_power_action("erase_disk").expect_err("unknown action must error");
        assert_eq!(err.0, "Unknown power action.");
    }

    #[test]
    fn power_req_deserializes_from_json_body() {
        let req: PowerReq = serde_json::from_value(serde_json::json!({ "action": "restart_device" }))
            .expect("deserialize PowerReq");
        assert_eq!(req.action, "restart_device");
    }

    #[test]
    fn time_sync_req_time_server_is_optional() {
        let req: TimeSyncReq = serde_json::from_value(serde_json::json!({})).expect("deserialize TimeSyncReq");
        assert_eq!(req.time_server, None);

        let req: TimeSyncReq = serde_json::from_value(serde_json::json!({ "time_server": "pool.ntp.org" }))
            .expect("deserialize TimeSyncReq");
        assert_eq!(req.time_server, Some("pool.ntp.org".to_string()));
    }

    // The following tests exercise the exact dynamic-map merge pipeline used
    // by `api_settings_update`'s admin branch (settings_to_map -> overlay
    // JSON values -> validate_map -> enforce_matter_requires_admin ->
    // enforce_hksv_requires_motion -> merge_settings), without needing a
    // running AppState/db. They pin down two things that a subtle mistake in
    // that handler could silently break: (1) native JSON Value types
    // (bool/number) overlaid onto the seeded map are honored by
    // `validate_map` without any string coercion, and (2) the security
    // invariants still win over a client-supplied value when the pipeline
    // runs in the correct order.

    #[test]
    fn json_overlay_matter_enabled_is_still_forced_off_without_admin_password() {
        let current = Settings {
            admin_password_hash: String::new(),
            ..Settings::default()
        };
        let mut next_map = settings_to_map(&current).expect("serialize current settings");
        // A JSON client sends a real boolean, not the string "true" the HTML
        // form's checkbox hack would produce.
        next_map.insert("matter_enabled".to_string(), Value::Bool(true));

        let mut validated = settings::validate_map(&next_map);
        assert!(
            validated.matter_enabled,
            "validate_map must honor the native JSON bool before enforcement runs"
        );

        settings::enforce_matter_requires_admin(&mut validated);
        settings::enforce_hksv_requires_motion(&mut validated);

        assert!(
            !validated.matter_enabled,
            "enforce_matter_requires_admin must still win when admin_password_hash is empty"
        );
    }

    #[test]
    fn json_overlay_hksv_enabled_is_still_forced_off_without_motion() {
        let current = Settings {
            motion_enabled: false,
            ..Settings::default()
        };
        let mut next_map = settings_to_map(&current).expect("serialize current settings");
        next_map.insert("hksv_enabled".to_string(), Value::Bool(true));

        let mut validated = settings::validate_map(&next_map);
        assert!(validated.hksv_enabled);

        settings::enforce_matter_requires_admin(&mut validated);
        settings::enforce_hksv_requires_motion(&mut validated);

        assert!(
            !validated.hksv_enabled,
            "enforce_hksv_requires_motion must still win when motion_enabled is false"
        );
    }

    #[test]
    fn json_overlay_preserves_untouched_fields_from_current_settings() {
        // The seed-from-current-then-overlay design means fields the JSON
        // client didn't mention must retain their current value rather than
        // resetting to Settings::default() — this is what makes PUT
        // /api/settings a genuine partial update.
        let current = Settings {
            device_name: "back-porch-cam".to_string(),
            framerate: 24,
            ..Settings::default()
        };
        let mut next_map = settings_to_map(&current).expect("serialize current settings");
        next_map.insert("framerate".to_string(), Value::Number(30.into()));

        let validated = settings::validate_map(&next_map);
        assert_eq!(validated.framerate, 30, "overlaid field must be applied");
        assert_eq!(
            validated.device_name, "back-porch-cam",
            "untouched field must be preserved from the seeded current settings"
        );
    }

    #[test]
    fn wifi_connect_req_defaults_password_and_security() {
        let req: WifiConnectReq = serde_json::from_value(serde_json::json!({ "ssid": "HomeNet" }))
            .expect("deserialize WifiConnectReq without password/security");
        assert_eq!(req.ssid, "HomeNet");
        assert_eq!(req.password, "");
        assert_eq!(req.security, None);
    }

    #[test]
    fn wifi_connect_req_captures_password_and_security_when_present() {
        let req: WifiConnectReq = serde_json::from_value(serde_json::json!({
            "ssid": "HomeNet",
            "password": "s3cret",
            "security": "wpa2",
        }))
        .expect("deserialize full WifiConnectReq");
        assert_eq!(req.password, "s3cret");
        assert_eq!(req.security, Some("wpa2".to_string()));
    }

    #[test]
    fn wifi_delete_req_defaults_source_to_none() {
        let req: WifiDeleteReq = serde_json::from_value(serde_json::json!({ "name": "HomeNet" }))
            .expect("deserialize WifiDeleteReq without source");
        assert_eq!(req.name, "HomeNet");
        assert_eq!(req.source, None);
    }
}
