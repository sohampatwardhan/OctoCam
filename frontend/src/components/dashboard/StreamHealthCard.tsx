import { useStatus } from "@/hooks/useStatus"
import { useSettings } from "@/hooks/useSettings"
import { useMotionEvents } from "@/hooks/useMotionEvents"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"

export function StreamHealthCard() {
  const { data, isLoading, isError } = useStatus()
  const { data: settings } = useSettings()
  const motionDetected = useMotionEvents(data?.motion_detected)
  // Detection being off is a different state from "nothing is moving", and
  // motion_detected reads false in both.
  const motionEnabled = settings?.motion_enabled ?? false
  const rtspState = data?.services.rtsp.state
  const isActive = rtspState === "active"

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>Stream health</CardTitle>
        {!isLoading && !isError && rtspState && (
          <span className="inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
            <span
              className={cn("size-1.5 rounded-full", isActive ? "bg-success" : "bg-muted-foreground/60")}
              aria-hidden="true"
            />
            {isActive ? "Active" : rtspState}
          </span>
        )}
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="grid grid-cols-2 gap-4">
            {Array.from({ length: 4 }, (_, index) => (
              <div key={index} className="flex flex-col gap-1.5">
                <Skeleton className="h-3 w-14" />
                <Skeleton className="h-4 w-20" />
              </div>
            ))}
          </div>
        ) : isError || !data ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <dl className="grid grid-cols-2 gap-4">
            <Metric label="Service" value={data.services.rtsp.state} />
            <Metric
              label="Camera"
              value={data.camera.available ? "Camera online" : data.camera.message || "Camera unavailable"}
            />
            <Metric label="Uptime" value={data.uptime ?? "—"} />
            <Metric label="Web UI" value={data.services.octocam_web.state} />
            <MotionMetric enabled={motionEnabled} detected={motionDetected} />
          </dl>
        )}
      </CardContent>
    </Card>
  )
}

function MotionMetric({ enabled, detected }: { enabled: boolean; detected: boolean }) {
  const value = !enabled ? "Off" : detected ? "Motion detected" : "Clear"
  return (
    <div className="flex flex-col gap-1">
      <dt className="text-xs font-medium text-muted-foreground">Motion</dt>
      <dd className="flex items-center gap-1.5 truncate text-sm font-semibold" title={value}>
        <span
          className={cn(
            "size-1.5 shrink-0 rounded-full",
            !enabled ? "bg-muted-foreground/60" : detected ? "bg-destructive" : "bg-success"
          )}
          aria-hidden="true"
        />
        {value}
      </dd>
    </div>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1">
      <dt className="text-xs font-medium text-muted-foreground">{label}</dt>
      <dd className="truncate text-sm font-semibold" title={value}>
        {value}
      </dd>
    </div>
  )
}
