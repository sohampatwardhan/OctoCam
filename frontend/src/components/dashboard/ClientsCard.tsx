import { useState } from "react"
import { ChevronDown } from "lucide-react"
import { useStatus } from "@/hooks/useStatus"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import type { PathViewers } from "@/lib/api"

export function ClientsCard() {
  const { data, isLoading, isError } = useStatus()

  return (
    <Card>
      <CardHeader>
        <CardTitle>Clients</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {isLoading ? (
          <>
            <Skeleton className="h-10 w-full rounded-lg" />
            <Skeleton className="h-10 w-full rounded-lg" />
          </>
        ) : isError || !data ? (
          <p className="text-sm text-muted-foreground">Device unreachable.</p>
        ) : (
          <>
            <ClientRow label="HD Stream" viewers={data.viewers?.main ?? null} />
            <ClientRow label="SD Stream" viewers={data.viewers?.sub ?? null} />
          </>
        )}
      </CardContent>
    </Card>
  )
}

function ClientRow({ label, viewers }: { label: string; viewers: PathViewers | null }) {
  const [open, setOpen] = useState(false)

  if (!viewers) {
    return (
      <div className="flex items-center gap-3 rounded-lg border border-border px-3 py-2.5">
        <span className="flex-1 text-sm font-medium">{label}</span>
        <span className="text-xs text-muted-foreground">unavailable</span>
      </div>
    )
  }

  return (
    <div className="rounded-lg border border-border">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-3 px-3 py-2.5 text-left"
      >
        <ChevronDown
          className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", open && "rotate-180")}
          aria-hidden="true"
        />
        <span className="flex-1 text-sm font-medium">{label}</span>
        <Badge variant="secondary">
          {viewers.total} / {viewers.capacity}
        </Badge>
      </button>
      {open && (
        <div className="border-t border-border px-3 py-2">
          {viewers.clients.length === 0 ? (
            <p className="text-sm text-muted-foreground">No clients connected.</p>
          ) : (
            <ul className="flex flex-col gap-2.5">
              {viewers.clients.map((client, index) => (
                <li key={`${client.label}-${index}`} className="flex flex-col gap-0.5 text-sm">
                  <span className="font-medium">
                    {client.label}
                    <span className="ml-1.5 font-normal text-muted-foreground">{client.client_type}</span>
                  </span>
                  <span className="text-xs text-muted-foreground">{client.remote_addr}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}
