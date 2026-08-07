import { useEffect, useRef, useState, type FormEvent } from "react"
import { useNavigate } from "react-router-dom"
import { useMutation, useQuery } from "@tanstack/react-query"
import { Camera, Fingerprint, Loader2 } from "lucide-react"
import { apiGet, apiPost } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"
import {
  base64urlToBuffer,
  bufferToBase64url,
  isConditionalMediationAvailable,
  isWebAuthnSupported,
} from "@/lib/webauthn"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"

interface LoginResponse {
  success: boolean
  username: string
  role: string
  is_admin: boolean
}

interface PasskeyLoginStart {
  challenge_id: string
  publicKey: {
    rpId: string
    challenge: string
    timeout: number
    userVerification: string
    allowCredentials: { type: string; id: string }[]
  }
}

interface PasskeyLoginFinish {
  success: boolean
  redirect?: string
  error?: string
}

interface SetupStatus {
  setup_required: boolean
}

export default function Login() {
  const navigate = useNavigate()
  const [username, setUsername] = useState("admin")
  const [password, setPassword] = useState("")
  const [passkeyError, setPasskeyError] = useState<string | null>(null)
  const [passkeyBusy, setPasskeyBusy] = useState(false)
  const conditionalAttempted = useRef(false)

  // Public — used only to surface a "finish setup" link when no admin exists yet.
  const { data: setup } = useQuery({
    queryKey: ["setup-status"],
    queryFn: () => apiGet<SetupStatus>("/api/setup"),
    retry: false,
  })

  const loginMutation = useMutation({
    mutationFn: () => apiPost<LoginResponse>("/api/login", { username, password }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["me"] })
      navigate("/", { replace: true })
    },
  })

  // Ported from rust/octocam-web/templates/login.html's inline script — same
  // /api/passkey/login/start + /finish request/response shapes and the same
  // base64url encode/decode of challenge, allowCredentials[].id, rawId, and
  // response.*. `mediationMode: "conditional"` runs silently on mount for
  // autofill; "optional" is the explicit button click.
  async function performPasskeyLogin(mediationMode: "optional" | "conditional") {
    if (mediationMode === "optional") setPasskeyBusy(true)
    try {
      setPasskeyError(null)
      const data = await apiPost<PasskeyLoginStart>("/api/passkey/login/start", {})

      const publicKey: PublicKeyCredentialRequestOptions = {
        rpId: data.publicKey.rpId,
        challenge: base64urlToBuffer(data.publicKey.challenge),
        timeout: data.publicKey.timeout,
        userVerification: data.publicKey.userVerification as UserVerificationRequirement,
        allowCredentials: data.publicKey.allowCredentials.map((cred) => ({
          type: "public-key" as const,
          id: base64urlToBuffer(cred.id),
        })),
      }

      const options: Record<string, unknown> = { publicKey }
      if (mediationMode === "conditional") {
        const supported = await isConditionalMediationAvailable()
        if (!supported) return
        options.mediation = "conditional"
      }

      const credential = (await navigator.credentials.get(
        options as CredentialRequestOptions
      )) as PublicKeyCredential | null
      if (!credential) return

      const response = credential.response as AuthenticatorAssertionResponse
      const finishData = await apiPost<PasskeyLoginFinish>("/api/passkey/login/finish", {
        challenge_id: data.challenge_id,
        id: credential.id,
        rawId: bufferToBase64url(credential.rawId),
        response: {
          clientDataJSON: bufferToBase64url(response.clientDataJSON),
          authenticatorData: bufferToBase64url(response.authenticatorData),
          signature: bufferToBase64url(response.signature),
          userHandle: response.userHandle ? bufferToBase64url(response.userHandle) : null,
        },
      })

      if (finishData.success) {
        await queryClient.invalidateQueries({ queryKey: ["me"] })
        navigate("/", { replace: true })
      } else if (mediationMode !== "conditional") {
        setPasskeyError(finishData.error || "Passkey authentication failed.")
      }
    } catch (err) {
      const name = err instanceof DOMException ? err.name : undefined
      if (name === "NotAllowedError" || name === "AbortError") return
      if (mediationMode !== "conditional") {
        setPasskeyError(err instanceof Error ? err.message : "Passkey login error.")
      }
    } finally {
      if (mediationMode === "optional") setPasskeyBusy(false)
    }
  }

  // Conditional-mediation autofill: offer saved passkeys inline in the
  // username field the moment the page loads. No-ops silently when the
  // browser doesn't support it.
  useEffect(() => {
    if (!isWebAuthnSupported() || conditionalAttempted.current) return
    conditionalAttempted.current = true
    void performPasskeyLogin("conditional")
  }, [])

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    loginMutation.mutate()
  }

  return (
    <main className="relative flex min-h-screen items-center justify-center overflow-hidden bg-background p-6">
      <ApertureBackdrop />

      <Card className="relative z-10 w-full max-w-sm">
        <CardHeader>
          <p className="flex items-center gap-1.5 text-xs font-medium tracking-wide text-muted-foreground uppercase">
            <Camera className="size-3.5" />
            OctoCam
          </p>
          <CardTitle className="text-2xl">Sign in</CardTitle>
          <CardDescription>Enter your credentials to reach the dashboard.</CardDescription>
        </CardHeader>

        <CardContent className="flex flex-col gap-4">
          <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                name="username"
                type="text"
                autoComplete="username webauthn"
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                name="password"
                type="password"
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>

            {loginMutation.error && (
              <p className="text-sm text-destructive">{loginMutation.error.message}</p>
            )}

            <Button type="submit" className="w-full" disabled={loginMutation.isPending}>
              {loginMutation.isPending && <Loader2 className="size-4 animate-spin" />}
              Sign in
            </Button>
          </form>

          {isWebAuthnSupported() && (
            <>
              <div className="flex items-center gap-3">
                <Separator className="flex-1" />
                <span className="text-xs text-muted-foreground">or</span>
                <Separator className="flex-1" />
              </div>

              <Button
                type="button"
                variant="outline"
                className="w-full"
                disabled={passkeyBusy}
                onClick={() => performPasskeyLogin("optional")}
              >
                {passkeyBusy ? <Loader2 className="size-4 animate-spin" /> : <Fingerprint className="size-4" />}
                Sign in with a passkey
              </Button>

              {passkeyError && <p className="text-sm text-destructive">{passkeyError}</p>}
            </>
          )}

          {setup?.setup_required && (
            <p className="text-center text-xs text-muted-foreground">
              First time here?{" "}
              <a href="/setup" className="underline underline-offset-2 hover:text-foreground">
                Finish setup
              </a>
            </p>
          )}
        </CardContent>
      </Card>
    </main>
  )
}

// Signature element: concentric aperture rings, evoking a camera iris,
// sitting quietly behind the login card. Decorative only.
function ApertureBackdrop() {
  const radii = [420, 340, 260, 180]
  return (
    <svg
      aria-hidden="true"
      className="pointer-events-none absolute top-1/2 left-1/2 size-[900px] -translate-x-1/2 -translate-y-1/2 text-border"
      viewBox="0 0 900 900"
      fill="none"
    >
      {radii.map((r) => (
        <circle key={r} cx="450" cy="450" r={r} stroke="currentColor" strokeWidth="1" strokeDasharray="2 10" />
      ))}
      <circle cx="450" cy="450" r="120" className="text-primary/15" fill="currentColor" />
    </svg>
  )
}
