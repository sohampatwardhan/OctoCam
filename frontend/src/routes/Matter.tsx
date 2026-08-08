import { type ReactNode, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { Loader2, RotateCcw, Share2 } from "lucide-react"
import { apiGet, apiPost, type MatterInfo } from "@/lib/api"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import { queryClient } from "@/lib/queryClient"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Switch } from "@/components/ui/switch"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"

export default function Matter() {
  const { data: settings, isLoading: settingsLoading, isError: settingsError } = useSettings()
  const updateSettings = useUpdateSettings()
  const {
    data: matter,
    isLoading: matterLoading,
    isError: matterError,
  } = useQuery({
    queryKey: ["matter"],
    queryFn: () => apiGet<MatterInfo>("/api/matter"),
  })

  const enabled = settings?.matter_enabled ?? false
  const adminPasswordSet = matter?.admin_password_set ?? true
  const bridgeLoading = settingsLoading || matterLoading

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">Matter</h1>

      {settingsError ? (
        <p className="text-sm text-muted-foreground">Device unreachable.</p>
      ) : (
        <div className="flex flex-col gap-6">
          {!matterLoading && matter && !adminPasswordSet && (
            <InfoBox tone="destructive">
              Set an admin password first (Admin page). The Matter pairing code is a durable credential for this
              camera's feed, and without a password anyone on your network could read it.
            </InfoBox>
          )}

          <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-2">
            <Card>
              <CardHeader>
                <CardTitle>Bridge</CardTitle>
                <CardDescription>Publish this camera to Matter ecosystems (SmartThings, Home Assistant).</CardDescription>
              </CardHeader>
              <CardContent>
                {settingsLoading ? (
                  <div className="flex flex-col gap-4">
                    <Skeleton className="h-5 w-full" />
                    <Skeleton className="h-8 w-full" />
                  </div>
                ) : (
                  <div className="flex flex-col gap-4">
                    <label htmlFor="matter_enabled" className="flex items-center justify-between gap-3">
                      <span className="text-sm font-medium">Matter camera enabled</span>
                      <span className="flex items-center gap-2">
                        {updateSettings.isPending && (
                          <Loader2 className="size-3.5 animate-spin text-muted-foreground" aria-hidden="true" />
                        )}
                        <Switch
                          id="matter_enabled"
                          checked={enabled}
                          disabled={updateSettings.isPending || !adminPasswordSet}
                          onCheckedChange={(checked) =>
                            updateSettings.mutate(
                              { matter_enabled: checked },
                              { onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["matter"] }) }
                            )
                          }
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
                        <Metric label="Accessory" value={matter?.status ?? "—"} />
                        <Metric
                          label="Commissioned"
                          value={
                            matter?.commissioned
                              ? `yes (${matter.fabric_count} fabric${matter.fabric_count !== 1 ? "s" : ""})`
                              : "not commissioned"
                          }
                        />
                        <Metric label="Stream source" value={matter?.stream_source ?? "—"} />
                      </dl>
                    )}

                    {matter?.has_error && <InfoBox tone="destructive">{matter.error}</InfoBox>}
                    {matter && !matter.ipv6_ok && (
                      <InfoBox tone="destructive">
                        IPv6 appears disabled on this device. Matter requires IPv6 (link-local at minimum) —
                        commissioning will fail until it is re-enabled.
                      </InfoBox>
                    )}
                    {matter?.snapshot_endpoint_down && (
                      <InfoBox tone="destructive">
                        The internal snapshot endpoint failed to start; Matter controllers will not receive
                        snapshots until OctoCam restarts. Check the system logs.
                      </InfoBox>
                    )}
                    {matter?.orphaned_fabrics && (
                      <InfoBox tone="destructive">
                        Matter is disabled but {matter.fabric_count} previously paired ecosystem(s) still hold
                        credentials. Re-enabling restores their access to the camera; use "Reset Matter pairing" to
                        revoke.
                      </InfoBox>
                    )}
                  </div>
                )}
              </CardContent>
            </Card>

            <PairingCard
              enabled={enabled}
              settingsLoading={settingsLoading}
              matter={matter}
              isLoading={matterLoading}
              isError={matterError}
            />
          </div>

          <ResetCard enabled={enabled} matterLoading={matterLoading} />
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
  matter,
  isLoading,
  isError,
}: {
  enabled: boolean
  settingsLoading: boolean
  matter: MatterInfo | undefined
  isLoading: boolean
  isError: boolean
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Share2 className="size-4 text-primary" aria-hidden="true" />
          Pairing
        </CardTitle>
        <CardDescription>Add this camera to a Matter ecosystem, such as SmartThings.</CardDescription>
      </CardHeader>
      <CardContent>
        {settingsLoading || isLoading ? (
          <div className="flex flex-col gap-4">
            <Skeleton className="h-24 w-full" />
          </div>
        ) : !enabled ? (
          <InfoBox tone="muted">
            Enable Matter and it will be published to Matter ecosystems (SmartThings, Home Assistant, and others as
            support rolls out).
          </InfoBox>
        ) : isError || !matter ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <div className="flex flex-col gap-4">
            {matter.manual_code.length > 0 ? (
              <div className="flex flex-col items-center gap-4 sm:flex-row sm:items-start">
                <div
                  className="size-36 shrink-0 rounded-lg border border-border bg-white p-2 [&_svg]:size-full"
                  dangerouslySetInnerHTML={{ __html: matter.qr_svg }}
                />
                <div className="flex flex-col gap-1.5 text-center sm:text-left">
                  <span className="text-xs font-medium text-muted-foreground">Manual code</span>
                  <strong className="font-mono text-2xl font-semibold tracking-widest">{matter.manual_code}</strong>
                  <span className="font-mono text-xs break-all text-muted-foreground">{matter.qr_payload}</span>
                </div>
              </div>
            ) : (
              <InfoBox tone="muted">Matter is starting. Check back in a few seconds.</InfoBox>
            )}

            <InfoBox tone="muted">
              Ecosystem support (July 2026): SmartThings works. Home Assistant is experimental and blocks
              uncertified devices by default (this camera uses a test vendor ID) — a manual override is required.
              Alexa commissions but cannot show video yet. Google Home and Apple Home do not support Matter cameras
              yet.
            </InfoBox>
            <InfoBox tone="muted">
              Disabling Matter stops the service but does not revoke access: previously paired ecosystems regain
              the camera feed when re-enabled. Use "Reset Matter pairing" to revoke all pairings.
            </InfoBox>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function ResetCard({ enabled, matterLoading }: { enabled: boolean; matterLoading: boolean }) {
  const [open, setOpen] = useState(false)
  const [resetSuccess, setResetSuccess] = useState(false)
  const [resetError, setResetError] = useState<string | null>(null)
  const [resetting, setResetting] = useState(false)

  async function handleReset() {
    setResetting(true)
    setResetError(null)
    try {
      await apiPost("/api/matter/reset", {})
      setOpen(false)
      setResetSuccess(true)
      void queryClient.invalidateQueries({ queryKey: ["matter"] })
    } catch (error) {
      setResetError(error instanceof Error ? error.message : "Reset failed.")
    } finally {
      setResetting(false)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Reset pairing</CardTitle>
        <CardDescription>Removes all paired ecosystems and rotates the pairing code.</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-3">
          {resetSuccess && <InfoBox tone="success">Matter pairing reset. The code above has been rotated.</InfoBox>}
          {resetError && <InfoBox tone="destructive">{resetError}</InfoBox>}

          <Dialog
            open={open}
            onOpenChange={(next) => {
              setOpen(next)
              if (next) setResetError(null)
            }}
          >
            <DialogTrigger render={<Button variant="destructive" className="self-start" disabled={matterLoading} />}>
              <RotateCcw />
              Reset Matter pairing
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Reset Matter pairing?</DialogTitle>
                <DialogDescription>This unpairs Matter and rotates the code.</DialogDescription>
              </DialogHeader>
              {resetError && <p className="text-sm text-destructive">{resetError}</p>}
              <DialogFooter>
                <Button type="button" variant="destructive" onClick={handleReset} disabled={resetting}>
                  {resetting && <Loader2 className="animate-spin" />}
                  Reset pairing
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {!enabled && (
            <p className="text-xs text-muted-foreground">
              Matter is disabled, but a reset still revokes any previously paired ecosystems.
            </p>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

function InfoBox({ tone, children }: { tone: "muted" | "success" | "destructive"; children: ReactNode }) {
  const toneClass =
    tone === "success"
      ? "border-success/30 bg-success/10 text-success"
      : tone === "destructive"
        ? "border-destructive/30 bg-destructive/10 text-destructive"
        : "border-border bg-muted/40 text-muted-foreground"
  return <div className={`rounded-lg border px-3 py-2.5 text-sm ${toneClass}`}>{children}</div>
}
