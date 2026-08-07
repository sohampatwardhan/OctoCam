import { useState, type FormEvent, type ReactNode } from "react"
import { Navigate, useNavigate } from "react-router-dom"
import { useMutation, useQuery } from "@tanstack/react-query"
import { Camera, Loader2 } from "lucide-react"
import { apiGet, apiPost, type SetupResult, type SetupStatus } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"

// Mirrors settings::RESOLUTION_PRESETS in rust/octocam-web/src/settings.rs —
// there's no endpoint exposing these, so the values/labels are duplicated
// here. validate_map's apply_resolution_preset splits "WxH" server-side.
const RESOLUTION_PRESETS = [
  { value: "640x480", label: "640 x 480 (4:3)" },
  { value: "800x600", label: "800 x 600 (4:3)" },
  { value: "1024x768", label: "1024 x 768 (4:3)" },
  { value: "1296x972", label: "1296 x 972 (4:3)" },
  { value: "1536x864", label: "1536 x 864 (16:9)" },
  { value: "1280x720", label: "1280 x 720 (16:9 cropped)" },
  { value: "1920x1080", label: "1920 x 1080 (16:9 cropped)" },
]

// Fixed sub-stream fields the Askama setup form sends as hidden inputs (see
// setup.html) — this wizard doesn't expose sub-stream configuration, so the
// same defaults are sent verbatim.
const SUB_STREAM_FIELDS = {
  sub_stream_enabled: "on",
  sub_resolution: "640x480",
  sub_framerate: 10,
  sub_bitrate_kbps: 600,
  sub_rtsp_path: "sub",
  sub_rtsp_max_clients: 2,
}

const selectClass =
  "h-8 w-full rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 dark:bg-input/30"

function SectionHeading({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-3">
      <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{children}</h2>
      <Separator className="flex-1" />
    </div>
  )
}

export default function Setup() {
  const navigate = useNavigate()

  // Public — no session required. If setup is already complete, bounce home
  // rather than let an already-configured device re-run the wizard.
  const { data: setup, isLoading } = useQuery({
    queryKey: ["setup"],
    queryFn: () => apiGet<SetupStatus>("/api/setup"),
    retry: false,
  })

  const [deviceName, setDeviceName] = useState("OctoCam")
  const [room, setRoom] = useState("Living Room")
  const [cameraLabel, setCameraLabel] = useState("OctoCam")
  const [wifiSsid, setWifiSsid] = useState("")
  const [wifiPassword, setWifiPassword] = useState("")
  const [adminUsername, setAdminUsername] = useState("admin")
  const [adminPassword, setAdminPassword] = useState("")
  const [adminPasswordConfirm, setAdminPasswordConfirm] = useState("")
  const [resolution, setResolution] = useState("1280x720")
  const [framerate, setFramerate] = useState("15")
  const [rtspPath, setRtspPath] = useState("main")
  const [homekitEnabled, setHomekitEnabled] = useState(false)

  const passwordsPresent = adminPassword.length > 0 && adminPasswordConfirm.length > 0
  const passwordsMatch = adminPassword === adminPasswordConfirm

  const setupMutation = useMutation({
    mutationFn: () => {
      // CRITICAL: api_setup_post treats presence of the `homekit_enabled`
      // key as "enabled" regardless of its value — so the key must be
      // omitted entirely when the checkbox is unchecked, never sent as
      // false.
      const body: Record<string, unknown> = {
        device_name: deviceName,
        room,
        camera_label: cameraLabel,
        wifi_ssid: wifiSsid,
        wifi_password: wifiPassword,
        admin_username: adminUsername,
        admin_password: adminPassword,
        admin_password_confirm: adminPasswordConfirm,
        resolution,
        framerate,
        rtsp_path: rtspPath,
        ...SUB_STREAM_FIELDS,
      }
      if (homekitEnabled) body.homekit_enabled = true
      return apiPost<SetupResult>("/api/setup", body)
    },
    onSuccess: async (result) => {
      if (!result.success) return
      // The session cookie is already set by this response — refetch (not
      // invalidate) so AuthGate sees an authenticated cache immediately,
      // same reasoning as Login's post-login refetch.
      await queryClient.refetchQueries({ queryKey: ["me"] })
      navigate("/", { replace: true })
    },
  })

  const result = setupMutation.data
  const softError = result && !result.success ? result : null
  const fieldError = (field: string) => (softError?.field === field ? softError.message : undefined)
  const generalError =
    softError && softError.field !== "admin_password_confirm" && softError.field !== "wifi"
      ? softError.message ?? "Setup failed."
      : null

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    if (!passwordsPresent || !passwordsMatch) return
    setupMutation.mutate()
  }

  if (isLoading) return null
  if (setup && !setup.setup_required) return <Navigate to="/" replace />

  const canSubmit = passwordsPresent && passwordsMatch && !setupMutation.isPending

  return (
    <main className="relative flex min-h-screen items-center justify-center overflow-hidden bg-background p-6">
      <ApertureBackdrop />

      <Card className="relative z-10 w-full max-w-lg">
        <CardHeader>
          <p className="flex items-center gap-1.5 text-xs font-medium tracking-wide text-muted-foreground uppercase">
            <Camera className="size-3.5" />
            OctoCam setup
          </p>
          <CardTitle className="text-2xl">Make this camera yours</CardTitle>
          <CardDescription>A few details, then you're live.</CardDescription>
        </CardHeader>

        <CardContent>
          <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
            <div className="flex flex-col gap-3">
              <SectionHeading>Identity</SectionHeading>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="device_name">Device name</Label>
                <Input
                  id="device_name"
                  value={deviceName}
                  maxLength={80}
                  onChange={(e) => setDeviceName(e.target.value)}
                />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="room">Room</Label>
                  <Input id="room" value={room} maxLength={80} onChange={(e) => setRoom(e.target.value)} />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="camera_label">Camera label</Label>
                  <Input
                    id="camera_label"
                    value={cameraLabel}
                    maxLength={80}
                    onChange={(e) => setCameraLabel(e.target.value)}
                  />
                </div>
              </div>
            </div>

            <div className="flex flex-col gap-3">
              <SectionHeading>Network</SectionHeading>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="wifi_ssid">Wi-Fi SSID</Label>
                <Input
                  id="wifi_ssid"
                  value={wifiSsid}
                  maxLength={80}
                  autoComplete="off"
                  placeholder="Leave blank to keep the current connection"
                  onChange={(e) => setWifiSsid(e.target.value)}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="wifi_password">Wi-Fi password</Label>
                <Input
                  id="wifi_password"
                  type="password"
                  autoComplete="current-password"
                  value={wifiPassword}
                  onChange={(e) => setWifiPassword(e.target.value)}
                />
              </div>
              {fieldError("wifi") && <p className="text-sm text-destructive">{fieldError("wifi")}</p>}
            </div>

            <div className="flex flex-col gap-3">
              <SectionHeading>Admin</SectionHeading>
              <div className="grid grid-cols-2 gap-3">
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="admin_username">Admin username</Label>
                  <Input
                    id="admin_username"
                    value={adminUsername}
                    required
                    onChange={(e) => setAdminUsername(e.target.value)}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="admin_password">Admin password</Label>
                  <Input
                    id="admin_password"
                    type="password"
                    autoComplete="new-password"
                    required
                    value={adminPassword}
                    onChange={(e) => setAdminPassword(e.target.value)}
                  />
                </div>
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="admin_password_confirm">Confirm password</Label>
                <Input
                  id="admin_password_confirm"
                  type="password"
                  autoComplete="new-password"
                  required
                  aria-invalid={passwordsPresent && !passwordsMatch}
                  value={adminPasswordConfirm}
                  onChange={(e) => setAdminPasswordConfirm(e.target.value)}
                />
                {passwordsPresent && !passwordsMatch && (
                  <p className="text-sm text-destructive">Passwords do not match.</p>
                )}
                {fieldError("admin_password_confirm") && (
                  <p className="text-sm text-destructive">{fieldError("admin_password_confirm")}</p>
                )}
              </div>
            </div>

            <div className="flex flex-col gap-3">
              <SectionHeading>Stream</SectionHeading>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="resolution">Resolution</Label>
                <select
                  id="resolution"
                  className={selectClass}
                  value={resolution}
                  onChange={(e) => setResolution(e.target.value)}
                >
                  {RESOLUTION_PRESETS.map((preset) => (
                    <option key={preset.value} value={preset.value}>
                      {preset.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="framerate">FPS</Label>
                  <Input
                    id="framerate"
                    type="number"
                    min={1}
                    max={60}
                    value={framerate}
                    onChange={(e) => setFramerate(e.target.value)}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="rtsp_path">RTSP path</Label>
                  <Input
                    id="rtsp_path"
                    value={rtspPath}
                    maxLength={80}
                    onChange={(e) => setRtspPath(e.target.value)}
                  />
                </div>
              </div>
            </div>

            <div className="flex flex-col gap-3">
              <SectionHeading>HomeKit</SectionHeading>
              <label htmlFor="homekit_enabled" className="flex items-center justify-between gap-3">
                <span className="text-sm">Enable HomeKit bridge</span>
                <input
                  id="homekit_enabled"
                  type="checkbox"
                  className="size-4 rounded border border-input accent-primary"
                  checked={homekitEnabled}
                  onChange={(e) => setHomekitEnabled(e.target.checked)}
                />
              </label>
            </div>

            {generalError && <p className="text-sm text-destructive">{generalError}</p>}
            {setupMutation.isError && (
              <p className="text-sm text-destructive">{setupMutation.error.message}</p>
            )}

            <Button type="submit" className="w-full" disabled={!canSubmit}>
              {setupMutation.isPending && <Loader2 className="size-4 animate-spin" />}
              Finish setup
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  )
}

// Signature element carried over from Login — same concentric aperture
// motif, so the pre-auth pages read as one continuous flow.
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
