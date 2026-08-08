import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { Radio } from "lucide-react"
import { apiGet, type RtspUrls } from "@/lib/api"
import { useSettings } from "@/hooks/useSettings"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { RtspUrlRow } from "@/components/stream/RtspUrlRow"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"

// Dashboard shortcut for grabbing the RTSP URLs without a trip to Stream
// Config. `/api/rtsp` is admin-only, so callers gate the trigger on that.
export function RtspUrlsDialog() {
  const [open, setOpen] = useState(false)
  const { data: settings } = useSettings()

  const { data, isLoading, isError } = useQuery({
    queryKey: ["rtsp"],
    queryFn: () => apiGet<RtspUrls>("/api/rtsp"),
    // Nothing to fetch until someone actually wants the URLs.
    enabled: open,
  })

  const rtspEnabled = settings?.rtsp_enabled ?? true

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button variant="secondary" size="sm" />}>RTSP</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Radio className="size-4 text-primary" aria-hidden="true" />
            RTSP stream URLs
          </DialogTitle>
          <DialogDescription>Paste into VLC, an NVR, or any RTSP client.</DialogDescription>
        </DialogHeader>

        {!rtspEnabled && (
          <p className="text-sm text-muted-foreground">
            RTSP is currently turned off. Enable it in Stream Config for these to serve.
          </p>
        )}

        {isLoading ? (
          <div className="flex flex-col gap-4">
            <Skeleton className="h-13 w-full" />
            <Skeleton className="h-13 w-full" />
          </div>
        ) : isError || !data ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <div className="flex flex-col gap-4">
            <RtspUrlRow label="HD" url={data.main} idPrefix="dashboard-rtsp-url" />
            {data.has_sub && <RtspUrlRow label="SD" url={data.sub} idPrefix="dashboard-rtsp-url" />}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
