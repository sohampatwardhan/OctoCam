import { StatusCard } from "@/components/system/StatusCard"
import { MaintenanceCard } from "@/components/system/MaintenanceCard"
import { BackupRestoreCard } from "@/components/system/BackupRestoreCard"

export default function System() {
  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">System info</h1>

      <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-2">
        <StatusCard />
        <div className="flex flex-col gap-6">
          <MaintenanceCard />
          <BackupRestoreCard />
        </div>
      </div>
    </div>
  )
}
