import { useEffect, useState } from "react"
import { Outlet, useLocation } from "react-router-dom"
import { Topbar } from "@/components/Topbar"
import { Sidebar } from "@/components/Sidebar"
import { UnsavedChangesProvider } from "@/components/UnsavedChangesProvider"

// Every route except the dashboard gets the persistent settings sidebar
// (two-column grid, ~228px + fluid content). The dashboard stays full-width,
// topbar-only, matching slice-1 behavior.
export function AppShell() {
  const { pathname } = useLocation()
  const withSidebar = pathname !== "/"
  const [mobileNavOpen, setMobileNavOpen] = useState(false)

  // Route change closes the mobile drawer (covers nav-link clicks).
  useEffect(() => {
    setMobileNavOpen(false)
  }, [pathname])

  // Growing back to desktop width should drop the (now hidden) drawer state
  // so it doesn't reopen mid-transition if the viewport shrinks again.
  useEffect(() => {
    const desktop = window.matchMedia("(min-width: 768px)")
    const handleChange = () => setMobileNavOpen(false)
    desktop.addEventListener("change", handleChange)
    return () => desktop.removeEventListener("change", handleChange)
  }, [])

  return (
    <UnsavedChangesProvider>
    <div className="flex min-h-screen flex-col bg-background">
      <Topbar
        showMenuButton={withSidebar}
        menuOpen={mobileNavOpen}
        onMenuClick={() => setMobileNavOpen((open) => !open)}
      />
      {withSidebar ? (
        <div className="grid flex-1 md:grid-cols-[228px_minmax(0,1fr)]">
          <Sidebar mobileOpen={mobileNavOpen} onClose={() => setMobileNavOpen(false)} />
          <main className="min-w-0 flex-1">
            <Outlet />
          </main>
        </div>
      ) : (
        <main className="flex-1">
          <Outlet />
        </main>
      )}
    </div>
    </UnsavedChangesProvider>
  )
}
