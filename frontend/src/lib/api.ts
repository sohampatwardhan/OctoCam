// Minimal typed fetch wrapper. Credentialed so the octocam_session cookie rides along.
export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin" })
  if (!res.ok) throw new Error(`${path} -> ${res.status}`)
  return res.json() as Promise<T>
}

// Shared by apiPost/apiPut/apiDelete: throws the server's `{error}` message
// when the response body is JSON shaped that way, otherwise falls back to a
// generic "<path> -> <status>" message.
async function throwOnError(path: string, res: Response): Promise<void> {
  if (res.ok) return
  let message = `${path} -> ${res.status}`
  try {
    const data: unknown = await res.json()
    if (data && typeof data === "object" && "error" in data && typeof data.error === "string") {
      message = data.error
    }
  } catch {
    // response wasn't JSON — keep the generic message
  }
  throw new Error(message)
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
export interface Settings {
  rtsp_enabled: boolean
  rtsp_path: string
  rtsp_max_clients: number
  homekit_enabled: boolean
  matter_enabled: boolean
}

// Subset of system.rs's WifiStatus the Wi-Fi page needs — field names must
// match exactly (see rust/octocam-web/src/system.rs).
export interface WifiStatusSummary {
  ssid: string | null
  state: string
  signal_dbm: string | null
  ip_addresses: string[]
  band: string | null
  wifi_generation_label: string | null
}

// Subset of the flattened SystemStatus (/api/status) the shell needs today —
// hostname, uptime, camera, motion_detected, services, viewers, wifi, and the
// browser-facing stream URLs. See system.rs/streams.rs for the full shape.
export interface Status {
  hostname: string
  uptime: string | null
  camera: CameraStatus
  motion_detected: boolean
  services: {
    rtsp: ServiceState
    octocam_web: ServiceState
  }
  viewers: ViewerReport | null
  browser_stream_urls: BrowserStreamUrls
  wifi: WifiStatusSummary
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
