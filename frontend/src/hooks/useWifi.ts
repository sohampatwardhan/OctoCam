import { useMutation, useQuery } from "@tanstack/react-query"
import { apiDelete, apiGet, apiPost, type SavedWifiProfile, type WifiCache } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"

// `/api/wifi/saved` — profiles known to NetworkManager/wpa_supplicant/the
// DietPi autosetup file, flagged with `active`/`can_delete` server-side.
export function useSavedWifi() {
  return useQuery({
    queryKey: ["wifi", "saved"],
    queryFn: () => apiGet<SavedWifiProfile[]>("/api/wifi/saved"),
  })
}

// `/api/wifi/networks` — the last scan's cached results, shown immediately
// so the add-network dialog isn't empty before the user hits "Scan".
export function useWifiCache() {
  return useQuery({
    queryKey: ["wifi", "networks"],
    queryFn: () => apiGet<WifiCache>("/api/wifi/networks"),
  })
}

// `/api/wifi/scan` — triggers a live rescan (nmcli, falling back to iw) and
// writes the result back to the same cache `useWifiCache` reads, so the
// dialog's network list updates without a refetch.
export function useScanWifi() {
  return useMutation({
    mutationFn: () => apiPost<WifiCache>("/api/wifi/scan", {}),
    onSuccess: (cache) => {
      queryClient.setQueryData(["wifi", "networks"], cache)
    },
  })
}

interface ConnectWifiInput {
  ssid: string
  password: string
  security: string
}

interface WifiActionResult {
  success: boolean
  message: string
}

// `/api/wifi/connect` — throws with the server's `{error}` message on a
// non-2xx (e.g. a bad password), which the caller surfaces inline.
export function useConnectWifi() {
  return useMutation({
    mutationFn: (input: ConnectWifiInput) => apiPost<WifiActionResult>("/api/wifi/connect", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["wifi", "saved"] })
      void queryClient.invalidateQueries({ queryKey: ["status"] })
    },
  })
}

interface DeleteWifiInput {
  name: string
  source: string
}

// `/api/wifi/delete` — the backend 400s with "Cannot delete the currently
// connected network." if `name` matches the active SSID; the UI shouldn't
// offer delete for that row (see `can_delete`), but the guard is server-side
// too, so this can still throw that message.
export function useDeleteWifi() {
  return useMutation({
    mutationFn: (input: DeleteWifiInput) => apiDelete<WifiActionResult>("/api/wifi/delete", input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["wifi", "saved"] })
    },
  })
}
