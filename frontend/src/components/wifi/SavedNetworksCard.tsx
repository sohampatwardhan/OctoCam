import { useState } from "react"
import { Trash2 } from "lucide-react"
import { useDeleteWifi, useSavedWifi } from "@/hooks/useWifi"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
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
import type { SavedWifiProfile } from "@/lib/api"

export function SavedNetworksCard() {
  const { data, isLoading, isError } = useSavedWifi()
  const [pending, setPending] = useState<SavedWifiProfile | null>(null)
  const deleteWifi = useDeleteWifi()

  function requestDelete(profile: SavedWifiProfile) {
    deleteWifi.reset()
    setPending(profile)
  }

  function confirmDelete() {
    if (!pending) return
    deleteWifi.mutate(
      { name: pending.name, source: pending.delete_source },
      { onSuccess: () => setPending(null) }
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Saved networks</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {isLoading ? (
          <>
            <Skeleton className="h-11 w-full rounded-lg" />
            <Skeleton className="h-11 w-full rounded-lg" />
          </>
        ) : isError || !data ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : data.length === 0 ? (
          <p className="text-sm text-muted-foreground">No saved networks yet.</p>
        ) : (
          data.map((profile) => (
            <div
              key={`${profile.source}:${profile.name}`}
              className="flex items-center gap-3 rounded-lg border border-border px-3 py-2.5"
            >
              <div className="flex flex-1 flex-col gap-0.5 overflow-hidden">
                <span className="truncate text-sm font-medium">{profile.name}</span>
                <span className="text-xs text-muted-foreground">
                  {profile.security} · {profile.source}
                </span>
              </div>
              {profile.active ? (
                <Badge variant="secondary">Connected</Badge>
              ) : (
                profile.can_delete && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Delete ${profile.name}`}
                    onClick={() => requestDelete(profile)}
                  >
                    <Trash2 />
                  </Button>
                )
              )}
            </div>
          ))
        )}
      </CardContent>

      <Dialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open) setPending(null)
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete {pending?.name}?</DialogTitle>
            <DialogDescription>
              This removes the saved profile from {pending?.source}. You'll need to reconnect
              manually to use this network again.
            </DialogDescription>
          </DialogHeader>

          {deleteWifi.isError && (
            <p className="text-sm text-destructive">{deleteWifi.error.message}</p>
          )}

          <DialogFooter>
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button variant="destructive" onClick={confirmDelete} disabled={deleteWifi.isPending}>
              {deleteWifi.isPending ? "Deleting…" : "Delete"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  )
}
