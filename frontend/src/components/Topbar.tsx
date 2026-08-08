import { Link, useLocation, useNavigate } from "react-router-dom"
import { LogOut, Menu, Save, Settings } from "lucide-react"
import { RaspberryPiIcon } from "@/components/icons/selfhst"
import { apiPost } from "@/lib/api"
import { useMe } from "@/hooks/useAuth"
import { useStatus } from "@/hooks/useStatus"
import { useUnsavedChanges } from "@/hooks/useUnsavedChanges"
import { queryClient } from "@/lib/queryClient"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { PowerDialog } from "@/components/PowerDialog"

interface TopbarProps {
  /** Shown only on routes that render the Sidebar (i.e. not "/"). */
  showMenuButton?: boolean
  menuOpen?: boolean
  onMenuClick?: () => void
}

// Points at unsaved work without touching it. Absent entirely when every form
// on the page is clean, so the topbar stays quiet until it has something to say.
function UnsavedChangesIndicator() {
  const { sections } = useUnsavedChanges()
  if (sections.length === 0) return null

  const summary =
    sections.length === 1
      ? `Unsaved changes in ${sections[0].label}`
      : `Unsaved changes in ${sections.length} sections`

  return (
    <Button
      variant="ghost"
      size="icon"
      aria-label={summary}
      title={`${summary} — jump to them`}
      className="relative text-primary hover:text-primary"
      onClick={() => {
        const target = document.getElementById(sections[0].anchorId)
        target?.scrollIntoView({ behavior: "smooth", block: "center" })
      }}
    >
      <Save />
      <span
        aria-hidden="true"
        className="absolute top-1 right-1 size-1.5 rounded-full bg-primary ring-2 ring-background"
      />
    </Button>
  )
}

export function Topbar({ showMenuButton = false, menuOpen = false, onMenuClick }: TopbarProps = {}) {
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const onSettingsPage = pathname.startsWith("/settings")
  const { data: me } = useMe()
  // Shared with the dashboard's useStatus() (same query key) — no duplicate
  // fetch. Degrades to nothing if /api/status is unreachable rather than
  // blocking the shell.
  const { data: status } = useStatus()

  const rtspState = status?.services.rtsp.state
  // systemd's real "live" state is "active" (see system.rs ServiceStatus.state)
  // — "running"/"connected" aren't states rtsp's unit ever reports.
  const isLive = rtspState === "active"

  async function handleLogout() {
    try {
      await apiPost("/api/logout", {})
    } finally {
      queryClient.clear()
      navigate("/login")
    }
  }

  return (
    <header className="sticky top-0 z-10 flex h-12 items-center gap-3 border-b border-border bg-background/95 px-4 backdrop-blur">
      {showMenuButton && (
        <Button
          variant="ghost"
          size="icon"
          aria-label="Toggle navigation"
          aria-expanded={menuOpen}
          className="md:hidden"
          onClick={onMenuClick}
        >
          <Menu />
        </Button>
      )}

      <Link to="/" className="flex items-center gap-2 font-heading text-sm font-semibold tracking-tight">
        <RaspberryPiIcon className="size-4 text-primary" />
        OctoCam
      </Link>

      <div className="ml-auto flex items-center gap-2">
        {rtspState && (
          <Badge variant={isLive ? "default" : "secondary"} className="gap-1.5">
            <span
              className={`size-1.5 rounded-full ${isLive ? "bg-primary-foreground" : "bg-muted-foreground"}`}
              aria-hidden="true"
            />
            {isLive ? "Live" : rtspState}
          </Badge>
        )}

        {/* Settings pages own their own per-form saves, so the shell only
            reports that unsaved work exists and jumps to it — it never
            submits. Elsewhere the cog stays put, which also keeps a
            non-admin's only topbar route to their account page. */}
        {onSettingsPage ? (
          <UnsavedChangesIndicator />
        ) : (
          <Button
            variant="ghost"
            size="icon"
            nativeButton={false}
            aria-label="Settings"
            title="Settings"
            render={<a href="/settings/account" />}
          >
            <Settings />
          </Button>
        )}

        {me?.is_admin && <PowerDialog />}

        <Button variant="ghost" size="icon" aria-label="Log out" title="Log out" onClick={handleLogout}>
          <LogOut />
        </Button>
      </div>
    </header>
  )
}
