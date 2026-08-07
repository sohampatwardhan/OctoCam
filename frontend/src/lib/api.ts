// Minimal typed fetch wrapper. Credentialed so the octocam_session cookie rides along.
export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin" })
  if (!res.ok) throw new Error(`${path} -> ${res.status}`)
  return res.json() as Promise<T>
}

export interface CameraStatus {
  available: boolean
  message: string
}

// Subset of the flattened SystemStatus we render in the pilot. /api/status
// returns many more fields (hostname, ip_addresses, uptime, cpu_temp_c,
// resources, wifi, camera, services, logs) plus viewers + motion_detected.
export interface Status {
  hostname: string
  uptime: string | null
  camera: CameraStatus
  motion_detected: boolean
}
