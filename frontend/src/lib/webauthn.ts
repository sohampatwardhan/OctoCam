// base64url <-> ArrayBuffer helpers for WebAuthn payloads. Ported from the
// Askama login page's inline script (rust/octocam-web/templates/login.html)
// so the passkey request/response shapes stay byte-for-byte compatible.

import { apiPost } from "@/lib/api"

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

// Response shape of `POST /api/passkey/register/start` — see
// api_passkey_register_start in rust/octocam-web/src/main.rs (~line 2832).
// `publicKey.challenge` and `publicKey.user.id` arrive base64url-encoded and
// must be decoded to ArrayBuffers before being handed to
// `navigator.credentials.create`.
interface PasskeyRegisterStart {
  challenge_id: string
  publicKey: {
    rp: { name: string; id: string }
    user: { id: string; name: string; displayName: string }
    challenge: string
    pubKeyCredParams: { type: "public-key"; alg: number }[]
    authenticatorSelection: {
      residentKey: string
      requireResidentKey: boolean
      userVerification: string
    }
    timeout: number
  }
}

// `POST /api/passkey/register/finish` replies with HTTP 200 either way —
// failure is signaled by `success:false` + `error`, not a non-2xx status
// (see api_passkey_register_finish, main.rs ~line 2881). `apiPost` won't
// throw on that shape, so callers must check `.success` themselves; this
// helper does that and throws so it composes with a TanStack Query mutation.
interface PasskeyRegisterFinish {
  success: boolean
  error?: string
}

// Ported from rust/octocam-web/templates/admin.html's inline script
// (register flow, ~lines 286-340) — same /api/passkey/register/{start,finish}
// request/response shapes and the same base64url encode/decode of challenge,
// user.id, rawId, and response.*. Throws on any failure, including a
// `{success:false}` 200 and a `navigator.credentials.create` rejection —
// callers that want to treat user cancellation
// (DOMException NotAllowedError/AbortError) as a silent no-op should catch
// those by `.name` around this call.
export async function registerPasskey(name: string): Promise<void> {
  const start = await apiPost<PasskeyRegisterStart>("/api/passkey/register/start", { name })

  const publicKey: PublicKeyCredentialCreationOptions = {
    rp: start.publicKey.rp,
    user: {
      id: base64urlToBuffer(start.publicKey.user.id),
      name: start.publicKey.user.name,
      displayName: start.publicKey.user.displayName,
    },
    challenge: base64urlToBuffer(start.publicKey.challenge),
    pubKeyCredParams: start.publicKey.pubKeyCredParams,
    authenticatorSelection: start.publicKey.authenticatorSelection as AuthenticatorSelectionCriteria,
    timeout: start.publicKey.timeout,
  }

  const credential = (await navigator.credentials.create({ publicKey })) as PublicKeyCredential | null
  if (!credential) throw new Error("Passkey registration was cancelled.")

  const response = credential.response as AuthenticatorAttestationResponse
  const finish = await apiPost<PasskeyRegisterFinish>("/api/passkey/register/finish", {
    challenge_id: start.challenge_id,
    id: credential.id,
    rawId: bufferToBase64url(credential.rawId),
    response: {
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      attestationObject: bufferToBase64url(response.attestationObject),
    },
    name,
  })

  if (!finish.success) throw new Error(finish.error || "Passkey registration failed.")
}
