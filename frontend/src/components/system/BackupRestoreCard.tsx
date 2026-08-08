import { useState } from "react"
import { useMutation } from "@tanstack/react-query"
import { Download, Loader2 } from "lucide-react"
import { apiUpload, type RestoreResult } from "@/lib/api"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"

const fileInputClass =
  "text-sm text-muted-foreground file:mr-3 file:h-8 file:rounded-lg file:border file:border-border file:bg-secondary file:px-2.5 file:text-sm file:font-medium file:text-secondary-foreground hover:file:bg-muted"

export function BackupRestoreCard() {
  const [file, setFile] = useState<File | null>(null)
  const [open, setOpen] = useState(false)

  const restore = useMutation({
    mutationFn: (file: File) => {
      const formData = new FormData()
      formData.append("backup", file)
      return apiUpload<RestoreResult>("/api/restore", formData)
    },
    onSuccess: () => setOpen(false),
  })

  function handleConfirm() {
    if (file) restore.mutate(file)
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Backup &amp; restore</CardTitle>
        <CardDescription>
          Restoring does not change the admin password, Wi-Fi credentials, or existing HomeKit/Matter pairings. SSH
          keys found in the backup are added to the existing set, not replaced.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <Button variant="outline" className="self-start" render={<a href="/backup" download />}>
          <Download />
          Download backup
        </Button>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="restore-file">Backup file</Label>
          <input
            id="restore-file"
            type="file"
            accept="application/json,.json"
            className={fileInputClass}
            onChange={(event) => {
              setFile(event.target.files?.[0] ?? null)
              restore.reset()
            }}
          />
        </div>

        <Dialog
          open={open}
          onOpenChange={(next) => {
            setOpen(next)
            if (!next) restore.reset()
          }}
        >
          <DialogTrigger render={<Button disabled={!file} className="self-start" />}>
            Restore from backup
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Restore configuration?</DialogTitle>
              <DialogDescription>
                This overwrites current config. Stream, image, and feature settings will be replaced with the
                backup's values.
              </DialogDescription>
            </DialogHeader>

            {restore.isError && <p className="text-sm text-destructive">{restore.error.message}</p>}

            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button type="button" onClick={handleConfirm} disabled={restore.isPending}>
                {restore.isPending && <Loader2 className="animate-spin" />}
                Restore
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {restore.isSuccess && restore.data && (
          <p className="text-sm text-success">
            Configuration restored. {restore.data.keys_added ?? 0} SSH key(s) added
            {restore.data.keys_failed ? `, ${restore.data.keys_failed} failed to write` : ""}.
          </p>
        )}
      </CardContent>
    </Card>
  )
}
