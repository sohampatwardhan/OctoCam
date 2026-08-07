// base64url <-> ArrayBuffer helpers for WebAuthn payloads. Ported from the
// Askama login page's inline script (rust/octocam-web/templates/login.html)
// so the passkey request/response shapes stay byte-for-byte compatible.

export function bufferToBase64url(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer)
  let binary = ""
  for (let i = 0; i < bytes.byteLength; i++) binary += String.fromCharCode(bytes[i])
  return window.btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=/g, "")
}

export function base64urlToBuffer(base64url: string): ArrayBuffer {
  let base64 = base64url.replace(/-/g, "+").replace(/_/g, "/")
  while (base64.length % 4) base64 += "="
  const raw = window.atob(base64)
  const buf = new Uint8Array(raw.length)
  for (let i = 0; i < raw.length; i++) buf[i] = raw.charCodeAt(i)
  return buf.buffer
}

export function isWebAuthnSupported(): boolean {
  return typeof window !== "undefined" && "PublicKeyCredential" in window
}

// `PublicKeyCredential.isConditionalMediationAvailable` landed after the
// baseline DOM types most TS lib.dom snapshots ship with — feature-detect it.
export async function isConditionalMediationAvailable(): Promise<boolean> {
  if (!isWebAuthnSupported()) return false
  const ctor = window.PublicKeyCredential as typeof PublicKeyCredential & {
    isConditionalMediationAvailable?: () => Promise<boolean>
  }
  if (typeof ctor.isConditionalMediationAvailable !== "function") return false
  try {
    return await ctor.isConditionalMediationAvailable()
  } catch {
    return false
  }
}
