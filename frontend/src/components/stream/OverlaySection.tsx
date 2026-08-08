import { useMutation } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { apiPost } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Select } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import type { StreamFormPatch, StreamFormState } from "@/components/stream/types"

const DATE_FORMATS: { value: string; label: string }[] = [
  { value: "dd/mm/yyyy", label: "DD/MM/YYYY" },
  { value: "mm/dd/yyyy", label: "MM/DD/YYYY" },
  { value: "yyyy-mm-dd", label: "YYYY-MM-DD" },
]

const CLOCK_FORMATS: { value: string; label: string }[] = [
  { value: "24h", label: "24 hour" },
  { value: "12h", label: "12 hour" },
]

const TIME_SERVER_PRESETS = [
  "pool.ntp.org",
  "time.cloudflare.com",
  "time.google.com",
  "time.apple.com",
  "time.windows.com",
]

interface OverlaySectionProps {
  value: StreamFormState
  onChange: (patch: StreamFormPatch) => void
  timezones: string[]
}

// Timestamp overlay + the device's NTP time server. Mirrors the legacy
// template, which groups the "Time server" field and its "Sync now" action
// under the same Overlay panel.
export function OverlaySection({ value, onChange, timezones }: OverlaySectionProps) {
  const syncNow = useMutation({
    mutationFn: () => apiPost<{ success: boolean }>("/api/time/sync", { time_server: value.timeServer }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["settings"] })
    },
  })

  return (
    <Card>
      <CardHeader>
        <CardTitle>Overlay</CardTitle>
        <CardDescription>Burn the date, time, and camera name into the video.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <label htmlFor="text_overlay_enabled" className="flex items-center justify-between gap-3">
          <span className="text-sm font-medium">Date/time + camera name</span>
          <Switch
            id="text_overlay_enabled"
            checked={value.textOverlayEnabled}
            onCheckedChange={(checked) => onChange({ textOverlayEnabled: checked })}
          />
        </label>

        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="text_overlay_timezone">Time zone</Label>
            <Select
              id="text_overlay_timezone"
              value={value.textOverlayTimezone}
              onChange={(event) => onChange({ textOverlayTimezone: event.target.value })}
            >
              {timezones.map((zone) => (
                <option key={zone} value={zone}>
                  {zone}
                </option>
              ))}
            </Select>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="text_overlay_date_format">Date</Label>
            <Select
              id="text_overlay_date_format"
              value={value.textOverlayDateFormat}
              onChange={(event) => onChange({ textOverlayDateFormat: event.target.value })}
            >
              {DATE_FORMATS.map((format) => (
                <option key={format.value} value={format.value}>
                  {format.label}
                </option>
              ))}
            </Select>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="text_overlay_clock_format">Clock</Label>
            <Select
              id="text_overlay_clock_format"
              value={value.textOverlayClockFormat}
              onChange={(event) => onChange({ textOverlayClockFormat: event.target.value })}
            >
              {CLOCK_FORMATS.map((format) => (
                <option key={format.value} value={format.value}>
                  {format.label}
                </option>
              ))}
            </Select>
          </div>
        </div>

        <div className="border-t border-border" />

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="time_server">Time server</Label>
          <div className="flex items-center gap-2">
            <Input
              id="time_server"
              value={value.timeServer}
              maxLength={120}
              list="time-servers"
              onChange={(event) => onChange({ timeServer: event.target.value })}
            />
            <Button
              type="button"
              variant="secondary"
              disabled={syncNow.isPending}
              onClick={() => syncNow.mutate()}
              className="shrink-0"
            >
              {syncNow.isPending && <Loader2 className="animate-spin" />}
              Sync now
            </Button>
          </div>
          <datalist id="time-servers">
            {TIME_SERVER_PRESETS.map((preset) => (
              <option key={preset} value={preset} />
            ))}
          </datalist>
          {syncNow.isError && <p className="text-sm text-destructive">{syncNow.error.message}</p>}
          {syncNow.isSuccess && !syncNow.isPending && (
            <p className="text-sm text-success">Time synced.</p>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
