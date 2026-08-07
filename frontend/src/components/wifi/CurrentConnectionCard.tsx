import { WifiHigh, WifiLow, WifiOff, WifiZero } from "lucide-react"
import { useStatus } from "@/hooks/useStatus"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { signalLevel, signalPercent } from "@/lib/wifi"
import { cn } from "@/lib/utils"

const SIGNAL_ICON = {
  high: WifiHigh,
  low: WifiLow,
  zero: WifiZero,
}

export function CurrentConnectionCard() {
  const { data, isLoading, isError } = useStatus()
  const wifi = data?.wifi
  const connected = wifi?.state === "connected"

  return (
    <Card>
      <CardHeader>
        <CardTitle>Current connection</CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="flex flex-col gap-4">
            <div className="flex items-center gap-3">
              <Skeleton className="size-9 rounded-full" />
              <div className="flex flex-col gap-1.5">
                <Skeleton className="h-4 w-32" />
                <Skeleton className="h-3 w-20" />
              </div>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-4 w-24" />
            </div>
          </div>
        ) : isError || !data || !wifi ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <div className="flex flex-col gap-4">
            <SignalHeader connected={connected} ssid={wifi.ssid} signalDbm={wifi.signal_dbm} />
            <dl className="grid grid-cols-2 gap-4">
              <Metric label="IP address" value={wifi.ip_addresses[0] ?? "—"} />
              <Metric
                label="Band"
                value={[wifi.band, wifi.wifi_generation_label].filter(Boolean).join(" · ") || "—"}
              />
            </dl>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function SignalHeader({
  connected,
  ssid,
  signalDbm,
}: {
  connected: boolean
  ssid: string | null
  signalDbm: string | null
}) {
  const percent = signalPercent(signalDbm)
  const level = signalLevel(percent)
  const Icon = connected ? SIGNAL_ICON[level] : WifiOff

  return (
    <div className="flex items-center gap-3">
      <span
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-full",
          connected ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"
        )}
      >
        <Icon className="size-4.5" aria-hidden="true" />
      </span>
      <div className="flex flex-col">
        <span className="text-sm font-semibold">{connected && ssid ? ssid : "Not connected"}</span>
        {connected && <span className="text-xs text-muted-foreground">{Math.round(percent)}% signal</span>}
      </div>
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
