import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { CopyButton } from "@/components/CopyButton"

// Shared by the Stream Config page and the dashboard's RTSP dialog so both
// present the same copyable URL. `idPrefix` keeps the label/input association
// unique when more than one set is mounted.
export function RtspUrlRow({
  label,
  url,
  idPrefix = "rtsp-url",
}: {
  label: string
  url: string
  idPrefix?: string
}) {
  const id = `${idPrefix}-${label}`
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label} URL</Label>
      <div className="flex items-center gap-2">
        <Input id={id} value={url} readOnly className="font-mono text-xs" />
        <CopyButton value={url} />
      </div>
    </div>
  )
}
