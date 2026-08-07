import { useState } from "react"
import { CurrentConnectionCard } from "@/components/wifi/CurrentConnectionCard"
import { SavedNetworksCard } from "@/components/wifi/SavedNetworksCard"
import { AddNetworkDialog } from "@/components/wifi/AddNetworkDialog"

export default function Wifi() {
  const [notice, setNotice] = useState<string | null>(null)

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <div className="flex items-center justify-between gap-4">
        <h1 className="font-heading text-2xl font-semibold tracking-tight">Wi-Fi</h1>
        <AddNetworkDialog onConnected={setNotice} />
      </div>

      {notice && (
        <p className="rounded-lg border border-success/30 bg-success/10 px-3 py-2 text-sm text-success">
          {notice}
        </p>
      )}

      <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-2">
        <CurrentConnectionCard />
        <SavedNetworksCard />
      </div>
    </div>
  )
}
