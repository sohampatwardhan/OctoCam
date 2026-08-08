import type { ComponentType, SVGProps } from "react"
import { NavLink } from "react-router-dom"
import {
  IdCard,
  KeyRound,
  LayoutDashboard,
  ScrollText,
  Server,
  Settings,
  Shield,
  SlidersHorizontal,
  Wifi,
} from "lucide-react"
import { AppleHomeKitIcon, MatterIcon } from "@/components/icons/selfhst"
import { useMe } from "@/hooks/useAuth"
import { isAdminOnlySettingsPath } from "@/lib/nav"
import { cn } from "@/lib/utils"

// lucide icons and our vendored selfhst glyphs both satisfy this.
type NavIcon = ComponentType<SVGProps<SVGSVGElement>>

interface NavItem {
  label: string
  to: string
  icon: NavIcon
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
      { label: "Identity", to: "/settings/identity", icon: IdCard, inApp: true },
      { label: "Wi-Fi", to: "/settings/wifi", icon: Wifi, inApp: true },
      { label: "Stream Config", to: "/settings/stream", icon: SlidersHorizontal, inApp: true },
      { label: "HomeKit", to: "/settings/homekit", icon: AppleHomeKitIcon, inApp: true },
      { label: "Matter", to: "/settings/matter", icon: MatterIcon, inApp: true },
    ],
  },
  {
    title: "Advanced Settings",
    items: [
      { label: "System info", to: "/settings/system", icon: Server, inApp: true },
      { label: "System logs", to: "/settings/logs", icon: ScrollText, inApp: true },
      { label: "SSH keys", to: "/settings/ssh-keys", icon: KeyRound, inApp: true },
      { label: "Admin", to: "/settings/admin", icon: Shield, inApp: true },
    ],
  },
  {
    items: [{ label: "Account Settings", to: "/settings/account", icon: Settings, inApp: true }],
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

  // Hidden entries come straight from the router's admin-only group, so a page
  // can never be guarded but still listed, or listed but reachable.
  const visibleSections = NAV_SECTIONS.map((section) => ({
    ...section,
    items: section.items.filter((item) => isAdmin || !isAdminOnlySettingsPath(item.to)),
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
