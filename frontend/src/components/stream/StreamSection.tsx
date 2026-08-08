import type { PresetOption } from "@/lib/api"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Select } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import type { StreamFormPatch, StreamFormState } from "@/components/stream/types"

interface StreamSectionProps {
  value: StreamFormState
  onChange: (patch: StreamFormPatch) => void
  resolutionPresets: PresetOption[]
  subResolutionPresets: PresetOption[]
}

// Camera enable + the HD and SD stream encoder settings. Mirrors the "Stream"
// panel in the legacy stream_settings.html template (camera toggle, then the
// HD/SD subsections).
export function StreamSection({ value, onChange, resolutionPresets, subResolutionPresets }: StreamSectionProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Stream</CardTitle>
        <CardDescription>Camera encoder settings for the HD and SD video streams.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-6">
        <label htmlFor="camera_enabled" className="flex items-center justify-between gap-3">
          <span className="text-sm font-medium">Camera</span>
          <Switch
            id="camera_enabled"
            checked={value.cameraEnabled}
            onCheckedChange={(checked) => onChange({ cameraEnabled: checked })}
          />
        </label>

        <div className="flex flex-col gap-3">
          <h3 className="text-sm font-semibold text-foreground">HD stream</h3>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="resolution">Resolution</Label>
            <Select
              id="resolution"
              value={value.resolution}
              onChange={(event) => onChange({ resolution: event.target.value })}
            >
              {resolutionPresets.map((preset) => (
                <option key={preset.value} value={preset.value}>
                  {preset.label}
                </option>
              ))}
            </Select>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="framerate">FPS</Label>
              <Input
                id="framerate"
                type="number"
                min={1}
                max={60}
                value={value.framerate}
                onChange={(event) => onChange({ framerate: event.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="bitrate_kbps">Bitrate kbps</Label>
              <Input
                id="bitrate_kbps"
                type="number"
                min={250}
                max={25000}
                step={250}
                value={value.bitrateKbps}
                onChange={(event) => onChange({ bitrateKbps: event.target.value })}
              />
            </div>
          </div>
        </div>

        <div className="border-t border-border" />

        <div className="flex flex-col gap-3">
          <label htmlFor="sub_stream_enabled" className="flex items-center justify-between gap-3">
            <span className="text-sm font-semibold text-foreground">SD stream</span>
            <Switch
              id="sub_stream_enabled"
              checked={value.subStreamEnabled}
              onCheckedChange={(checked) => onChange({ subStreamEnabled: checked })}
            />
          </label>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="sub_resolution">Resolution</Label>
            <Select
              id="sub_resolution"
              value={value.subResolution}
              onChange={(event) => onChange({ subResolution: event.target.value })}
              disabled={!value.subStreamEnabled}
            >
              {subResolutionPresets.map((preset) => (
                <option key={preset.value} value={preset.value}>
                  {preset.label}
                </option>
              ))}
            </Select>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="sub_framerate">FPS</Label>
              <Input
                id="sub_framerate"
                type="number"
                min={1}
                max={30}
                value={value.subFramerate}
                disabled={!value.subStreamEnabled}
                onChange={(event) => onChange({ subFramerate: event.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="sub_bitrate_kbps">Bitrate kbps</Label>
              <Input
                id="sub_bitrate_kbps"
                type="number"
                min={150}
                max={5000}
                step={50}
                value={value.subBitrateKbps}
                disabled={!value.subStreamEnabled}
                onChange={(event) => onChange({ subBitrateKbps: event.target.value })}
              />
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
