import { useMutation, useQuery } from "@tanstack/react-query"
import { apiGet, apiPut, type Settings } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"

// `/api/settings` (GET) — the settings pages' shared read model. See
// settings::public_settings in rust/octocam-web/src/settings.rs.
export function useSettings() {
  return useQuery({
    queryKey: ["settings"],
    queryFn: () => apiGet<Settings>("/api/settings"),
  })
}

interface UpdateSettingsResult {
  success: boolean
  settings: Settings
}

// `/api/settings` (PUT) — the server seeds a full settings map from the
// CURRENT saved settings, then overlays whatever's in the request body, so
// callers should send only the fields that changed (native booleans/numbers,
// not strings — see api_settings_update in main.rs). Refreshes both the
// settings cache and /api/status, since several settings (e.g. rtsp_enabled)
// affect status fields too.
export function useUpdateSettings() {
  return useMutation({
    mutationFn: (patch: Partial<Settings>) =>
      apiPut<UpdateSettingsResult>("/api/settings", patch),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["settings"] })
      void queryClient.invalidateQueries({ queryKey: ["status"] })
    },
  })
}
