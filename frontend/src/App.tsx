import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query"
import { Activity } from "lucide-react"
import { apiGet, type Status } from "@/lib/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

const queryClient = new QueryClient()

function StatusCard() {
  const { data, isLoading, isError } = useQuery({
    queryKey: ["status"],
    queryFn: () => apiGet<Status>("/api/status"),
    refetchInterval: 5000,
  })

  return (
    <Card className="w-full max-w-md">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Activity className="size-5" /> OctoCam SPA
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        {isLoading && <p className="text-muted-foreground">Loading status…</p>}
        {isError && <Badge variant="destructive">device unreachable</Badge>}
        {data && (
          <>
            <div>Host: <Badge>{data.hostname}</Badge></div>
            <div>
              Camera:{" "}
              <Badge variant={data.camera.available ? "default" : "destructive"}>
                {data.camera.message}
              </Badge>
            </div>
            <div>Uptime: {data.uptime ?? "unknown"}</div>
            <div>
              Motion: <Badge>{data.motion_detected ? "detected" : "clear"}</Badge>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  )
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <main className="min-h-screen flex items-center justify-center p-6">
        <StatusCard />
      </main>
    </QueryClientProvider>
  )
}
