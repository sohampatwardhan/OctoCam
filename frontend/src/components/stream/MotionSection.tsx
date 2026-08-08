import { useRef, useState } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { cn } from "@/lib/utils"
import type { StreamFormPatch, StreamFormState } from "@/components/stream/types"

const ALL_ZONES = (1n << 64n) - 1n
const ZONE_COUNT = 64

interface MotionSectionProps {
  value: StreamFormState
  onChange: (patch: StreamFormPatch) => void
}

// Motion detection enable, sensitivity, and the 8x8 zone bitmask editor.
// Bit N of `motionZones` (0..63, row-major, see motion.rs's grid_idx mapping)
// is set when that cell is MONITORED for motion; clearing a bit excludes
// that region (e.g. a road or a tree) from triggering detection.
export function MotionSection({ value, onChange }: MotionSectionProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Motion detection</CardTitle>
        <CardDescription>Alerts and logging when movement crosses the sensitivity threshold.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <label htmlFor="motion_enabled" className="flex items-center justify-between gap-3">
          <span className="text-sm font-medium">Motion alerts &amp; logging</span>
          <Switch
            id="motion_enabled"
            checked={value.motionEnabled}
            onCheckedChange={(checked) => onChange({ motionEnabled: checked })}
          />
        </label>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="motion_sensitivity">Sensitivity (1-100)</Label>
          <Input
            id="motion_sensitivity"
            type="number"
            min={1}
            max={100}
            value={value.motionSensitivity}
            disabled={!value.motionEnabled}
            onChange={(event) => onChange({ motionSensitivity: event.target.value })}
            className="max-w-32"
          />
        </div>

        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between gap-3">
            <Label>Detection zones</Label>
            <div className="flex gap-2">
              <Button
                type="button"
                variant="secondary"
                size="xs"
                disabled={!value.motionEnabled}
                onClick={() => onChange({ motionZones: ALL_ZONES })}
              >
                Monitor all
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="xs"
                disabled={!value.motionEnabled}
                onClick={() => onChange({ motionZones: 0n })}
              >
                Clear all
              </Button>
            </div>
          </div>
          <p className="text-sm text-muted-foreground">
            Tap or drag to include/exclude regions. Highlighted zones are monitored; dimmed zones are ignored.
          </p>
          <MotionZoneGrid
            mask={value.motionZones}
            disabled={!value.motionEnabled}
            onChange={(mask) => onChange({ motionZones: mask })}
          />
        </div>
      </CardContent>
    </Card>
  )
}

function MotionZoneGrid({
  mask,
  disabled,
  onChange,
}: {
  mask: bigint
  disabled: boolean
  onChange: (next: bigint) => void
}) {
  const isDrawingRef = useRef(false)
  const drawModeRef = useRef(true)
  const [snapshotFailed, setSnapshotFailed] = useState(false)

  function setCell(index: number, active: boolean) {
    const bit = 1n << BigInt(index)
    onChange(active ? mask | bit : mask & ~bit)
  }

  function startDrawing(index: number) {
    if (disabled) return
    const currentlyActive = (mask & (1n << BigInt(index))) !== 0n
    isDrawingRef.current = true
    drawModeRef.current = !currentlyActive
    setCell(index, drawModeRef.current)
  }

  function continueDrawing(index: number) {
    if (disabled || !isDrawingRef.current) return
    setCell(index, drawModeRef.current)
  }

  return (
    <div
      className={cn(
        "relative aspect-video w-full max-w-md touch-none overflow-hidden rounded-lg border border-border bg-muted select-none",
        disabled && "pointer-events-none opacity-50"
      )}
      onPointerUp={() => {
        isDrawingRef.current = false
      }}
      onPointerLeave={() => {
        isDrawingRef.current = false
      }}
    >
      {!snapshotFailed && (
        // Best-effort live backdrop so zones can be lined up against the
        // actual scene. Silently falls back to a plain grid if the camera is
        // unavailable or the snapshot route 404s/errors.
        <img
          src="/snapshot.jpg"
          alt=""
          aria-hidden="true"
          className="absolute inset-0 size-full object-cover opacity-70"
          onError={() => setSnapshotFailed(true)}
        />
      )}
      <div className="relative grid size-full grid-cols-8 grid-rows-8">
        {Array.from({ length: ZONE_COUNT }, (_, index) => {
          const active = (mask & (1n << BigInt(index))) !== 0n
          return (
            <button
              key={index}
              type="button"
              tabIndex={-1}
              aria-label={`Zone ${index + 1}, ${active ? "monitored" : "ignored"}`}
              aria-pressed={active}
              className={cn(
                "border border-white/15 transition-colors",
                active ? "bg-primary/45 hover:bg-primary/55" : "bg-black/25 hover:bg-black/15"
              )}
              onPointerDown={(event) => {
                event.preventDefault()
                startDrawing(index)
              }}
              onPointerEnter={() => continueDrawing(index)}
            />
          )
        })}
      </div>
    </div>
  )
}
