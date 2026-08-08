import { useEffect, useRef, useState } from "react"
import { Loader2 } from "lucide-react"
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
const DEFAULT_ASPECT = "4 / 3"

/** Whether a drag marks cells as monitored or ignored. */
type PaintMode = "monitor" | "ignore"

// Resolution presets are "WIDTHxHEIGHT". The grid has to match the camera's
// shape or the cells map to the wrong part of the scene, and the snapshot
// backdrop gets cropped by object-cover rather than lining up with the zones.
function aspectRatioFrom(resolution: string): string {
  const [width, height] = resolution.split("x").map(Number)
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return DEFAULT_ASPECT
  }
  return `${width} / ${height}`
}

interface MotionSectionProps {
  value: StreamFormState
  onChange: (patch: StreamFormPatch) => void
}

// Motion detection enable, sensitivity, and the 8x8 zone bitmask editor.
// Bit N of `motionZones` (0..63, row-major, see motion.rs's grid_idx mapping)
// is set when that cell is MONITORED for motion; clearing a bit excludes
// that region (e.g. a road or a tree) from triggering detection.
export function MotionSection({ value, onChange }: MotionSectionProps) {
  const [paintMode, setPaintMode] = useState<PaintMode>("monitor")

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
                Ignore all
              </Button>
            </div>
          </div>
          <p className="text-sm text-muted-foreground">
            Pick a brush, then tap or drag over the scene. Green regions are watched for motion; red
            regions are ignored.
          </p>

          <PaintModeToggle mode={paintMode} disabled={!value.motionEnabled} onChange={setPaintMode} />

          <MotionZoneGrid
            mask={value.motionZones}
            disabled={!value.motionEnabled}
            paintMode={paintMode}
            aspectRatio={aspectRatioFrom(value.resolution)}
            onChange={(mask) => onChange({ motionZones: mask })}
          />
        </div>
      </CardContent>
    </Card>
  )
}

// Explicit brush instead of inferring intent from the first cell touched, which
// made a drag's effect depend on where it started.
function PaintModeToggle({
  mode,
  disabled,
  onChange,
}: {
  mode: PaintMode
  disabled: boolean
  onChange: (next: PaintMode) => void
}) {
  const options: { value: PaintMode; label: string; selected: string }[] = [
    { value: "monitor", label: "Detect motion", selected: "bg-success text-white" },
    { value: "ignore", label: "Ignore motion", selected: "bg-destructive text-white" },
  ]

  return (
    <div
      className={cn(
        "inline-flex self-start overflow-hidden rounded-lg border border-border bg-input",
        disabled && "pointer-events-none opacity-50"
      )}
      role="group"
      aria-label="Zone brush"
    >
      {options.map((option, index) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={mode === option.value}
          onClick={() => onChange(option.value)}
          className={cn(
            "px-3 py-1.5 text-xs font-semibold tracking-wide transition-colors",
            index > 0 && "border-l border-border",
            mode === option.value
              ? option.selected
              : "text-muted-foreground hover:bg-card hover:text-foreground"
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  )
}

function MotionZoneGrid({
  mask,
  disabled,
  paintMode,
  aspectRatio,
  onChange,
}: {
  mask: bigint
  disabled: boolean
  paintMode: PaintMode
  aspectRatio: string
  onChange: (next: bigint) => void
}) {
  const isDrawingRef = useRef(false)
  const drawModeRef = useRef(true)
  // One snapshot per page load, but the capture is CPU-bound on the device and
  // times out under load, so a single 503 shouldn't cost the backdrop for the
  // whole visit. Retry a couple of times, spaced out, before falling back.
  const [attempt, setAttempt] = useState(0)
  const [snapshotFailed, setSnapshotFailed] = useState(false)
  const [snapshotLoaded, setSnapshotLoaded] = useState(false)
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

  function retrySnapshot() {
    setSnapshotFailed(false)
    setSnapshotLoaded(false)
    setAttempt((n) => n + 1)
  }

  function setCell(index: number, active: boolean) {
    const bit = 1n << BigInt(index)
    onChange(active ? mask | bit : mask & ~bit)
  }

  function startDrawing(index: number) {
    if (disabled) return
    isDrawingRef.current = true
    drawModeRef.current = paintMode === "monitor"
    setCell(index, drawModeRef.current)
  }

  function continueDrawing(index: number) {
    if (disabled || !isDrawingRef.current) return
    setCell(index, drawModeRef.current)
  }

  return (
    <div
      style={{ aspectRatio }}
      className={cn(
        "relative w-full max-w-md touch-none overflow-hidden rounded-lg border border-border bg-muted select-none",
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
        // The zones only mean anything against the scene they mask, so the
        // grid stays hidden until this resolves. Kept mounted while loading so
        // the request is actually in flight.
        <img
          key={attempt}
          src={attempt === 0 ? "/snapshot.jpg" : `/snapshot.jpg?attempt=${attempt}`}
          alt=""
          aria-hidden="true"
          className={cn(
            "absolute inset-0 size-full object-cover opacity-70",
            !snapshotLoaded && "invisible"
          )}
          onLoad={() => setSnapshotLoaded(true)}
          onError={handleSnapshotError}
        />
      )}

      {snapshotFailed ? (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-4 text-center">
          <p className="text-sm text-muted-foreground">
            Couldn't load a camera snapshot, so there's no scene to place zones against.
          </p>
          <Button type="button" variant="secondary" size="xs" onClick={retrySnapshot}>
            Try again
          </Button>
        </div>
      ) : !snapshotLoaded ? (
        <div className="absolute inset-0 flex items-center justify-center gap-2 text-muted-foreground">
          <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          <p className="text-sm">Loading...</p>
        </div>
      ) : (
        <div className="relative grid size-full grid-cols-8 grid-rows-8">
          {Array.from({ length: ZONE_COUNT }, (_, index) => {
            const active = (mask & (1n << BigInt(index))) !== 0n
            return (
              <button
                key={index}
                type="button"
                tabIndex={-1}
                aria-label={`Zone ${index + 1}, ${active ? "detecting motion" : "ignoring motion"}`}
                aria-pressed={active}
                className={cn(
                  "border border-white/15 transition-colors",
                  active ? "bg-success/45 hover:bg-success/55" : "bg-destructive/40 hover:bg-destructive/50"
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
      )}
    </div>
  )
}
