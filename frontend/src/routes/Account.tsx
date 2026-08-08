import { useState, type FormEvent } from "react"
import { Check, Fingerprint, Loader2, Pencil, Trash2, X } from "lucide-react"
import { useUpdateSettings } from "@/hooks/useSettings"
import { useDeletePasskey, usePasskeys, useRegisterPasskey, useRenamePasskey } from "@/hooks/usePasskeys"
import { isWebAuthnSupported } from "@/lib/webauthn"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import type { PasskeyDto } from "@/lib/api"

// A cancelled/dismissed `navigator.credentials.create` prompt rejects with a
// DOMException named NotAllowedError (user declined/timed out) or
// AbortError (another ceremony superseded it) — that's not a real failure,
// just the user backing out, so it should stay silent rather than surface as
// an error. DOMException isn't nominally an Error subtype in TS's lib.dom
// types, but it's structurally compatible (has `name`/`message`), which is
// all `useMutation`'s default `Error`-typed error needs for this check.
function isCancelledCredentialError(error: unknown): boolean {
  return error instanceof DOMException && (error.name === "NotAllowedError" || error.name === "AbortError")
}

function PasswordCard() {
  const [password, setPassword] = useState("")
  const [confirm, setConfirm] = useState("")
  const [justSaved, setJustSaved] = useState(false)
  const updateSettings = useUpdateSettings()

  const mismatch = password.length > 0 && confirm.length > 0 && password !== confirm
  const canSubmit = password.length > 0 && password === confirm

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    if (!canSubmit) return
    setJustSaved(false)
    updateSettings.mutate(
      { admin_password: password, admin_password_confirm: confirm },
      {
        onSuccess: () => {
          setPassword("")
          setConfirm("")
          setJustSaved(true)
        },
      }
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Password</CardTitle>
        <CardDescription>Change the password you sign in with.</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-3" onSubmit={handleSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="admin_password">New password</Label>
            <Input
              id="admin_password"
              name="admin_password"
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(event) => {
                setPassword(event.target.value)
                setJustSaved(false)
              }}
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="admin_password_confirm">Confirm password</Label>
            <Input
              id="admin_password_confirm"
              name="admin_password_confirm"
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(event) => {
                setConfirm(event.target.value)
                setJustSaved(false)
              }}
            />
          </div>

          {mismatch && <p className="text-sm text-destructive">Passwords don't match.</p>}
          {updateSettings.isError && (
            <p className="text-sm text-destructive">{updateSettings.error.message}</p>
          )}
          {justSaved && !updateSettings.isError && (
            <p className="text-sm text-success">Password updated.</p>
          )}

          <div>
            <Button type="submit" disabled={!canSubmit || updateSettings.isPending}>
              {updateSettings.isPending && <Loader2 className="animate-spin" />}
              Update password
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  )
}

function formatLastUsed(lastUsedAt: string | null): string {
  return lastUsedAt ? `Last used ${lastUsedAt}` : "Never used"
}

function PasskeyRow({ passkey }: { passkey: PasskeyDto }) {
  const [renaming, setRenaming] = useState(false)
  const [nameDraft, setNameDraft] = useState(passkey.name)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const rename = useRenamePasskey()
  const deletePasskey = useDeletePasskey()

  function startRename() {
    setNameDraft(passkey.name)
    rename.reset()
    setRenaming(true)
  }

  function saveRename() {
    const trimmed = nameDraft.trim()
    if (!trimmed || trimmed === passkey.name) {
      setRenaming(false)
      return
    }
    rename.mutate({ id: passkey.id, name: trimmed }, { onSuccess: () => setRenaming(false) })
  }

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex flex-1 flex-col gap-1 overflow-hidden text-sm">
        {renaming ? (
          <div className="flex items-center gap-1.5">
            <Input
              autoFocus
              className="h-7 max-w-56"
              value={nameDraft}
              onChange={(event) => setNameDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") saveRename()
                if (event.key === "Escape") setRenaming(false)
              }}
            />
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              onClick={saveRename}
              disabled={rename.isPending}
              aria-label="Save name"
            >
              <Check />
            </Button>
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              onClick={() => setRenaming(false)}
              aria-label="Cancel rename"
            >
              <X />
            </Button>
          </div>
        ) : (
          <span className="font-medium">{passkey.name}</span>
        )}
        <span className="text-xs text-muted-foreground">
          Registered {passkey.created_at} &middot; {formatLastUsed(passkey.last_used_at)}
        </span>
        {rename.isError && <span className="text-xs text-destructive">{rename.error.message}</span>}
      </div>

      {!renaming && (
        <div className="flex gap-2">
          <Button type="button" variant="outline" size="sm" onClick={startRename}>
            <Pencil />
            Rename
          </Button>
          <Button type="button" variant="destructive" size="sm" onClick={() => setConfirmDelete(true)}>
            <Trash2 />
            Delete
          </Button>
        </div>
      )}

      <Dialog
        open={confirmDelete}
        onOpenChange={(open) => {
          if (!open) {
            setConfirmDelete(false)
            deletePasskey.reset()
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete "{passkey.name}"?</DialogTitle>
            <DialogDescription>
              You won't be able to sign in with this passkey anymore. This can't be undone.
            </DialogDescription>
          </DialogHeader>

          {deletePasskey.isError && (
            <p className="text-sm text-destructive">{deletePasskey.error.message}</p>
          )}

          <DialogFooter>
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button
              variant="destructive"
              disabled={deletePasskey.isPending}
              onClick={() =>
                deletePasskey.mutate(passkey.id, { onSuccess: () => setConfirmDelete(false) })
              }
            >
              {deletePasskey.isPending ? "Deleting…" : "Delete passkey"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function RegisterPasskeyDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [name, setName] = useState("")
  const register = useRegisterPasskey()

  function handleRegister() {
    const trimmed = name.trim() || "OctoCam Passkey"
    register.mutate(trimmed, {
      onSuccess: () => {
        setName("")
        onOpenChange(false)
      },
    })
  }

  const errorMessage =
    register.isError && !isCancelledCredentialError(register.error) ? register.error.message : null

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) register.reset()
        onOpenChange(next)
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Register a passkey</DialogTitle>
          <DialogDescription>
            Name it so you'll recognize it later, then follow your browser's prompt.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="passkey-name">Name</Label>
          <Input
            id="passkey-name"
            autoFocus
            placeholder="OctoCam Passkey"
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") handleRegister()
            }}
          />
        </div>

        {errorMessage && <p className="text-sm text-destructive">{errorMessage}</p>}

        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button onClick={handleRegister} disabled={register.isPending}>
            {register.isPending && <Loader2 className="animate-spin" />}
            Continue
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default function Account() {
  const { data: passkeys, isLoading, isError } = usePasskeys()
  const [registerOpen, setRegisterOpen] = useState(false)

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">Account</h1>

      <PasswordCard />

      <Card>
        <CardHeader>
          <CardTitle>Passkeys</CardTitle>
          <CardDescription>
            Sign in without a password using a device passkey — fingerprint, face, or security key.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {isLoading ? (
            <div className="flex flex-col gap-2">
              <Skeleton className="h-16 w-full rounded-lg" />
              <Skeleton className="h-16 w-full rounded-lg" />
            </div>
          ) : isError ? (
            <p className="text-sm text-destructive">Couldn't load your passkeys.</p>
          ) : !passkeys || passkeys.length === 0 ? (
            <p className="text-sm text-muted-foreground">No passkeys registered yet.</p>
          ) : (
            passkeys.map((passkey) => <PasskeyRow key={passkey.id} passkey={passkey} />)
          )}

          {isWebAuthnSupported() && (
            <div>
              <Button type="button" variant="outline" onClick={() => setRegisterOpen(true)}>
                <Fingerprint />
                Register a passkey
              </Button>
            </div>
          )}
        </CardContent>
      </Card>

      <RegisterPasskeyDialog open={registerOpen} onOpenChange={setRegisterOpen} />
    </div>
  )
}
