import { useEffect, useRef, useState } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { cn } from "@/lib/utils"
import type { StreamFormPatch, StreamFormState } from "@/components/stream/types"

const ALL_ZONES = (1n << 64n) - 1n
const ZONE_COUNT = 64
const SNAPSHOT_ATTEMPTS = 3
const SNAPSHOT_RETRY_MS = 1500

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
          <div className="flex items-center justify-between">
            <Label htmlFor="motion_sensitivity">Sensitivity</Label>
            <span className="text-sm text-muted-foreground tabular-nums">{value.motionSensitivity}</span>
          </div>
          <input
            id="motion_sensitivity"
            type="range"
            min={1}
            max={100}
            value={value.motionSensitivity}
            disabled={!value.motionEnabled}
            onChange={(event) => onChange({ motionSensitivity: event.target.value })}
            className="h-1.5 w-full cursor-pointer appearance-none rounded-full bg-input accent-primary disabled:cursor-not-allowed disabled:opacity-50"
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
  // One snapshot per page load, but the capture is CPU-bound on the device and
  // times out under load, so a single 503 shouldn't cost the backdrop for the
  // whole visit. Retry a couple of times, spaced out, before falling back.
  const [attempt, setAttempt] = useState(0)
  const [snapshotFailed, setSnapshotFailed] = useState(false)
  const retryRef = useRef<number | undefined>(undefined)

  useEffect(() => () => window.clearTimeout(retryRef.current), [])

  function handleSnapshotError() {
    if (attempt + 1 >= SNAPSHOT_ATTEMPTS) {
      setSnapshotFailed(true)
      return
    }
    // Pause before retrying rather than piling onto the same contention.
    retryRef.current = window.setTimeout(() => setAttempt((n) => n + 1), SNAPSHOT_RETRY_MS)
  }

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
        // Best-effort backdrop so zones can be lined up against the actual
        // scene. Falls back to a plain grid once retries are exhausted.
        <img
          key={attempt}
          src={attempt === 0 ? "/snapshot.jpg" : `/snapshot.jpg?attempt=${attempt}`}
          alt=""
          aria-hidden="true"
          className="absolute inset-0 size-full object-cover opacity-70"
          onError={handleSnapshotError}
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
