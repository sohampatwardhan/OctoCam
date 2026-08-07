import { useEffect, useRef, useState } from "react"
import { useMutation } from "@tanstack/react-query"
import { CheckCircle2, Loader2, Power, PowerOff, RotateCw, XCircle } from "lucide-react"
import { apiPost } from "@/lib/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"

type PowerAction = "restart_service" | "restart_device" | "shutdown_device"

const ACTIONS: {
  action: PowerAction
  label: string
  description: string
  icon: typeof RotateCw
  danger?: boolean
}[] = [
  {
    action: "restart_service",
    label: "Restart OctoCam service",
    description: "Restarts the OctoCam web server and service process.",
    icon: RotateCw,
  },
  {
    action: "restart_device",
    label: "Restart device",
    description: "Reboots the Raspberry Pi.",
    icon: Power,
  },
  {
    action: "shutdown_device",
    label: "Shut down device",
    description: "Requires unplugging and re-plugging power to turn it back on.",
    icon: PowerOff,
    danger: true,
  },
]

interface PowerResponse {
  success: boolean
  scheduled?: string
}

// Admin-only power controls. Each action needs a confirming second click
// (arms for 4s, then auto-disarms) before it fires — these are disruptive to
// undoable, so a single accidental click shouldn't be enough.
export function PowerDialog() {
  const [open, setOpen] = useState(false)
  const [armed, setArmed] = useState<PowerAction | null>(null)
  const [result, setResult] = useState<{ action: PowerAction; ok: boolean; message: string } | null>(null)
  const armTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    return () => {
      if (armTimer.current) clearTimeout(armTimer.current)
    }
  }, [])

  const mutation = useMutation({
    mutationFn: (action: PowerAction) => apiPost<PowerResponse>("/api/power", { action }),
    onSuccess: (_data, action) => {
      setResult({ action, ok: true, message: `${labelFor(action)} scheduled.` })
    },
    onError: (error: Error, action) => {
      setResult({ action, ok: false, message: error.message || "Request failed." })
    },
  })

  function labelFor(action: PowerAction) {
    return ACTIONS.find((a) => a.action === action)?.label ?? action
  }

  function handleClick(action: PowerAction) {
    if (armed !== action) {
      setArmed(action)
      setResult(null)
      if (armTimer.current) clearTimeout(armTimer.current)
      armTimer.current = setTimeout(() => setArmed(null), 4000)
      return
    }
    if (armTimer.current) clearTimeout(armTimer.current)
    setArmed(null)
    mutation.mutate(action)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) {
          setArmed(null)
          setResult(null)
        }
      }}
    >
      <DialogTrigger
        render={
          <Button variant="ghost" size="icon" aria-label="Power options" title="Power options" />
        }
      >
        <Power />
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Power options</DialogTitle>
          <DialogDescription>Click an action once to arm it, then click again to confirm.</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-2">
          {ACTIONS.map(({ action, label, description, icon: Icon, danger }) => {
            const isArmed = armed === action
            const isPending = mutation.isPending && mutation.variables === action
            return (
              <button
                key={action}
                type="button"
                disabled={mutation.isPending}
                onClick={() => handleClick(action)}
                className={`flex items-start gap-3 rounded-lg border border-border p-3 text-left transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50 ${
                  isArmed ? (danger ? "border-destructive bg-destructive/10" : "border-primary bg-accent") : ""
                }`}
              >
                <span
                  className={`mt-0.5 ${danger ? "text-destructive" : "text-muted-foreground"} ${isArmed ? "text-foreground" : ""}`}
                >
                  {isPending ? <Loader2 className="size-4 animate-spin" /> : <Icon className="size-4" />}
                </span>
                <span className="flex flex-col gap-0.5">
                  <span className="text-sm font-medium">
                    {isArmed ? `Confirm: ${label}?` : label}
                  </span>
                  <span className="text-xs text-muted-foreground">{description}</span>
                </span>
              </button>
            )
          })}
        </div>

        {result && (
          <p
            className={`flex items-center gap-1.5 text-sm ${result.ok ? "text-foreground" : "text-destructive"}`}
          >
            {result.ok ? <CheckCircle2 className="size-4" /> : <XCircle className="size-4" />}
            {result.message}
          </p>
        )}

        <DialogFooter showCloseButton />
      </DialogContent>
    </Dialog>
  )
}
