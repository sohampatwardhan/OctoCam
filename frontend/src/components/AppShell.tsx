import { Outlet } from "react-router-dom"
import { Topbar } from "@/components/Topbar"

// Topbar-only, full-width shell for this slice — no sidebar. The first
// sidebar-bearing page (a later slice) grows this into a two-column layout.
export function AppShell() {
  return (
    <div className="flex min-h-screen flex-col bg-background">
      <Topbar />
      <main className="flex-1">
        <Outlet />
      </main>
    </div>
  )
}
