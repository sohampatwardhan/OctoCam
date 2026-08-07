import { StreamPreview } from "@/components/dashboard/StreamPreview"
import { StreamHealthCard } from "@/components/dashboard/StreamHealthCard"
import { ClientsCard } from "@/components/dashboard/ClientsCard"

export default function Dashboard() {
  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">Dashboard</h1>

      <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-[minmax(0,1fr)_360px]">
        <StreamPreview />
        <div className="flex flex-col gap-6">
          <StreamHealthCard />
          <ClientsCard />
        </div>
      </div>
    </div>
  )
}
