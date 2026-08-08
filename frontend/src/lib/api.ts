// Minimal typed fetch wrapper. Credentialed so the octocam_session cookie rides along.
export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin" })
  if (!res.ok) throw new Error(`${path} -> ${res.status}`)
  return res.json() as Promise<T>
}

// Thrown by throwOnError below. Behaves like a plain `Error` (same
// `.message`, same `instanceof Error`) so existing callers that only read
// `.message` keep working unchanged, but also carries the HTTP status and,
// when the server sent one (see api::ApiError::with_code in
// rust/octocam-web/src/api.rs), a machine-readable `code` — e.g.
// `api_ssh_keys_delete`'s last-key 409 sends `{error, code:"last_key"}`.
// Callers that need to branch on the failure kind should check `code`/
// `status` rather than pattern-matching `.message` text.
export class ApiRequestError extends Error {
  status: number
  code?: string

  constructor(message: string, status: number, code?: string) {
    super(message)
    this.name = "ApiRequestError"
    this.status = status
    this.code = code
  }
}

// Shared by apiPost/apiPut/apiDelete: throws the server's `{error}` message
// (plus `code` when present) as an ApiRequestError when the response body is
// JSON shaped that way, otherwise falls back to a generic "<path> -> <status>"
// message with no code.
async function throwOnError(path: string, res: Response): Promise<void> {
  if (res.ok) return
  let message = `${path} -> ${res.status}`
  let code: string | undefined
  try {
    const data: unknown = await res.json()
    if (data && typeof data === "object" && "error" in data && typeof data.error === "string") {
      message = data.error
      if ("code" in data && typeof data.code === "string") {
        code = data.code
      }
    }
  } catch {
    // response wasn't JSON — keep the generic message
  }
  throw new ApiRequestError(message, res.status, code)
}

async function apiSendJson<T>(method: "POST" | "PUT" | "DELETE", path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  })
  await throwOnError(path, res)
  return res.json() as Promise<T>
}

// POST JSON, credentialed. Throws the server's `{error}` message when present,
// otherwise falls back to a generic "<path> -> <status>" message.
export function apiPost<T>(path: string, body: unknown): Promise<T> {
  return apiSendJson<T>("POST", path, body)
}

// POST a multipart FormData body, credentialed. Used by /api/restore. Do NOT
// set a Content-Type header — the browser must generate the multipart
// boundary itself; setting one manually (even to the right-looking value)
// breaks the boundary and the server sees an empty/invalid part. Error
// surfacing matches apiSendJson: server errors come back as `{error, code?}`
// (see api::ApiError in rust/octocam-web/src/api.rs) and throwOnError
// extracts `error` into the thrown message.
export async function apiUpload<T>(path: string, formData: FormData): Promise<T> {
  const res = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    body: formData,
  })
  await throwOnError(path, res)
  return res.json() as Promise<T>
}

// PUT JSON, credentialed. Same error-surfacing behavior as apiPost. Used by
// `/api/settings`, which seeds a full settings map from the server's current
// values then overlays the request body — callers should send only the
// fields that changed, with native typed values (booleans/numbers, not
// stringified), since validate_map accepts native JSON types.
export function apiPut<T>(path: string, body: unknown): Promise<T> {
  return apiSendJson<T>("PUT", path, body)
}

// DELETE JSON, credentialed. Same error-surfacing behavior as apiPost — the
// wifi/delete endpoint reads its request (name + source) from a JSON body
// rather than the URL, so this isn't a bodyless DELETE.
export function apiDelete<T>(path: string, body: unknown): Promise<T> {
  return apiSendJson<T>("DELETE", path, body)
}

// `/api/me` — session/identity probe. 401s (with this same shape) when logged out.
export interface Me {
  authenticated: boolean
  username?: string
  role?: string
  is_admin?: boolean
  setup_required: boolean
}

export interface CameraStatus {
  available: boolean
  message: string
}

export interface ServiceState {
  state: string
}

export interface ClientView {
  label: string
  client_type: string
  remote_addr: string
  user_agent: string
  connected_at: string
}

export interface PathViewers {
  browser: number
  rtsp: number
  total: number
  capacity: number
  clients: ClientView[]
}

export interface ViewerReport {
  main: PathViewers
  sub: PathViewers
}

export interface BrowserStreamUrls {
  main: string
  sub: string
  has_sub: boolean
}

// `/api/rtsp` — the RTSP page's stream URLs (rtsp:// URIs, not the browser
// playback URLs above). See rtsp_urls in main.rs.
export interface RtspUrls {
  main: string
  sub: string
  has_sub: boolean
}

// Subset of settings::Settings (rust/octocam-web/src/settings.rs) that the
// SPA reads/writes today. `public_settings()` is the full struct minus
// `admin_password_hash`; field names/types must match exactly. `GET/PUT
// /api/settings` — see api_settings_get/api_settings_put in main.rs. PUT
// seeds a full map from the server's CURRENT settings then overlays the
// request body, so callers should send only the changed fields.
//
// The `scheduled_*_days` fields are what GET actually returns — a CSV of
// systemd weekday abbreviations (e.g. "Mon,Tue,Wed"), per
// `scheduled_service_restart_days`/`scheduled_reboot_days` in Settings. The
// `scheduled_*_day_<mon..sun>` fields below are NOT struct fields and are
// never present in a GET response — they're form-only keys that
// `weekdays_value()` (settings.rs ~610-643) recognizes on PUT and converts
// back into the CSV, taking priority over `scheduled_*_days` when present.
// They're included here (all optional) purely so `useUpdateSettings().mutate`
// can send per-day booleans in a properly-typed patch.
export interface Settings {
  // PUT-only virtual fields (never present in a GET response, like
  // `resolution`/`sub_resolution` below) — api_settings_update (main.rs
  // ~line 2212) reads these off the request body to change the caller's own
  // password, for both admin and non-admin callers. An empty/missing pair
  // leaves the password unchanged; a non-empty pair that doesn't match comes
  // back as a 400 `{error:"Password fields are empty or do not match."}`.
  admin_password?: string
  admin_password_confirm?: string
  device_name: string
  room: string
  camera_label: string
  rtsp_enabled: boolean
  rtsp_path: string
  rtsp_max_clients: number
  homekit_enabled: boolean
  matter_enabled: boolean
  // Stream-settings page fields (see rust/octocam-web/src/settings.rs's
  // Settings struct — names/types below match it exactly unless noted).
  camera_enabled: boolean
  resolution_width: number
  resolution_height: number
  framerate: number
  bitrate_kbps: number
  sub_stream_enabled: boolean
  sub_resolution_width: number
  sub_resolution_height: number
  sub_framerate: number
  sub_bitrate_kbps: number
  rotation: number
  hflip: boolean
  vflip: boolean
  brightness: number
  contrast: number
  noir_mode: boolean
  motion_enabled: boolean
  motion_sensitivity: number
  // A u64 bitmask server-side. public_settings() (settings.rs) emits it as a
  // decimal STRING (not a JSON number) because 64-bit values routinely
  // exceed Number.MAX_SAFE_INTEGER and would silently lose precision in JS.
  // Read/write it with BigInt, never Number. u64_value() (settings.rs)
  // accepts a Value::String on the way back in via PUT.
  motion_zones: string
  hksv_enabled: boolean
  text_overlay_enabled: boolean
  text_overlay_timezone: string
  text_overlay_clock_format: string
  text_overlay_date_format: string
  time_server: string
  // `resolution`/`sub_resolution` are PUT-only virtual fields (like the
  // scheduled_*_day_* fields below) — apply_resolution_preset() (settings.rs)
  // consumes a preset value string (e.g. "1280x720") on the way in and
  // expands it to the real *_width/*_height fields. GET never returns these;
  // they exist here purely so useUpdateSettings().mutate can send one.
  resolution?: string
  sub_resolution?: string
  scheduled_service_restart_enabled: boolean
  scheduled_service_restart_time: string
  scheduled_service_restart_days: string
  scheduled_service_restart_day_mon?: boolean
  scheduled_service_restart_day_tue?: boolean
  scheduled_service_restart_day_wed?: boolean
  scheduled_service_restart_day_thu?: boolean
  scheduled_service_restart_day_fri?: boolean
  scheduled_service_restart_day_sat?: boolean
  scheduled_service_restart_day_sun?: boolean
  scheduled_reboot_enabled: boolean
  scheduled_reboot_time: string
  scheduled_reboot_days: string
  scheduled_reboot_day_mon?: boolean
  scheduled_reboot_day_tue?: boolean
  scheduled_reboot_day_wed?: boolean
  scheduled_reboot_day_thu?: boolean
  scheduled_reboot_day_fri?: boolean
  scheduled_reboot_day_sat?: boolean
  scheduled_reboot_day_sun?: boolean
}

// A resolution/preset choice — see settings::PresetView in
// rust/octocam-web/src/settings.rs. `selected` reflects the settings that
// were current on the server when `/api/stream-options` was fetched; treat
// it only as an initial default, not a live indicator of unsaved form state.
export interface PresetOption {
  value: string
  label: string
  selected: boolean
}

// `GET /api/stream-options` — select-menu options for the Stream Settings
// page. See api_stream_options in main.rs (~line 2404). `timezones` is a
// flat list of IANA zone name strings (time_zone_views() unwrapped to just
// `.value`); `rotations` is the fixed [0, 90, 180, 270] choice set.
export interface StreamOptions {
  resolution_presets: PresetOption[]
  sub_resolution_presets: PresetOption[]
  timezones: string[]
  rotations: number[]
}

// system.rs's WifiStatus, flattened into `/api/status` under `wifi` — field
// names must match exactly (see rust/octocam-web/src/system.rs). Originally a
// smaller subset for the Wi-Fi page; extended with the raw fields the System
// page's Wi-Fi details list needs (mirrors `wifi_details()` in system.rs).
export interface WifiStatusSummary {
  ssid: string | null
  state: string
  message: string
  interface: string | null
  bssid: string | null
  frequency_mhz: number | null
  channel: number | null
  band: string | null
  channel_width: string | null
  signal_dbm: string | null
  rx_bitrate: string | null
  tx_bitrate: string | null
  tx_power: string | null
  wifi_generation_label: string | null
  security: string | null
  ip_address: string | null
  ip_addresses: string[]
  mac_address: string | null
  default_gateway: string | null
  default_interface: string | null
}

// MemoryStatus/ResourceStatus from system.rs — field names must match
// exactly. Swap fields live under `memory`, not `resources` directly.
export interface MemoryStatus {
  total_mb: number
  available_mb: number
  used_mb: number
  used_percent: number | null
  swap_total_mb: number
  swap_used_mb: number
  swap_used_percent: number | null
}

export interface ResourceStatus {
  cpu_usage_percent: number | null
  load_average: string | null
  memory: MemoryStatus
  memory_summary: string | null
}

// The flattened SystemStatus (/api/status) — hostname, ip_addresses, uptime,
// cpu_temp_c, resources, camera, motion_detected, services, viewers, wifi,
// and the browser-facing stream URLs. See system.rs/streams.rs for the full
// shape (this omits `logs`, which no SPA page reads yet).
export interface Status {
  hostname: string
  ip_addresses: string[]
  uptime: string | null
  cpu_temp_c: number | null
  resources: ResourceStatus
  camera: CameraStatus
  motion_detected: boolean
  services: {
    rtsp: ServiceState
    octocam_web: ServiceState
    homekit: ServiceState
  }
  viewers: ViewerReport | null
  browser_stream_urls: BrowserStreamUrls
  wifi: WifiStatusSummary
}

// `POST /api/restore` response — see api_restore in main.rs. On success
// (HTTP 200) the body is `{success:true, keys_added, keys_failed}`. Errors
// (e.g. the 256 KiB cap's `too_large`) come back as a non-2xx status with
// `{error, code?}`, which `apiUpload` throws as an `Error` before this type
// is ever seen — `error`/`code` are included here for completeness only.
export interface RestoreResult {
  success: boolean
  keys_added?: number
  keys_failed?: number
  error?: string
  code?: string
}

// `/api/homekit` — the HomeKit page's pairing view. Field names/types must
// match api_homekit's response shape exactly (see main.rs, ~line 2329).
export interface HomeKitInfo {
  status: string
  paired: boolean
  has_pairing: boolean
  pincode: string
  setup_uri: string
  has_qr: boolean
  qr_data_url: string
  error: string
  has_error: boolean
}

// `/api/matter` — the Matter page's status/pairing view. Field names/types
// must match api_matter's response shape exactly (see main.rs, ~line 2354).
// `qr_payload` is the raw Matter onboarding payload string encoded by
// `qr_svg`, shown in small text below the manual code.
export interface MatterInfo {
  status: string
  commissioned: boolean
  fabric_count: number
  orphaned_fabrics: boolean
  manual_code: string
  qr_svg: string
  qr_payload: string
  stream_source: string
  error: string
  has_error: boolean
  ipv6_ok: boolean
  admin_password_set: boolean
  snapshot_endpoint_down: boolean
}

// `/api/wifi/networks` + `/api/wifi/scan` — see rust/octocam-web/src/wifi.rs.
export interface WifiNetwork {
  ssid: string
  security: string
  raw_security: string
  signal: number
}

export interface WifiCache {
  scanned_at: number | null
  networks: WifiNetwork[]
}

// `/api/wifi/saved` — see StoredWifiProfile in rust/octocam-web/src/system.rs.
export interface SavedWifiProfile {
  name: string
  security: string
  source: string
  active: boolean
  can_delete: boolean
  delete_source: string
}

// `GET /api/logs` — see api_logs in main.rs. A fixed 40-line journalctl
// snapshot (not a live tail); the Logs page polls this every 5s.
export interface LogsResponse {
  lines: string[]
}

// `/api/ssh-keys` (GET) — JSON view of root's authorized_keys, admin-only.
// See SshKeyDto in main.rs (~line 1386), sourced from ssh_keys::AuthorizedKey.
export interface SshKeyDto {
  key_type: string
  comment: string
  fingerprint: string
  preview: string
}

// `GET /api/passkeys` — the caller's own passkeys (scoped server-side by
// user_id — see api_passkeys_list/list_passkeys_for_user in
// rust/octocam-web/src/main.rs and db.rs's Passkey struct). The server
// response also includes `credential_id`/`public_key` (raw byte arrays),
// `user_id`, `counter`, and `transports`; only the fields the Account page
// renders are typed here.
export interface PasskeyDto {
  id: number
  name: string
  created_at: string
  last_used_at: string | null
}

// `GET /api/users` — user accounts / RBAC, admin-only. See api_users_list in
// rust/octocam-web/src/main.rs (~line 3051). `role` is whatever string is
// stored in the `users` table; `api_users_add` only ever writes "admin" or
// "viewer" (anything else it sends normalizes to "viewer"), but this stays a
// plain `string` rather than a union since the server doesn't validate it on
// read.
export interface UserDto {
  id: number
  username: string
  role: string
  created_at: string
}

// `GET /api/setup` — see api_setup_get in main.rs. Also embedded in `Me`
// above for the post-login probe; this is the standalone pre-auth shape.
export interface SetupStatus {
  setup_required: boolean
}

// `POST /api/setup` response — see api_setup_post in main.rs. Soft failures
// (password mismatch, Wi-Fi join failure) come back as HTTP 200 with
// success:false, so apiPost resolves normally rather than throwing.
export type SetupResult =
  | { success: true }
  | { success: false; field?: string; message?: string }
