import { useEffect, useState, type FormEvent } from "react"
import { Loader2 } from "lucide-react"
import { useSettings, useUpdateSettings } from "@/hooks/useSettings"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"

export default function Identity() {
  const { data: settings, isLoading, isError } = useSettings()
  const updateSettings = useUpdateSettings()

  // Local, editable copy seeded once from the server so in-progress edits
  // survive the background refetches useSettings/useUpdateSettings trigger.
  const [initialized, setInitialized] = useState(false)
  const [deviceName, setDeviceName] = useState("")
  const [room, setRoom] = useState("")
  const [cameraLabel, setCameraLabel] = useState("")

  useEffect(() => {
    if (settings && !initialized) {
      setDeviceName(settings.device_name)
      setRoom(settings.room)
      setCameraLabel(settings.camera_label)
      setInitialized(true)
    }
  }, [settings, initialized])

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    updateSettings.mutate({
      device_name: deviceName,
      room,
      camera_label: cameraLabel,
    })
  }

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">Identity</h1>

      {isError ? (
        <p className="text-sm text-muted-foreground">Device unreachable.</p>
      ) : (
        <Card className="max-w-xl">
          <CardHeader>
            <CardTitle>Device identity</CardTitle>
            <CardDescription>How this camera identifies itself to you and to other apps.</CardDescription>
          </CardHeader>
          <CardContent>
            {isLoading || !initialized ? (
              <div className="flex flex-col gap-4">
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
              </div>
            ) : (
              <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="device_name">Device name</Label>
                  <Input
                    id="device_name"
                    value={deviceName}
                    maxLength={80}
                    onChange={(e) => setDeviceName(e.target.value)}
                  />
                </div>

                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="room">Room</Label>
                  <Input id="room" value={room} maxLength={80} onChange={(e) => setRoom(e.target.value)} />
                </div>

                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="camera_label">Camera label</Label>
                  <Input
                    id="camera_label"
                    value={cameraLabel}
                    maxLength={80}
                    onChange={(e) => setCameraLabel(e.target.value)}
                  />
                </div>

                {updateSettings.isError && (
                  <p className="text-sm text-destructive">{updateSettings.error.message}</p>
                )}
                {updateSettings.isSuccess && !updateSettings.isPending && (
                  <p className="text-sm text-success">Identity saved.</p>
                )}

                <Button type="submit" disabled={updateSettings.isPending} className="self-start">
                  {updateSettings.isPending && <Loader2 className="animate-spin" />}
                  Save identity
                </Button>
              </form>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  )
}
