import { Link, useNavigate } from "react-router-dom"
import { LogOut, Menu, Settings } from "lucide-react"
import { RaspberryPiIcon } from "@/components/icons/selfhst"
import { apiPost } from "@/lib/api"
import { useMe } from "@/hooks/useAuth"
import { useStatus } from "@/hooks/useStatus"
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

export function Topbar({ showMenuButton = false, menuOpen = false, onMenuClick }: TopbarProps = {}) {
  const navigate = useNavigate()
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

        {me?.is_admin && <PowerDialog />}

        <Button variant="ghost" size="icon" aria-label="Log out" title="Log out" onClick={handleLogout}>
          <LogOut />
        </Button>
      </div>
    </header>
  )
}
