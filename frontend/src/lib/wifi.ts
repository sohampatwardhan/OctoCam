// Wi-Fi page helpers — ported from static/app.js's wifi* helpers (~lines
// 480-546): signal-dBm-to-percent conversion, a coarse strength bucket for
// icon choice, and per-security-type password validation.

// Same formula the backend uses for its own dBm→percent scan display
// (see signal_dbm_to_percent in rust/octocam-web/src/wifi.rs): -100dBm is 0%,
// -50dBm (or better) is 100%.
export function signalPercent(dbm: string | null): number {
  if (dbm === null) return 0
  const parsed = parseFloat(dbm)
  if (Number.isNaN(parsed)) return 0
  return Math.min(100, Math.max(0, ((parsed + 100) / 50) * 100))
}

export type SignalLevel = "high" | "low" | "zero"

export function signalLevel(percent: number): SignalLevel {
  if (percent >= 67) return "high"
  if (percent >= 34) return "low"
  return "zero"
}

// Mirrors wifiPasswordMeetsCriteria in static/app.js: open networks need no
// password; WEP accepts either an ASCII passphrase (5 or 13 chars) or a hex
// key (10 or 26 hex digits); everything else (WPA/WPA2/WPA3) needs an
// 8-63 char passphrase.
export function passwordMeetsCriteria(security: string, password: string): boolean {
  if (security === "open") return true
  if (security === "wep") {
    return (
      password.length === 5 ||
      password.length === 13 ||
      /^[0-9a-fA-F]{10}$/.test(password) ||
      /^[0-9a-fA-F]{26}$/.test(password)
    )
  }
  return password.length >= 8 && password.length <= 63
}
