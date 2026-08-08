import { useEffect, useState, type FormEvent } from "react"
import { useQuery } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { apiGet, type StreamOptions } from "@/lib/api"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { StreamSection } from "@/components/stream/StreamSection"
import { ImageSection } from "@/components/stream/ImageSection"
import { MotionSection } from "@/components/stream/MotionSection"
import { HksvSection } from "@/components/stream/HksvSection"
import { OverlaySection } from "@/components/stream/OverlaySection"
import { RtspSection } from "@/components/stream/RtspSection"
import type { StreamFormPatch, StreamFormState } from "@/components/stream/types"
import { isFormDirty, useReportUnsavedChanges } from "@/hooks/useUnsavedChanges"

const ALL_ZONES = (1n << 64n) - 1n

function parseMotionZones(raw: string): bigint {
  try {
    return BigInt(raw)
  } catch {
    return ALL_ZONES
  }
}

export default function StreamSettings() {
  const { data: settings, isLoading: settingsLoading, isError: settingsError } = useSettings()
  const { data: options, isLoading: optionsLoading, isError: optionsError } = useQuery({
    queryKey: ["stream-options"],
    queryFn: () => apiGet<StreamOptions>("/api/stream-options"),
  })
  const updateSettings = useUpdateSettings()

  const isLoading = settingsLoading || optionsLoading
  const isError = settingsError || optionsError

  const [initialized, setInitialized] = useState(false)
  const [form, setForm] = useState<StreamFormState | null>(null)
  // Snapshot of what the device last confirmed, so the shell can tell edited
  // from merely loaded. Re-baselined on a successful save.
  const [savedForm, setSavedForm] = useState<StreamFormState | null>(null)

  useReportUnsavedChanges(
    { id: "stream-settings", label: "Stream settings", anchorId: "stream-settings-form" },
    isFormDirty(form, savedForm)
  )

  useEffect(() => {
    if (settings && options && !initialized) {
      const resolution =
        options.resolution_presets.find((preset) => preset.selected)?.value ??
        options.resolution_presets[0]?.value ??
        ""
      const subResolution =
        options.sub_resolution_presets.find((preset) => preset.selected)?.value ??
        options.sub_resolution_presets[0]?.value ??
        ""

      const loaded: StreamFormState = {
        cameraEnabled: settings.camera_enabled,
        resolution,
        framerate: String(settings.framerate),
        bitrateKbps: String(settings.bitrate_kbps),
        subStreamEnabled: settings.sub_stream_enabled,
        subResolution,
        subFramerate: String(settings.sub_framerate),
        subBitrateKbps: String(settings.sub_bitrate_kbps),
        rotation: settings.rotation,
        contrast: String(settings.contrast),
        brightness: String(settings.brightness),
        hflip: settings.hflip,
        vflip: settings.vflip,
        noirMode: settings.noir_mode,
        motionEnabled: settings.motion_enabled,
        motionSensitivity: String(settings.motion_sensitivity),
        motionZones: parseMotionZones(settings.motion_zones),
        hksvEnabled: settings.hksv_enabled,
        textOverlayEnabled: settings.text_overlay_enabled,
        textOverlayTimezone: settings.text_overlay_timezone,
        textOverlayDateFormat: settings.text_overlay_date_format,
        textOverlayClockFormat: settings.text_overlay_clock_format,
        timeServer: settings.time_server,
      }
      setForm(loaded)
      setSavedForm(loaded)
      setInitialized(true)
    }
  }, [settings, options, initialized])

  function updateForm(patch: StreamFormPatch) {
    setForm((current) => (current ? { ...current, ...patch } : current))
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    if (!form) return
    const submitted = form
    updateSettings.mutate({
      camera_enabled: form.cameraEnabled,
      resolution: form.resolution,
      framerate: Number(form.framerate) || 1,
      bitrate_kbps: Number(form.bitrateKbps) || 250,
      sub_stream_enabled: form.subStreamEnabled,
      sub_resolution: form.subResolution,
      sub_framerate: Number(form.subFramerate) || 1,
      sub_bitrate_kbps: Number(form.subBitrateKbps) || 150,
      rotation: form.rotation,
      contrast: Number(form.contrast) || 0,
      brightness: Number(form.brightness) || 0,
      hflip: form.hflip,
      vflip: form.vflip,
      noir_mode: form.noirMode,
      motion_enabled: form.motionEnabled,
      motion_sensitivity: Number(form.motionSensitivity) || 1,
      motion_zones: form.motionZones.toString(),
      hksv_enabled: form.hksvEnabled,
      text_overlay_enabled: form.textOverlayEnabled,
      text_overlay_timezone: form.textOverlayTimezone,
      text_overlay_date_format: form.textOverlayDateFormat,
      text_overlay_clock_format: form.textOverlayClockFormat,
      time_server: form.timeServer,
    }, {
      // Only a confirmed save clears the indicator; a failed one leaves the
      // edits flagged.
      onSuccess: () => setSavedForm(submitted),
    })
  }

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">Stream settings</h1>

      {isError ? (
        <p className="text-sm text-muted-foreground">Device unreachable.</p>
      ) : isLoading || !form || !options ? (
        <div className="flex flex-col gap-6">
          <Card>
            <CardHeader>
              <CardTitle>Stream</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <Skeleton className="h-5 w-full" />
              <Skeleton className="h-8 w-full" />
              <Skeleton className="h-8 w-full" />
              <Skeleton className="h-8 w-full" />
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Image</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <Skeleton className="h-8 w-full" />
              <Skeleton className="h-8 w-full" />
            </CardContent>
          </Card>
        </div>
      ) : (
        <form id="stream-settings-form" className="flex flex-col gap-6" onSubmit={handleSubmit}>
          <StreamSection
            value={form}
            onChange={updateForm}
            resolutionPresets={options.resolution_presets}
            subResolutionPresets={options.sub_resolution_presets}
          />
          <ImageSection value={form} onChange={updateForm} rotations={options.rotations} />
          <MotionSection value={form} onChange={updateForm} />
          <HksvSection value={form} onChange={updateForm} />
          <OverlaySection value={form} onChange={updateForm} timezones={options.timezones} />

          <Card>
            <CardContent className="flex flex-col gap-3">
              {updateSettings.isError && (
                <p className="text-sm text-destructive">{updateSettings.error.message}</p>
              )}
              {updateSettings.isSuccess && !updateSettings.isPending && (
                <p className="text-sm text-success">Stream settings saved.</p>
              )}
              <Button type="submit" disabled={updateSettings.isPending} className="self-start">
                {updateSettings.isPending && <Loader2 className="animate-spin" />}
                Save stream settings
              </Button>
            </CardContent>
          </Card>
        </form>
      )}

      <RtspSection />
    </div>
  )
}
