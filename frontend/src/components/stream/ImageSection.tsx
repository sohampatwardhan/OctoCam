import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Select } from "@/components/ui/select"
import type { StreamFormPatch, StreamFormState } from "@/components/stream/types"

interface ImageSectionProps {
  value: StreamFormState
  onChange: (patch: StreamFormPatch) => void
  rotations: number[]
}

// Rotation, contrast/brightness, and the flip/NoIR checks. Mirrors the
// "Image controls" subsection of the legacy template.
export function ImageSection({ value, onChange, rotations }: ImageSectionProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Image</CardTitle>
        <CardDescription>Orientation and picture adjustments applied to both streams.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="rotation">Rotation</Label>
            <Select
              id="rotation"
              value={value.rotation}
              onChange={(event) => onChange({ rotation: Number(event.target.value) })}
            >
              {rotations.map((rotation) => (
                <option key={rotation} value={rotation}>
                  {rotation}&deg;
                </option>
              ))}
            </Select>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="contrast">Contrast</Label>
            <Input
              id="contrast"
              type="number"
              min={0}
              max={4}
              step={0.1}
              value={value.contrast}
              onChange={(event) => onChange({ contrast: event.target.value })}
            />
          </div>
        </div>

        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <Label htmlFor="brightness">Brightness</Label>
            <span className="text-sm text-muted-foreground tabular-nums">{value.brightness}</span>
          </div>
          <input
            id="brightness"
            type="range"
            min={-100}
            max={100}
            value={value.brightness}
            onChange={(event) => onChange({ brightness: event.target.value })}
            className="h-1.5 w-full cursor-pointer appearance-none rounded-full bg-input accent-primary"
          />
        </div>

        <div className="flex flex-col gap-2.5">
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={value.hflip}
              onChange={(event) => onChange({ hflip: event.target.checked })}
              className="size-4 rounded border-input accent-primary"
            />
            Horizontal flip
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={value.vflip}
              onChange={(event) => onChange({ vflip: event.target.checked })}
              className="size-4 rounded border-input accent-primary"
            />
            Vertical flip
          </label>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={value.noirMode}
              onChange={(event) => onChange({ noirMode: event.target.checked })}
              className="size-4 rounded border-input accent-primary"
            />
            NoIR correction (infrared white balance)
          </label>
        </div>
      </CardContent>
    </Card>
  )
}
