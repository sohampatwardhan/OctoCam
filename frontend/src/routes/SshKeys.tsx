import { useState } from "react"
import { Loader2, Trash2 } from "lucide-react"
import { useStatus } from "@/hooks/useStatus"
import {
  friendlyAddKeyError,
  isLastKeyError,
  useAddSshKey,
  useRevokeSshKey,
  useSshKeys,
} from "@/hooks/useSshKeys"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
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
import type { SshKeyDto } from "@/lib/api"

const textareaClass =
  "min-h-24 w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-2 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 dark:bg-input/30"

const codeClass = "rounded bg-muted px-1 py-0.5 font-mono text-xs"

// Mirrors `ssh_target` in rust/octocam-web/src/system.rs (~line 162): the
// first reported IP address, falling back to `<hostname>.local` so the hint
// still resolves on the same LAN even before the device has an IP to show.
function useSshTarget(): string | null {
  const { data: status } = useStatus()
  if (!status) return null
  return status.ip_addresses[0] ?? `${status.hostname}.local`
}

export default function SshKeys() {
  const sshTarget = useSshTarget()
  const { data: keys, isLoading, isError } = useSshKeys()

  const [publicKey, setPublicKey] = useState("")
  const [addedNotice, setAddedNotice] = useState(false)
  const [pendingRevoke, setPendingRevoke] = useState<SshKeyDto | null>(null)

  const addKey = useAddSshKey()
  const revokeKey = useRevokeSshKey()

  function handleAdd() {
    setAddedNotice(false)
    addKey.mutate(publicKey, {
      onSuccess: () => {
        setPublicKey("")
        setAddedNotice(true)
      },
    })
  }

  // Normal revoke: fire immediately with confirm:false. If this is the last
  // remaining key, the server 409s (see isLastKeyError's doc comment) and we
  // open the confirm dialog instead of surfacing a bare error.
  function requestRevoke(key: SshKeyDto) {
    revokeKey.reset()
    revokeKey.mutate(
      { fingerprint: key.fingerprint, confirm: false },
      {
        onError: (error) => {
          if (isLastKeyError(error.message)) setPendingRevoke(key)
        },
      }
    )
  }

  function confirmRevoke() {
    if (!pendingRevoke) return
    revokeKey.mutate(
      { fingerprint: pendingRevoke.fingerprint, confirm: true },
      { onSuccess: () => setPendingRevoke(null) }
    )
  }

  const revokeError =
    revokeKey.isError && !isLastKeyError(revokeKey.error.message) ? revokeKey.error.message : null

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">SSH keys</h1>

      <Card>
        <CardHeader>
          <CardTitle>Root SSH keys</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">
            These public keys can log in to this device over SSH as <code className={codeClass}>root</code>
            {sshTarget && (
              <>
                {" "}
                (<code className={codeClass}>ssh root@{sshTarget}</code>)
              </>
            )}
            . Revoke any you don't recognize.
          </p>

          {isLoading ? (
            <div className="flex flex-col gap-2">
              <Skeleton className="h-20 w-full rounded-lg" />
              <Skeleton className="h-20 w-full rounded-lg" />
            </div>
          ) : isError ? (
            <p className="text-sm text-destructive">
              Couldn't read authorized_keys. This usually means the OctoCam service user lacks
              passwordless sudo — check the system logs.
            </p>
          ) : !keys || keys.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No SSH keys are authorized for root. Add one below to enable SSH access.
            </p>
          ) : (
            keys.map((key) => (
              <div
                key={key.fingerprint}
                className="flex flex-col gap-2 rounded-lg border border-border px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="flex flex-1 flex-col gap-1 overflow-hidden text-sm">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">{key.key_type}</span>
                    <span className="text-muted-foreground">{key.comment || "(none)"}</span>
                  </div>
                  <span className="truncate font-mono text-xs text-muted-foreground">
                    {key.fingerprint}
                  </span>
                  <span className="truncate font-mono text-xs text-muted-foreground">{key.preview}</span>
                </div>
                <Button
                  type="button"
                  variant="destructive"
                  size="sm"
                  onClick={() => requestRevoke(key)}
                  disabled={revokeKey.isPending}
                >
                  <Trash2 />
                  Revoke
                </Button>
              </div>
            ))
          )}

          {revokeError && <p className="text-sm text-destructive">{revokeError}</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Authorize a new key</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">
            Paste a single public key line, e.g. the contents of{" "}
            <code className={codeClass}>~/.ssh/id_ed25519.pub</code>.
          </p>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="ssh-public-key">Public key</Label>
            <textarea
              id="ssh-public-key"
              className={textareaClass}
              rows={3}
              placeholder="ssh-ed25519 AAAA… user@host"
              value={publicKey}
              onChange={(event) => {
                setPublicKey(event.target.value)
                setAddedNotice(false)
              }}
            />
          </div>

          {addKey.isError && (
            <p className="text-sm text-destructive">{friendlyAddKeyError(addKey.error.message)}</p>
          )}
          {addedNotice && !addKey.isError && (
            <p className="text-sm text-success">SSH key authorized.</p>
          )}

          <div>
            <Button
              type="button"
              onClick={handleAdd}
              disabled={publicKey.trim().length === 0 || addKey.isPending}
            >
              {addKey.isPending && <Loader2 className="animate-spin" />}
              Authorize key
            </Button>
          </div>
        </CardContent>
      </Card>

      <Dialog
        open={pendingRevoke !== null}
        onOpenChange={(open) => {
          if (!open) {
            setPendingRevoke(null)
            revokeKey.reset()
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove your last SSH key?</DialogTitle>
            <DialogDescription>
              This is the only key authorized for root SSH. Removing it ends remote SSH access to
              this device — you won't be able to SSH in as root again until you add another key
              (e.g. via the console or a reflash).
            </DialogDescription>
          </DialogHeader>

          <DialogFooter>
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button variant="destructive" onClick={confirmRevoke} disabled={revokeKey.isPending}>
              {revokeKey.isPending ? "Removing…" : "Yes, remove it and end root SSH"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
