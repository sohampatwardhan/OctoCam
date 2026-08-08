import { useEffect, useState, type FormEvent } from "react"
import { useQuery } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { apiGet, type StreamOptions } from "@/lib/api"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { MotionSection } from "@/components/stream/MotionSection"
import type { MotionFormPatch, MotionFormState } from "@/components/stream/types"
import { isFormDirty, useReportUnsavedChanges } from "@/hooks/useUnsavedChanges"

const ALL_ZONES = (1n << 64n) - 1n

function parseMotionZones(raw: string): bigint {
  try {
    return BigInt(raw)
  } catch {
    return ALL_ZONES
  }
}

export default function MotionSettings() {
  const { data: settings, isLoading: settingsLoading, isError: settingsError } = useSettings()
  // Only for the camera's aspect ratio, which shapes the zone grid.
  const { data: options } = useQuery({
    queryKey: ["stream-options"],
    queryFn: () => apiGet<StreamOptions>("/api/stream-options"),
  })
  const updateSettings = useUpdateSettings()

  const [initialized, setInitialized] = useState(false)
  const [form, setForm] = useState<MotionFormState | null>(null)
  const [savedForm, setSavedForm] = useState<MotionFormState | null>(null)

  useReportUnsavedChanges(
    { id: "motion", label: "Motion detection", anchorId: "motion-settings-form" },
    isFormDirty(form, savedForm)
  )

  useEffect(() => {
    if (settings && !initialized) {
      const loaded: MotionFormState = {
        motionEnabled: settings.motion_enabled,
        motionSensitivity: String(settings.motion_sensitivity),
        motionZones: parseMotionZones(settings.motion_zones),
      }
      setForm(loaded)
      setSavedForm(loaded)
      setInitialized(true)
    }
  }, [settings, initialized])

  function updateForm(patch: MotionFormPatch) {
    setForm((current) => (current ? { ...current, ...patch } : current))
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    if (!form) return
    const submitted = form
    updateSettings.mutate(
      {
        motion_enabled: form.motionEnabled,
        motion_sensitivity: Number(form.motionSensitivity) || 1,
        motion_zones: form.motionZones.toString(),
      },
      { onSuccess: () => setSavedForm(submitted) }
    )
  }

  const resolution =
    options?.resolution_presets.find((preset) => preset.selected)?.value ??
    options?.resolution_presets[0]?.value ??
    ""

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">Motion detection</h1>

      {settingsError ? (
        <p className="text-sm text-muted-foreground">Device unreachable.</p>
      ) : settingsLoading || !form ? (
        <Card>
          <CardHeader>
            <CardTitle>Motion detection</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Skeleton className="h-5 w-full" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-40 w-full max-w-md" />
          </CardContent>
        </Card>
      ) : (
        <form id="motion-settings-form" className="flex flex-col gap-6" onSubmit={handleSubmit}>
          <MotionSection value={form} resolution={resolution} onChange={updateForm} />

          <Card>
            <CardContent className="flex flex-col gap-3">
              {updateSettings.isError && (
                <p className="text-sm text-destructive">{updateSettings.error.message}</p>
              )}
              {updateSettings.isSuccess && !updateSettings.isPending && (
                <p className="text-sm text-success">Motion settings saved.</p>
              )}
              <Button type="submit" disabled={updateSettings.isPending} className="self-start">
                {updateSettings.isPending && <Loader2 className="animate-spin" />}
                Save motion settings
              </Button>
            </CardContent>
          </Card>
        </form>
      )}
    </div>
  )
}
