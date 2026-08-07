import { useState } from "react"
import { Loader2, Plus, RadioTower } from "lucide-react"
import { useConnectWifi, useScanWifi, useWifiCache } from "@/hooks/useWifi"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
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
import { passwordMeetsCriteria } from "@/lib/wifi"

const SECURITY_OPTIONS = ["open", "wep", "wpa2", "wpa2-wpa3", "wpa3"] as const

// Scanned networks report a coarse security type (open/wep/wpa/wpa2/wpa3/
// unknown, see normalize_security in wifi.rs); the security select only
// offers the visible set above, so fold anything else (plain "wpa",
// "unknown") into "wpa2" — same as visibleWifiSecurity in static/app.js.
function visibleSecurity(security: string): string {
  if (security === "open" || security === "wep" || security === "wpa2-wpa3" || security === "wpa3") {
    return security
  }
  return "wpa2"
}

const selectClass =
  "h-8 w-full rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 dark:bg-input/30"

export function AddNetworkDialog({ onConnected }: { onConnected: (message: string) => void }) {
  const [open, setOpen] = useState(false)
  const [selectedSsid, setSelectedSsid] = useState("")
  const [manualSsid, setManualSsid] = useState("")
  const [security, setSecurity] = useState<string>("wpa2")
  const [password, setPassword] = useState("")

  const { data: cache } = useWifiCache()
  const scanWifi = useScanWifi()
  const connectWifi = useConnectWifi()
  const networks = cache?.networks ?? []

  const ssid = manualSsid.trim() || selectedSsid

  function resetForm() {
    setSelectedSsid("")
    setManualSsid("")
    setSecurity("wpa2")
    setPassword("")
    connectWifi.reset()
  }

  function handleSelectNetwork(value: string) {
    setSelectedSsid(value)
    const network = networks.find((candidate) => candidate.ssid === value)
    if (network) setSecurity(visibleSecurity(network.security))
  }

  function handleSubmit() {
    connectWifi.mutate(
      { ssid, password: security === "open" ? "" : password, security },
      {
        onSuccess: (result) => {
          setOpen(false)
          resetForm()
          onConnected(result.message)
        },
      }
    )
  }

  const canSave = ssid.length > 0 && passwordMeetsCriteria(security, security === "open" ? "" : password)

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) resetForm()
      }}
    >
      <DialogTrigger render={<Button variant="outline" size="sm" />}>
        <Plus />
        Add network
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add a Wi-Fi network</DialogTitle>
          <DialogDescription>Scan for nearby networks, or enter a name manually.</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => scanWifi.mutate()}
              disabled={scanWifi.isPending}
            >
              {scanWifi.isPending ? <Loader2 className="animate-spin" /> : <RadioTower />}
              Scan
            </Button>
            {cache?.scanned_at && (
              <span className="text-xs text-muted-foreground">
                Last scanned {new Date(cache.scanned_at * 1000).toLocaleTimeString()}
              </span>
            )}
          </div>

          {scanWifi.isError && <p className="text-sm text-destructive">{scanWifi.error.message}</p>}

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="wifi-network-select">Nearby networks</Label>
            <select
              id="wifi-network-select"
              className={selectClass}
              value={selectedSsid}
              onChange={(event) => handleSelectNetwork(event.target.value)}
              disabled={networks.length === 0}
            >
              <option value="">
                {networks.length === 0 ? "No networks scanned yet" : "Choose a network…"}
              </option>
              {networks.map((network) => (
                <option key={network.ssid} value={network.ssid}>
                  {network.ssid} · {network.security.toUpperCase()} · {network.signal}%
                </option>
              ))}
            </select>
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="wifi-manual-ssid">Network name (SSID)</Label>
            <Input
              id="wifi-manual-ssid"
              placeholder="Enter manually to override the list above"
              value={manualSsid}
              onChange={(event) => setManualSsid(event.target.value)}
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="wifi-security">Security</Label>
            <select
              id="wifi-security"
              className={selectClass}
              value={security}
              onChange={(event) => setSecurity(event.target.value)}
            >
              {SECURITY_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {option.toUpperCase()}
                </option>
              ))}
            </select>
          </div>

          {security !== "open" && (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="wifi-password">Password</Label>
              <Input
                id="wifi-password"
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </div>
          )}

          {connectWifi.isError && <p className="text-sm text-destructive">{connectWifi.error.message}</p>}
        </div>

        <DialogFooter>
          <Button type="button" onClick={handleSubmit} disabled={!canSave || connectWifi.isPending}>
            {connectWifi.isPending && <Loader2 className="animate-spin" />}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
