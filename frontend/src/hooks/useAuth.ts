import { useQuery } from "@tanstack/react-query"
import { apiGet, type Me } from "@/lib/api"

// `/api/me` 401s when logged out — apiGet throws, so isError is the
// logged-out signal alongside `!data.authenticated`. No retries: a 401
// is a real answer, not a transient failure.
export function useMe() {
  return useQuery({
    queryKey: ["me"],
    queryFn: () => apiGet<Me>("/api/me"),
    retry: false,
  })
}
