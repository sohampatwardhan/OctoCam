import { useEffect, useState, type FormEvent } from "react"
import { useQuery } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { apiGet, type MqttStatus } from "@/lib/api"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { Skeleton } from "@/components/ui/skeleton"
import { isFormDirty, useReportUnsavedChanges } from "@/hooks/useUnsavedChanges"
import { cn } from "@/lib/utils"

// Local, editable form state. The password is handled separately from the rest
// (see PASSWORD_UNCHANGED) because the server never sends it back — the form
// only knows whether one is set, not its value.
interface MqttFormState {
  enabled: boolean
  host: string
  port: string
  username: string
  tls: boolean
  clientId: string
  baseTopic: string
  discoveryPrefix: string
}

// Sentinel for the password field's initial value: a placeholder the user has
// not touched. Submitting while the field still holds this leaves the stored
// password alone; clearing the field to empty explicitly clears it.
const PASSWORD_UNCHANGED = "••••••••"

function MqttStatusBadge({ status }: { status?: MqttStatus }) {
  const state = status?.state ?? "disabled"
  const label = state === "connected" ? "Connected" : state === "connecting" ? "Disconnected" : "Disabled"
  const dot =
    state === "connected"
      ? "bg-success"
      : state === "connecting"
        ? "bg-destructive"
        : "bg-muted-foreground/60"
  return (
    <span className="inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
      <span className={cn("size-1.5 rounded-full", dot)} aria-hidden="true" />
      {label}
    </span>
  )
}

export default function MqttSettings() {
  const { data: settings, isLoading, isError } = useSettings()
  const updateSettings = useUpdateSettings()

  // Publisher state is independent of the settings read model, and changes
  // without a form save (the broker connects/drops on its own), so it polls.
  const { data: status } = useQuery({
    queryKey: ["mqtt-status"],
    queryFn: () => apiGet<MqttStatus>("/api/mqtt/status"),
    refetchInterval: 5000,
  })

  const [initialized, setInitialized] = useState(false)
  const [form, setForm] = useState<MqttFormState | null>(null)
  const [savedForm, setSavedForm] = useState<MqttFormState | null>(null)
  // Kept out of MqttFormState: its baseline is a sentinel, not a real value,
  // so it participates in dirty-tracking on its own terms.
  const [password, setPassword] = useState(PASSWORD_UNCHANGED)
  const passwordDirty = password !== PASSWORD_UNCHANGED

  useReportUnsavedChanges(
    { id: "mqtt", label: "MQTT", anchorId: "mqtt-settings-form" },
    isFormDirty(form, savedForm) || passwordDirty
  )

  useEffect(() => {
    if (settings && !initialized) {
      const loaded: MqttFormState = {
        enabled: settings.mqtt_enabled,
        host: settings.mqtt_host,
        port: String(settings.mqtt_port),
        username: settings.mqtt_username,
        tls: settings.mqtt_tls,
        clientId: settings.mqtt_client_id,
        baseTopic: settings.mqtt_base_topic,
        discoveryPrefix: settings.mqtt_discovery_prefix,
      }
      setForm(loaded)
      setSavedForm(loaded)
      setInitialized(true)
    }
  }, [settings, initialized])

  function update(patch: Partial<MqttFormState>) {
    setForm((current) => (current ? { ...current, ...patch } : current))
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    if (!form) return
    const submitted = form
    const submittedPassword = password
    updateSettings.mutate(
      {
        mqtt_enabled: form.enabled,
        mqtt_host: form.host,
        mqtt_port: Number(form.port) || 1883,
        mqtt_username: form.username,
        mqtt_tls: form.tls,
        mqtt_client_id: form.clientId,
        mqtt_base_topic: form.baseTopic,
        mqtt_discovery_prefix: form.discoveryPrefix,
        // Only send the password when the admin actually changed it, so an
        // untouched form preserves the stored value rather than blanking it.
        ...(passwordDirty ? { mqtt_password: password } : {}),
      },
      {
        onSuccess: () => {
          setSavedForm(submitted)
          // Re-baseline the password: what was just sent is now the stored
          // value, so the field returns to its untouched sentinel.
          if (submittedPassword !== PASSWORD_UNCHANGED) setPassword(PASSWORD_UNCHANGED)
        },
      }
    )
  }

  const passwordAlreadySet = settings?.mqtt_password_set ?? false

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <div className="flex items-center justify-between gap-3">
        <h1 className="font-heading text-2xl font-semibold tracking-tight">MQTT</h1>
        <MqttStatusBadge status={status} />
      </div>

      {isError ? (
        <p className="text-sm text-muted-foreground">Device unreachable.</p>
      ) : isLoading || !form ? (
        <Card>
          <CardHeader>
            <CardTitle>Broker</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
          </CardContent>
        </Card>
      ) : (
        <form id="mqtt-settings-form" className="flex flex-col gap-6" onSubmit={handleSubmit}>
          <Card>
            <CardHeader>
              <CardTitle>Broker</CardTitle>
              <CardDescription>
                Publish this camera's motion sensor to an MQTT broker so Home Assistant discovers it
                automatically.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <label htmlFor="mqtt_enabled" className="flex items-center justify-between gap-3">
                <span className="text-sm font-medium">Publish to MQTT</span>
                <Switch
                  id="mqtt_enabled"
                  checked={form.enabled}
                  onCheckedChange={(checked) => update({ enabled: checked })}
                />
              </label>

              <div className="grid grid-cols-1 gap-4 sm:grid-cols-[minmax(0,1fr)_8rem]">
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="mqtt_host">Broker host</Label>
                  <Input
                    id="mqtt_host"
                    value={form.host}
                    placeholder="homeassistant.local"
                    onChange={(event) => update({ host: event.target.value })}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="mqtt_port">Port</Label>
                  <Input
                    id="mqtt_port"
                    type="number"
                    min={1}
                    max={65535}
                    value={form.port}
                    onChange={(event) => update({ port: event.target.value })}
                  />
                </div>
              </div>

              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="mqtt_username">Username</Label>
                  <Input
                    id="mqtt_username"
                    value={form.username}
                    autoComplete="off"
                    onChange={(event) => update({ username: event.target.value })}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="mqtt_password">
                    Password{" "}
                    {passwordAlreadySet && !passwordDirty && (
                      <span className="text-xs font-normal text-muted-foreground">(set)</span>
                    )}
                  </Label>
                  <Input
                    id="mqtt_password"
                    type="password"
                    value={password}
                    autoComplete="new-password"
                    onFocus={(event) => {
                      // Clear the placeholder on first focus so the admin edits a
                      // real value, not the bullet sentinel.
                      if (password === PASSWORD_UNCHANGED) {
                        setPassword("")
                        event.currentTarget.value = ""
                      }
                    }}
                    onChange={(event) => setPassword(event.target.value)}
                  />
                </div>
              </div>

              <label htmlFor="mqtt_tls" className="flex items-center justify-between gap-3">
                <span className="text-sm font-medium">Use TLS</span>
                <Switch
                  id="mqtt_tls"
                  checked={form.tls}
                  onCheckedChange={(checked) => update({ tls: checked })}
                />
              </label>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Topics</CardTitle>
              <CardDescription>
                Defaults suit a standard Home Assistant install. Change these only if your broker
                uses a different discovery prefix or topic convention.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="mqtt_discovery_prefix">Discovery prefix</Label>
                <Input
                  id="mqtt_discovery_prefix"
                  value={form.discoveryPrefix}
                  placeholder="homeassistant"
                  onChange={(event) => update({ discoveryPrefix: event.target.value })}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="mqtt_base_topic">Base topic</Label>
                <Input
                  id="mqtt_base_topic"
                  value={form.baseTopic}
                  placeholder="octocam"
                  onChange={(event) => update({ baseTopic: event.target.value })}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="mqtt_client_id">Client ID</Label>
                <Input
                  id="mqtt_client_id"
                  value={form.clientId}
                  placeholder="(derived from device)"
                  onChange={(event) => update({ clientId: event.target.value })}
                />
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardContent className="flex flex-col gap-3">
              {updateSettings.isError && (
                <p className="text-sm text-destructive">{updateSettings.error.message}</p>
              )}
              {updateSettings.isSuccess && !updateSettings.isPending && (
                <p className="text-sm text-success">MQTT settings saved.</p>
              )}
              {status?.state === "connecting" && status.last_error && (
                <p className="text-sm text-muted-foreground">
                  Broker connection failing: {status.last_error}
                </p>
              )}
              <Button type="submit" disabled={updateSettings.isPending} className="self-start">
                {updateSettings.isPending && <Loader2 className="animate-spin" />}
                Save MQTT settings
              </Button>
            </CardContent>
          </Card>
        </form>
      )}
    </div>
  )
}
