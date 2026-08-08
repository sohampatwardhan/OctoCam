import { useMutation, useQuery } from "@tanstack/react-query"
import { apiDelete, apiGet, apiPost, type PasskeyDto } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"
import { registerPasskey } from "@/lib/webauthn"

const PASSKEYS_KEY = ["passkeys"]

// `/api/passkeys` — the caller's own passkeys. See PasskeyDto's doc comment
// (lib/api.ts) for why the type omits several server-returned fields.
export function usePasskeys() {
  return useQuery({
    queryKey: PASSKEYS_KEY,
    queryFn: () => apiGet<PasskeyDto[]>("/api/passkeys"),
  })
}

// Wraps webauthn.ts's registerPasskey (the /api/passkey/register/start+finish
// ceremony) as a mutation so Account.tsx gets isPending/isError for free.
export function useRegisterPasskey() {
  return useMutation({
    mutationFn: (name: string) => registerPasskey(name),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: PASSKEYS_KEY })
    },
  })
}

// `POST /api/passkey/{id}/rename` and `DELETE /api/passkey/{id}` both reply
// with HTTP 200 either way — failure is `{success:false, error}`, not a
// non-2xx status (see api_passkey_rename/api_passkey_delete in
// rust/octocam-web/src/main.rs). apiPost/apiDelete only throw on a non-2xx
// response, so both mutations below check `.success` themselves and throw,
// mirroring registerPasskey's finish-step handling in lib/webauthn.ts.
interface PasskeyActionResult {
  success: boolean
  error?: string
}

export function useRenamePasskey() {
  return useMutation({
    mutationFn: async ({ id, name }: { id: number; name: string }) => {
      const result = await apiPost<PasskeyActionResult>(`/api/passkey/${id}/rename`, { name })
      if (!result.success) throw new Error(result.error || "Failed to rename passkey.")
      return result
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: PASSKEYS_KEY })
    },
  })
}

export function useDeletePasskey() {
  return useMutation({
    mutationFn: async (id: number) => {
      const result = await apiDelete<PasskeyActionResult>(`/api/passkey/${id}`, {})
      if (!result.success) throw new Error(result.error || "Failed to delete passkey.")
      return result
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: PASSKEYS_KEY })
    },
  })
}
