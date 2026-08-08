import { useMutation, useQuery } from "@tanstack/react-query"
import { apiDelete, apiGet, apiPost, type SshKeyDto } from "@/lib/api"
import { queryClient } from "@/lib/queryClient"

// `/api/ssh-keys` — root's authorized_keys, admin-only. See api_ssh_keys_list
// in rust/octocam-web/src/main.rs. A 503 (the service user couldn't read
// /root/.ssh/authorized_keys) surfaces as a plain fetch failure via `apiGet`
// (it doesn't parse the server's `{error}` body) — the route below shows a
// fixed "couldn't read authorized_keys" message on any `isError`, matching
// how SavedNetworksCard/Logs handle read failures elsewhere in the SPA.
export function useSshKeys() {
  return useQuery({
    queryKey: ["ssh-keys"],
    queryFn: () => apiGet<SshKeyDto[]>("/api/ssh-keys"),
  })
}

interface AddKeyResult {
  success: boolean
}

// The `code` strings `api_ssh_keys_add` can throw — see
// `ssh_keys::KeyError::code()` in rust/octocam-web/src/ssh_keys.rs. The
// server sends these as the *body* of the thrown message (main.rs's
// `api::ApiError::bad_request(e.code())` makes the code itself the
// `{error}` text, there's no separate `code` field on this route), so
// `useAddSshKey`'s thrown `Error.message` IS one of these five strings
// verbatim. Copy mirrors `ssh_key_message` in main.rs (~1256-1292) so the
// wording matches the legacy Askama page.
const ADD_KEY_ERROR_COPY: Record<string, string> = {
  bad_key:
    "That isn't a single valid public key. Paste one line like 'ssh-ed25519 AAAA… comment' — options and multi-line input aren't accepted.",
  too_long: "That key is too large to store.",
  duplicate: "That key is already authorized.",
  write_failed: "Couldn't update root's authorized_keys — check the service user's sudo access.",
  read_failed: "Couldn't read root's authorized_keys — check the service user's sudo access.",
}

// Maps a thrown add-key error message to friendly copy. Falls back to the
// raw message for anything unrecognized (defensive — the server's code list
// is a closed set today, but this keeps an unexpected code readable rather
// than silently swallowed).
export function friendlyAddKeyError(message: string): string {
  return ADD_KEY_ERROR_COPY[message] ?? message
}

export function useAddSshKey() {
  return useMutation({
    mutationFn: (public_key: string) => apiPost<AddKeyResult>("/api/ssh-keys", { public_key }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["ssh-keys"] })
    },
  })
}

interface RevokeInput {
  fingerprint: string
  confirm: boolean
}

interface RevokeResult {
  success: boolean
}

// `DELETE /api/ssh-keys` — revoking the LAST remaining key with
// `confirm:false` returns HTTP 409 with
// `{error: "This is the last key; resend with confirm=true to remove it"}`
// (`api::ApiError::conflict` in `api_ssh_keys_delete`, main.rs ~1483).
// `apiDelete`'s `throwOnError` only surfaces the message text, not the
// status code, and `lib/api.ts` has no `ApiHttpError` variant carrying
// status today. Rather than add one for a single call site, we detect the
// last-key case by matching that stable, server-authored message substring
// — it's not user input, so this is safe, and it keeps the change local to
// this hook. `isLastKeyError` is exported so `SshKeys.tsx` can trigger its
// confirm dialog without re-deriving the match.
export function isLastKeyError(message: string): boolean {
  return message.toLowerCase().includes("last key")
}

export function useRevokeSshKey() {
  return useMutation({
    mutationFn: ({ fingerprint, confirm }: RevokeInput) =>
      apiDelete<RevokeResult>("/api/ssh-keys", { fingerprint, confirm }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["ssh-keys"] })
    },
  })
}
