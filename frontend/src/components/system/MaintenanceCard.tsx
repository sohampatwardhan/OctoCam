import { useEffect, useState, type FormEvent } from "react"
import { Loader2 } from "lucide-react"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import type { Settings } from "@/lib/api"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"

type DaySlug = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"

const WEEKDAYS: { slug: DaySlug; label: string; systemd: string }[] = [
  { slug: "mon", label: "Mon", systemd: "mon" },
  { slug: "tue", label: "Tue", systemd: "tue" },
  { slug: "wed", label: "Wed", systemd: "wed" },
  { slug: "thu", label: "Thu", systemd: "thu" },
  { slug: "fri", label: "Fri", systemd: "fri" },
  { slug: "sat", label: "Sat", systemd: "sat" },
  { slug: "sun", label: "Sun", systemd: "sun" },
]

type Days = Record<DaySlug, boolean>

const ALL_DAYS_OFF: Days = { mon: false, tue: false, wed: false, thu: false, fri: false, sat: false, sun: false }

// `scheduled_*_days` from the server is a CSV of systemd weekday
// abbreviations (e.g. "Mon,Tue,Wed,Thu,Fri,Sat,Sun") — see
// scheduled_service_restart_days/scheduled_reboot_days in settings.rs.
function parseDays(csv: string): Days {
  const parts = csv.split(",").map((part) => part.trim().toLowerCase())
  const days = { ...ALL_DAYS_OFF }
  for (const day of WEEKDAYS) {
    days[day.slug] = parts.includes(day.systemd)
  }
  return days
}

export function MaintenanceCard() {
  const { data: settings, isLoading, isError } = useSettings()

  return (
    <Card>
      <CardHeader>
        <CardTitle>Scheduled maintenance</CardTitle>
        <CardDescription>Automatically restart the web service or reboot the device on a schedule.</CardDescription>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="flex flex-col gap-4">
            <Skeleton className="h-24 w-full" />
            <Skeleton className="h-24 w-full" />
          </div>
        ) : isError || !settings ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <div className="flex flex-col gap-6">
            <MaintenanceSection
              title="Restart OctoCam service"
              idPrefix="service-restart"
              enabled={settings.scheduled_service_restart_enabled}
              time={settings.scheduled_service_restart_time}
              days={settings.scheduled_service_restart_days}
              onSave={(enabled, time, days) =>
                ({
                  scheduled_service_restart_enabled: enabled,
                  scheduled_service_restart_time: time,
                  scheduled_service_restart_day_mon: days.mon,
                  scheduled_service_restart_day_tue: days.tue,
                  scheduled_service_restart_day_wed: days.wed,
                  scheduled_service_restart_day_thu: days.thu,
                  scheduled_service_restart_day_fri: days.fri,
                  scheduled_service_restart_day_sat: days.sat,
                  scheduled_service_restart_day_sun: days.sun,
                })
              }
            />
            <div className="border-t border-border" />
            <MaintenanceSection
              title="Reboot device"
              idPrefix="reboot"
              enabled={settings.scheduled_reboot_enabled}
              time={settings.scheduled_reboot_time}
              days={settings.scheduled_reboot_days}
              onSave={(enabled, time, days) =>
                ({
                  scheduled_reboot_enabled: enabled,
                  scheduled_reboot_time: time,
                  scheduled_reboot_day_mon: days.mon,
                  scheduled_reboot_day_tue: days.tue,
                  scheduled_reboot_day_wed: days.wed,
                  scheduled_reboot_day_thu: days.thu,
                  scheduled_reboot_day_fri: days.fri,
                  scheduled_reboot_day_sat: days.sat,
                  scheduled_reboot_day_sun: days.sun,
                })
              }
            />
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function MaintenanceSection({
  title,
  idPrefix,
  enabled: enabledFromServer,
  time: timeFromServer,
  days: daysFromServer,
  onSave,
}: {
  title: string
  idPrefix: string
  enabled: boolean
  time: string
  days: string
  onSave: (enabled: boolean, time: string, days: Days) => Partial<Settings>
}) {
  const updateSettings = useUpdateSettings()
  const [initialized, setInitialized] = useState(false)
  const [enabled, setEnabled] = useState(false)
  const [time, setTime] = useState("00:00")
  const [days, setDays] = useState<Days>(ALL_DAYS_OFF)

  useEffect(() => {
    if (!initialized) {
      setEnabled(enabledFromServer)
      setTime(timeFromServer)
      setDays(parseDays(daysFromServer))
      setInitialized(true)
    }
    // Only seed once — after that, the form owns these fields until Save.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialized])

  function toggleDay(slug: DaySlug) {
    setDays((current) => ({ ...current, [slug]: !current[slug] }))
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    updateSettings.mutate(onSave(enabled, time, days))
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
      <label htmlFor={`${idPrefix}-enabled`} className="flex items-center justify-between gap-3">
        <span className="text-sm font-medium">{title}</span>
        <Switch id={`${idPrefix}-enabled`} checked={enabled} onCheckedChange={setEnabled} />
      </label>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-time`}>Time</Label>
        <Input
          id={`${idPrefix}-time`}
          type="time"
          value={time}
          onChange={(event) => setTime(event.target.value)}
          className="w-32"
        />
      </div>

      <fieldset className="flex flex-col gap-1.5">
        <legend className="text-sm font-medium text-foreground">Days</legend>
        <div className="flex gap-1.5">
          {WEEKDAYS.map((day) => (
            <button
              key={day.slug}
              type="button"
              aria-pressed={days[day.slug]}
              aria-label={day.label}
              onClick={() => toggleDay(day.slug)}
              className={cn(
                "flex h-8 w-9 items-center justify-center rounded-md border text-xs font-medium transition-colors",
                days[day.slug]
                  ? "border-primary bg-primary/10 text-primary"
                  : "border-border bg-transparent text-muted-foreground hover:bg-muted"
              )}
            >
              {day.label}
            </button>
          ))}
        </div>
      </fieldset>

      {updateSettings.isError && <p className="text-sm text-destructive">{updateSettings.error.message}</p>}
      {updateSettings.isSuccess && !updateSettings.isPending && (
        <p className="text-sm text-success">Maintenance schedule saved.</p>
      )}

      <Button type="submit" disabled={updateSettings.isPending} className="self-start">
        {updateSettings.isPending && <Loader2 className="animate-spin" />}
        Save
      </Button>
    </form>
  )
}
