import { useMutation, useQuery } from "@tanstack/react-query"
import { apiDelete, apiGet, apiPost, type UserDto } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"

// `/api/users` — user accounts / RBAC, admin-only. See api_users_list in
// rust/octocam-web/src/main.rs (~line 3051).
export function useUsers() {
  return useQuery({
    queryKey: ["users"],
    queryFn: () => apiGet<UserDto[]>("/api/users"),
  })
}

interface MutationResult {
  success: boolean
  error?: string
}

// `api_users_add`/`api_users_delete` (main.rs) both return HTTP 200 even on
// failure — the body is `{success:false, error}` rather than a non-2xx
// status, so `apiPost`/`apiDelete` resolve normally instead of throwing.
// Callers MUST check `.success` themselves and surface `.error`; this is the
// same 200-on-failure pattern the passkey endpoints use (see Account.tsx).
function throwIfFailed(result: MutationResult): MutationResult {
  if (!result.success) {
    throw new Error(friendlyUserError(result.error ?? "Something went wrong."))
  }
  return result
}

// Maps known server error strings to friendly copy. The duplicate-username
// case comes back as rusqlite's raw constraint-violation text (e.g.
// "UNIQUE constraint failed: users.username") rather than a coded error —
// api_users_add just forwards `err.to_string()` — so this matches on
// substring rather than an exact string/code.
export function friendlyUserError(message: string): string {
  if (message.includes("UNIQUE constraint failed")) {
    return "That username is already taken."
  }
  return message
}

interface AddUserInput {
  username: string
  password: string
  role: string
}

export function useAddUser() {
  return useMutation({
    mutationFn: (input: AddUserInput) =>
      apiPost<MutationResult>("/api/users/add", input).then(throwIfFailed),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["users"] })
    },
  })
}

export function useDeleteUser() {
  return useMutation({
    mutationFn: (id: number) =>
      apiDelete<MutationResult>(`/api/users/${id}`, {}).then(throwIfFailed),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["users"] })
    },
  })
}
