import type { ReactNode } from "react"
import { WifiHigh, WifiLow, WifiOff, WifiZero } from "lucide-react"
import { useStatus } from "@/hooks/useStatus"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { signalLevel, signalPercent } from "@/lib/wifi"
import { cn } from "@/lib/utils"
import type { WifiStatusSummary } from "@/lib/api"

const SIGNAL_ICON = {
  high: WifiHigh,
  low: WifiLow,
  zero: WifiZero,
}

export function StatusCard() {
  const { data, isLoading, isError } = useStatus()

  return (
    <Card>
      <CardHeader>
        <CardTitle>System info</CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="flex flex-col gap-3">
            {Array.from({ length: 8 }, (_, index) => (
              <Skeleton key={index} className="h-5 w-full" />
            ))}
          </div>
        ) : isError || !data ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <>
            <dl className="flex flex-col divide-y divide-border">
              <WifiSignalRow wifi={data.wifi} />
              <StatusRow label="Address">{data.ip_addresses.join(", ") || "Not available"}</StatusRow>
              <StatusRow label="Uptime">{data.uptime ?? "Not available"}</StatusRow>
              <StatusRow label="CPU temp">
                {data.cpu_temp_c != null ? `${data.cpu_temp_c.toFixed(1)} °C` : "Not available"}
              </StatusRow>
              <MeterRow
                label="CPU usage"
                valueText={
                  data.resources.cpu_usage_percent != null ? `${data.resources.cpu_usage_percent.toFixed(1)}%` : "Not available"
                }
                percent={data.resources.cpu_usage_percent ?? 0}
              />
              <StatusRow label="Load average">{data.resources.load_average ?? "Not available"}</StatusRow>
              <MeterRow
                label="Memory"
                valueText={
                  data.resources.memory.total_mb > 0
                    ? `${data.resources.memory.used_mb} / ${data.resources.memory.total_mb} MB${
                        data.resources.memory.used_percent != null
                          ? ` (${data.resources.memory.used_percent.toFixed(1)}%)`
                          : ""
                      }`
                    : "Not available"
                }
                percent={data.resources.memory.used_percent ?? 0}
              />
              {data.resources.memory.swap_total_mb > 0 && (
                <MeterRow
                  label="Swap"
                  valueText={`${data.resources.memory.swap_used_mb} / ${data.resources.memory.swap_total_mb} MB${
                    data.resources.memory.swap_used_percent != null
                      ? ` (${data.resources.memory.swap_used_percent.toFixed(1)}%)`
                      : ""
                  }`}
                  percent={data.resources.memory.swap_used_percent ?? 0}
                />
              )}
              <ServiceRow label="Web UI" state={data.services.octocam_web.state} />
              <ServiceRow label="RTSP" state={data.services.rtsp.state} />
              <ServiceRow label="HomeKit" state={data.services.homekit.state} />
            </dl>

            {(() => {
              const rows = wifiDetailRows(data.wifi)
              return rows.length > 0 ? (
                <div className="mt-4 flex flex-col gap-2">
                  <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Wi-Fi details</p>
                  <dl className="flex flex-col divide-y divide-border">
                    {rows.map((row) => (
                      <StatusRow key={row.label} label={row.label}>
                        {row.value}
                      </StatusRow>
                    ))}
                  </dl>
                </div>
              ) : null
            })()}
          </>
        )}
      </CardContent>
    </Card>
  )
}

function StatusRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-1.5 first:pt-0 last:pb-0">
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd className="truncate text-sm font-medium">{children}</dd>
    </div>
  )
}

function MeterRow({ label, valueText, percent }: { label: string; valueText: string; percent: number }) {
  const clamped = Math.min(100, Math.max(0, percent))
  return (
    <div className="flex flex-col gap-1.5 py-1.5 first:pt-0 last:pb-0">
      <div className="flex items-center justify-between gap-4">
        <dt className="text-sm text-muted-foreground">{label}</dt>
        <dd className="text-sm font-medium">{valueText}</dd>
      </div>
      <span className="block h-1.5 w-full overflow-hidden rounded-full bg-muted" aria-hidden="true">
        <span className="block h-full rounded-full bg-primary transition-[width]" style={{ width: `${clamped}%` }} />
      </span>
    </div>
  )
}

function ServiceRow({ label, state }: { label: string; state: string }) {
  const active = state === "active"
  return (
    <div className="flex items-center justify-between gap-4 py-1.5 first:pt-0 last:pb-0">
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd className="inline-flex items-center gap-1.5 text-sm font-medium capitalize">
        <span className={cn("size-1.5 rounded-full", active ? "bg-success" : "bg-muted-foreground/60")} aria-hidden="true" />
        {state}
      </dd>
    </div>
  )
}

function WifiSignalRow({ wifi }: { wifi: WifiStatusSummary }) {
  const connected = wifi.state === "connected"
  const percent = signalPercent(wifi.signal_dbm)
  const level = signalLevel(percent)
  const Icon = connected ? SIGNAL_ICON[level] : WifiOff

  return (
    <div className="flex items-center justify-between gap-4 py-1.5 first:pt-0 last:pb-0">
      <dt className="text-sm text-muted-foreground">Wi-Fi</dt>
      <dd className="inline-flex items-center gap-1.5 text-sm font-medium">
        <span>{connected ? wifi.ssid ?? "Connected" : wifi.message || "Not connected"}</span>
        <Icon
          className={cn("size-4", connected ? "text-primary" : "text-muted-foreground")}
          aria-hidden="true"
        />
        {connected && wifi.signal_dbm && <span className="text-xs text-muted-foreground">{Math.round(percent)}%</span>}
      </dd>
    </div>
  )
}

// Mirrors wifi_details() in rust/octocam-web/src/system.rs — same row order,
// same "only show what's present" filtering.
function wifiDetailRows(wifi: WifiStatusSummary): { label: string; value: string }[] {
  const rows: { label: string; value: string }[] = []
  const push = (label: string, value: string | null | undefined) => {
    if (value) rows.push({ label, value })
  }

  push("Interface", wifi.interface)
  push("IP address", wifi.ip_address)
  push("MAC address", wifi.mac_address)
  push("BSSID", wifi.bssid)
  push("Security", wifi.security)
  push("PHY mode", wifi.wifi_generation_label)
  if (wifi.frequency_mhz != null) {
    let value = `${wifi.frequency_mhz} MHz`
    if (wifi.band) value += ` · ${wifi.band}`
    if (wifi.channel != null) value += ` · Channel ${wifi.channel}`
    rows.push({ label: "Frequency", value })
  }
  push("Channel width", wifi.channel_width)
  push("RSSI", wifi.signal_dbm)
  push("RX rate", wifi.rx_bitrate)
  push("TX rate", wifi.tx_bitrate)
  push("TX power", wifi.tx_power)
  if (wifi.default_interface) {
    rows.push({
      label: "Default route",
      value: wifi.default_gateway ? `${wifi.default_interface} via ${wifi.default_gateway}` : wifi.default_interface,
    })
  }

  return rows
}
