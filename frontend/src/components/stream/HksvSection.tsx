import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import type { StreamFormPatch, StreamFormState } from "@/components/stream/types"

interface HksvSectionProps {
  value: StreamFormState
  /**
   * Saved motion state from the device, not local form state — motion now
   * saves on its own page, so the gate has to follow what the server actually
   * has rather than unsaved edits elsewhere.
   */
  motionEnabled: boolean
  onChange: (patch: StreamFormPatch) => void
}

// HomeKit Secure Video enable. Disabled whenever motion detection is off,
// mirroring settings::enforce_hksv_requires_motion (settings.rs) — the
// bridge can only start a recording from a motion trigger, so HKSV can't
// mean anything without it.
export function HksvSection({ value, motionEnabled, onChange }: HksvSectionProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>HomeKit Secure Video</CardTitle>
        <CardDescription>
          Requires motion detection, a Home Hub (HomePod/Apple TV), and an iCloud+ plan with HomeKit Secure
          Video storage. Recording quality is chosen by the Home app.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <label htmlFor="hksv_enabled" className="flex items-center justify-between gap-3">
          <span className="text-sm font-medium">Record clips to HomeKit on motion</span>
          <Switch
            id="hksv_enabled"
            checked={value.hksvEnabled && motionEnabled}
            disabled={!motionEnabled}
            onCheckedChange={(checked) => onChange({ hksvEnabled: checked })}
          />
        </label>
        {!motionEnabled && (
          <p className="mt-2 text-sm text-muted-foreground">Turn on motion detection in Motion settings to enable this.</p>
        )}
      </CardContent>
    </Card>
  )
}
