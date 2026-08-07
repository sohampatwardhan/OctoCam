import { useQuery } from "@tanstack/react-query"
import { apiGet, type Status } from "@/lib/api"

// `/api/status` — polled every 5s for the dashboard cards and the topbar
// live-status chip. No retries: an unreachable device is a real answer
// ("device unreachable"), not a transient blip worth masking with retries.
export function useStatus() {
  return useQuery({
    queryKey: ["status"],
    queryFn: () => apiGet<Status>("/api/status"),
    refetchInterval: 5000,
    retry: false,
  })
}
