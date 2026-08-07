// Minimal typed fetch wrapper. Credentialed so the octocam_session cookie rides along.
export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin" })
  if (!res.ok) throw new Error(`${path} -> ${res.status}`)
  return res.json() as Promise<T>
}

// POST JSON, credentialed. Throws the server's `{error}` message when present,
// otherwise falls back to a generic "<path> -> <status>" message.
export async function apiPost<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  })
  if (!res.ok) {
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
  return res.json() as Promise<T>
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

// Subset of the flattened SystemStatus (/api/status) the shell needs today —
// hostname, uptime, camera, motion_detected, services, viewers, and the
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
}
