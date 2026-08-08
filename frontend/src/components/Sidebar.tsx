import { NavLink } from "react-router-dom"
import type { LucideIcon } from "lucide-react"
import {
  IdCard,
  KeyRound,
  LayoutDashboard,
  House,
  Radio,
  ScrollText,
  Server,
  Settings,
  Share2,
  Shield,
  SlidersHorizontal,
  Wifi,
} from "lucide-react"
import { useMe } from "@/hooks/useAuth"
import { cn } from "@/lib/utils"

interface NavItem {
  label: string
  to: string
  icon: LucideIcon
  /** Only shown to admins. */
  adminOnly?: boolean
  /** Only shown to non-admins (e.g. account settings admins reach via /admin). */
  hideForAdmin?: boolean
  /** Real SPA route → react-router NavLink with active styling. Anything
   * else is a plain absolute link that leaves the SPA for the Askama pages. */
  inApp?: boolean
}

interface NavSection {
  title?: string
  items: NavItem[]
}

const NAV_SECTIONS: NavSection[] = [
  {
    items: [{ label: "Dashboard", to: "/", icon: LayoutDashboard, inApp: true }],
  },
  {
    title: "Basic Settings",
    items: [
      { label: "Identity", to: "/identity", icon: IdCard, adminOnly: true },
      { label: "Wi-Fi", to: "/wifi", icon: Wifi, adminOnly: true, inApp: true },
      { label: "Stream Config", to: "/stream-settings", icon: SlidersHorizontal, adminOnly: true },
      { label: "RTSP", to: "/rtsp", icon: Radio, adminOnly: true, inApp: true },
      { label: "HomeKit", to: "/homekit", icon: House, adminOnly: true, inApp: true },
      { label: "Matter", to: "/matter", icon: Share2, adminOnly: true },
    ],
  },
  {
    title: "Advanced Settings",
    items: [
      { label: "System info", to: "/system", icon: Server, adminOnly: true },
      { label: "System logs", to: "/logs", icon: ScrollText, adminOnly: true },
      { label: "SSH keys", to: "/ssh-keys", icon: KeyRound, adminOnly: true },
      { label: "Admin", to: "/admin", icon: Shield, adminOnly: true },
    ],
  },
  {
    items: [{ label: "Account Settings", to: "/settings", icon: Settings, hideForAdmin: true }],
  },
]

const navRowClass =
  "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"

function NavRow({ item, onNavigate }: { item: NavItem; onNavigate?: () => void }) {
  const Icon = item.icon

  if (item.inApp) {
    return (
      <NavLink
        to={item.to}
        end={item.to === "/"}
        onClick={onNavigate}
        className={({ isActive }) =>
          cn(navRowClass, isActive && "bg-muted text-foreground shadow-[inset_3px_0_0_var(--color-primary)]")
        }
      >
        <Icon className="size-4" aria-hidden="true" />
        {item.label}
      </NavLink>
    )
  }

  return (
    <a href={item.to} className={navRowClass}>
      <Icon className="size-4" aria-hidden="true" />
      {item.label}
    </a>
  )
}

function SidebarNav({ sections, onNavigate }: { sections: NavSection[]; onNavigate?: () => void }) {
  return (
    <nav className="flex flex-1 flex-col gap-4 overflow-y-auto px-3 py-4">
      {sections.map((section, index) => (
        <div key={section.title ?? index} className="flex flex-col gap-0.5">
          {section.title && (
            <p className="px-3 pb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase">
              {section.title}
            </p>
          )}
          {section.items.map((item) => (
            <NavRow key={item.to} item={item} onNavigate={onNavigate} />
          ))}
        </div>
      ))}
    </nav>
  )
}

interface SidebarProps {
  mobileOpen: boolean
  onClose: () => void
}

// Persistent settings nav. Rendered by AppShell on every route except `/`.
// Below `md` it collapses to an off-canvas drawer toggled from Topbar's
// hamburger; AppShell owns the open state and closes it on route change or
// on resize back to desktop.
export function Sidebar({ mobileOpen, onClose }: SidebarProps) {
  const { data: me } = useMe()
  const isAdmin = me?.is_admin ?? false

  const visibleSections = NAV_SECTIONS.map((section) => ({
    ...section,
    items: section.items.filter((item) => {
      if (item.adminOnly && !isAdmin) return false
      if (item.hideForAdmin && isAdmin) return false
      return true
    }),
  })).filter((section) => section.items.length > 0)

  return (
    <>
      <aside className="hidden md:sticky md:top-12 md:flex md:h-[calc(100vh-3rem)] md:w-full md:flex-col md:border-r md:border-border md:bg-sidebar">
        <SidebarNav sections={visibleSections} />
      </aside>

      <div
        className={cn("fixed inset-0 z-50 md:hidden", !mobileOpen && "pointer-events-none")}
        aria-hidden={!mobileOpen}
      >
        <div
          className={cn(
            "absolute inset-0 bg-black/50 transition-opacity duration-200",
            mobileOpen ? "opacity-100" : "opacity-0"
          )}
          onClick={onClose}
        />
        <aside
          className={cn(
            "absolute inset-y-0 left-0 flex w-[228px] max-w-[80vw] flex-col border-r border-border bg-sidebar shadow-xl transition-transform duration-200",
            mobileOpen ? "translate-x-0" : "-translate-x-full"
          )}
        >
          <SidebarNav sections={visibleSections} onNavigate={onClose} />
        </aside>
      </div>
    </>
  )
}
