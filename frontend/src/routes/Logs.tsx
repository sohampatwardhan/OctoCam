import { useQuery } from "@tanstack/react-query"
import { RefreshCw } from "lucide-react"
import { apiGet, type LogsResponse } from "@/lib/api"
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"

// `/api/logs` — a fixed 40-line journalctl snapshot (not a live tail),
// polled every 5s to mirror the legacy Askama page's behavior.
function useLogs() {
  return useQuery({
    queryKey: ["logs"],
    queryFn: () => apiGet<LogsResponse>("/api/logs"),
    refetchInterval: 5000,
  })
}

export default function Logs() {
  const { data, isLoading, isError, isFetching, refetch } = useLogs()

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">System logs</h1>

      <Card>
        <CardHeader>
          <CardTitle>Recent activity</CardTitle>
          <CardAction>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => refetch()}
              disabled={isFetching}
            >
              <RefreshCw className={isFetching ? "animate-spin" : undefined} aria-hidden="true" />
              Refresh
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <p className="text-xs text-muted-foreground">
            Auto-refreshes every 5s · latest 40 lines from the system journal
          </p>

          {isLoading ? (
            <div className="flex flex-col gap-2">
              {Array.from({ length: 10 }, (_, index) => (
                <Skeleton key={index} className="h-4 w-full" />
              ))}
            </div>
          ) : isError || !data ? (
            <p className="text-sm text-muted-foreground">Logs unavailable.</p>
          ) : data.lines.length === 0 ? (
            <p className="text-sm text-muted-foreground">No recent logs available.</p>
          ) : (
            <pre className="max-h-[60vh] overflow-auto rounded-lg bg-muted p-4 font-mono text-xs leading-relaxed text-foreground">
              {data.lines.join("\n")}
            </pre>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
