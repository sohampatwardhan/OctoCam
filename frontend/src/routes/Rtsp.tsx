import { useEffect, useState, type FormEvent } from "react"
import { useQuery } from "@tanstack/react-query"
import { Loader2, Radio } from "lucide-react"
import { apiGet, type RtspUrls } from "@/lib/api"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { Skeleton } from "@/components/ui/skeleton"
import { CopyButton } from "@/components/CopyButton"

export default function Rtsp() {
  const { data: settings, isLoading, isError } = useSettings()
  const updateSettings = useUpdateSettings()

  // Local, editable copy of the fields this form owns — seeded once from
  // the server so the user's in-progress edits survive the background
  // refetches useSettings/useUpdateSettings trigger (e.g. after Save).
  const [initialized, setInitialized] = useState(false)
  const [enabled, setEnabled] = useState(false)
  const [path, setPath] = useState("")
  const [maxClients, setMaxClients] = useState("1")

  useEffect(() => {
    if (settings && !initialized) {
      setEnabled(settings.rtsp_enabled)
      setPath(settings.rtsp_path)
      setMaxClients(String(settings.rtsp_max_clients))
      setInitialized(true)
    }
  }, [settings, initialized])

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    updateSettings.mutate({
      rtsp_enabled: enabled,
      rtsp_path: path,
      rtsp_max_clients: Number(maxClients) || 1,
    })
  }

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">RTSP</h1>

      {isError ? (
        <p className="text-sm text-muted-foreground">Device unreachable.</p>
      ) : (
        <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Settings</CardTitle>
              <CardDescription>Serve the camera's HD stream over RTSP for NVRs and media players.</CardDescription>
            </CardHeader>
            <CardContent>
              {isLoading || !initialized ? (
                <div className="flex flex-col gap-4">
                  <Skeleton className="h-5 w-full" />
                  <Skeleton className="h-8 w-full" />
                  <Skeleton className="h-8 w-full" />
                </div>
              ) : (
                <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
                  <label htmlFor="rtsp_enabled" className="flex items-center justify-between gap-3">
                    <span className="text-sm font-medium">Service enabled</span>
                    <Switch id="rtsp_enabled" checked={enabled} onCheckedChange={setEnabled} />
                  </label>

                  <div className="flex flex-col gap-1.5">
                    <Label htmlFor="rtsp_path">Path</Label>
                    <Input id="rtsp_path" value={path} maxLength={80} onChange={(e) => setPath(e.target.value)} />
                  </div>

                  <div className="flex flex-col gap-1.5">
                    <Label htmlFor="rtsp_max_clients">Max clients</Label>
                    <Input
                      id="rtsp_max_clients"
                      type="number"
                      min={1}
                      max={4}
                      value={maxClients}
                      onChange={(e) => setMaxClients(e.target.value)}
                    />
                  </div>

                  {updateSettings.isError && (
                    <p className="text-sm text-destructive">{updateSettings.error.message}</p>
                  )}
                  {updateSettings.isSuccess && !updateSettings.isPending && (
                    <p className="text-sm text-success">RTSP settings saved.</p>
                  )}

                  <Button type="submit" disabled={updateSettings.isPending} className="self-start">
                    {updateSettings.isPending && <Loader2 className="animate-spin" />}
                    Save RTSP settings
                  </Button>
                </form>
              )}
            </CardContent>
          </Card>

          {initialized && enabled && <RtspUrlsCard />}
        </div>
      )}
    </div>
  )
}

function RtspUrlsCard() {
  const { data, isLoading, isError } = useQuery({
    queryKey: ["rtsp"],
    queryFn: () => apiGet<RtspUrls>("/api/rtsp"),
  })

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Radio className="size-4 text-primary" aria-hidden="true" />
          Stream URLs
        </CardTitle>
        <CardDescription>Paste into VLC, an NVR, or any RTSP client.</CardDescription>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="flex flex-col gap-4">
            <Skeleton className="h-13 w-full" />
          </div>
        ) : isError || !data ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <div className="flex flex-col gap-4">
            <UrlRow label="HD" url={data.main} />
            {data.has_sub && <UrlRow label="SD" url={data.sub} />}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function UrlRow({ label, url }: { label: string; url: string }) {
  const id = `rtsp-url-${label}`
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
