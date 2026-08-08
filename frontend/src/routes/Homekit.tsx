import type { ReactNode } from "react"
import { useQuery } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import { AppleHomeKitIcon } from "@/components/icons/selfhst"
import { apiGet, type HomeKitInfo } from "@/lib/api"
import { useStatus } from "@/hooks/useStatus"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import { Skeleton } from "@/components/ui/skeleton"

export default function Homekit() {
  const { data: settings, isLoading: settingsLoading, isError: settingsError } = useSettings()
  const updateSettings = useUpdateSettings()
  const { data: status } = useStatus()
  const {
    data: homekit,
    isLoading: homekitLoading,
    isError: homekitError,
  } = useQuery({
    queryKey: ["homekit"],
    queryFn: () => apiGet<HomeKitInfo>("/api/homekit"),
  })

  const enabled = settings?.homekit_enabled ?? false
  const bridgeLoading = settingsLoading || homekitLoading

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">HomeKit</h1>

      {settingsError ? (
        <p className="text-sm text-muted-foreground">Device unreachable.</p>
      ) : (
        <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Bridge</CardTitle>
              <CardDescription>Publish this camera to Apple Home as a HomeKit accessory.</CardDescription>
            </CardHeader>
            <CardContent>
              {settingsLoading ? (
                <div className="flex flex-col gap-4">
                  <Skeleton className="h-5 w-full" />
                  <Skeleton className="h-8 w-full" />
                </div>
              ) : (
                <div className="flex flex-col gap-4">
                  <label htmlFor="homekit_enabled" className="flex items-center justify-between gap-3">
                    <span className="text-sm font-medium">Bridge enabled</span>
                    <span className="flex items-center gap-2">
                      {updateSettings.isPending && (
                        <Loader2 className="size-3.5 animate-spin text-muted-foreground" aria-hidden="true" />
                      )}
                      <Switch
                        id="homekit_enabled"
                        checked={enabled}
                        disabled={updateSettings.isPending}
                        onCheckedChange={(checked) => updateSettings.mutate({ homekit_enabled: checked })}
                      />
                    </span>
                  </label>

                  {updateSettings.isError && (
                    <p className="text-sm text-destructive">{updateSettings.error.message}</p>
                  )}

                  {bridgeLoading ? (
                    <div className="grid grid-cols-3 gap-4">
                      {Array.from({ length: 3 }, (_, index) => (
                        <div key={index} className="flex flex-col gap-1.5">
                          <Skeleton className="h-3 w-14" />
                          <Skeleton className="h-4 w-16" />
                        </div>
                      ))}
                    </div>
                  ) : (
                    <dl className="grid grid-cols-3 gap-4">
                      <Metric label="Service" value={status?.services.homekit.state ?? "—"} />
                      <Metric label="Pairing" value={homekit?.paired ? "Paired" : "Not paired"} />
                      <Metric label="Accessory" value={homekit?.status ?? "—"} />
                    </dl>
                  )}
                </div>
              )}
            </CardContent>
          </Card>

          <PairingCard
            enabled={enabled}
            settingsLoading={settingsLoading}
            homekit={homekit}
            isLoading={homekitLoading}
            isError={homekitError}
          />
        </div>
      )}
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

function PairingCard({
  enabled,
  settingsLoading,
  homekit,
  isLoading,
  isError,
}: {
  enabled: boolean
  settingsLoading: boolean
  homekit: HomeKitInfo | undefined
  isLoading: boolean
  isError: boolean
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <AppleHomeKitIcon className="size-4 text-primary" aria-hidden="true" />
          Pairing
        </CardTitle>
        <CardDescription>Add this camera to the Home app on your iPhone or iPad.</CardDescription>
      </CardHeader>
      <CardContent>
        {settingsLoading || isLoading ? (
          <div className="flex flex-col gap-4">
            <Skeleton className="h-24 w-full" />
          </div>
        ) : !enabled ? (
          <InfoBox tone="muted">Enable the bridge and it will be published to Apple Home.</InfoBox>
        ) : isError || !homekit ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : homekit.paired ? (
          <InfoBox tone="success">This camera is marked paired in OctoCam settings.</InfoBox>
        ) : (
          <div className="flex flex-col gap-4">
            {homekit.has_error && <InfoBox tone="destructive">{homekit.error}</InfoBox>}
            {homekit.has_pairing ? (
              <div className="flex flex-col items-center gap-4 sm:flex-row sm:items-start">
                {homekit.has_qr && (
                  <img
                    src={homekit.qr_data_url}
                    alt="HomeKit pairing QR code"
                    className="size-36 shrink-0 rounded-lg border border-border bg-white p-2"
                  />
                )}
                <div className="flex flex-col gap-1.5 text-center sm:text-left">
                  <span className="text-xs font-medium text-muted-foreground">Manual code</span>
                  <strong className="font-mono text-2xl font-semibold tracking-widest">
                    {homekit.pincode}
                  </strong>
                  <span className="font-mono text-xs break-all text-muted-foreground">{homekit.setup_uri}</span>
                </div>
              </div>
            ) : (
              <InfoBox tone="muted">HomeKit is starting. Check back in a few seconds.</InfoBox>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function InfoBox({
  tone,
  children,
}: {
  tone: "muted" | "success" | "destructive"
  children: ReactNode
}) {
  const toneClass =
    tone === "success"
      ? "border-success/30 bg-success/10 text-success"
      : tone === "destructive"
        ? "border-destructive/30 bg-destructive/10 text-destructive"
        : "border-border bg-muted/40 text-muted-foreground"
  return <div className={`rounded-lg border px-3 py-2.5 text-sm ${toneClass}`}>{children}</div>
}
