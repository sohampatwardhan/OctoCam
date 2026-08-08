import type { ReactElement } from "react"
import { Navigate, Route, Routes } from "react-router-dom"
import { AdminGate } from "@/components/AdminGate"
import { AppShell } from "@/components/AppShell"
import { AuthGate } from "@/components/AuthGate"
import { ADMIN_ONLY_SETTINGS_SLUGS, type AdminOnlySettingsSlug } from "@/lib/nav"
import Account from "@/routes/Account"
import Admin from "@/routes/Admin"
import Dashboard from "@/routes/Dashboard"
import Homekit from "@/routes/Homekit"
import Identity from "@/routes/Identity"
import Logs from "@/routes/Logs"
import Login from "@/routes/Login"
import Matter from "@/routes/Matter"
import Setup from "@/routes/Setup"
import SshKeys from "@/routes/SshKeys"
import StreamSettings from "@/routes/StreamSettings"
import System from "@/routes/System"
import Wifi from "@/routes/Wifi"

// Total in its key type: every admin-only slug must have a page here, and a
// page here must be a declared admin-only slug. That is what keeps the guarded
// group and the sidebar's hidden entries from drifting apart.
const ADMIN_ONLY_PAGES: Record<AdminOnlySettingsSlug, ReactElement> = {
  identity: <Identity />,
  wifi: <Wifi />,
  stream: <StreamSettings />,
  homekit: <Homekit />,
  matter: <Matter />,
  system: <System />,
  logs: <Logs />,
  "ssh-keys": <SshKeys />,
  admin: <Admin />,
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route path="/setup" element={<Setup />} />
      <Route
        element={
          <AuthGate>
            <AppShell />
          </AuthGate>
        }
      >
        <Route path="/" element={<Dashboard />} />
        <Route path="settings">
          <Route index element={<Navigate to="/settings/account" replace />} />
          <Route element={<AdminGate />}>
            {ADMIN_ONLY_SETTINGS_SLUGS.map((slug) => (
              <Route key={slug} path={slug} element={ADMIN_ONLY_PAGES[slug]} />
            ))}
          </Route>
          <Route path="account" element={<Account />} />
          <Route path="*" element={<Navigate to="/settings/account" replace />} />
        </Route>

        <Route path="identity" element={<Navigate to="/settings/identity" replace />} />
        <Route path="wifi" element={<Navigate to="/settings/wifi" replace />} />
        <Route path="stream-settings" element={<Navigate to="/settings/stream" replace />} />
        <Route path="rtsp" element={<Navigate to="/settings/stream" replace />} />
        <Route path="homekit" element={<Navigate to="/settings/homekit" replace />} />
        <Route path="matter" element={<Navigate to="/settings/matter" replace />} />
        <Route path="system" element={<Navigate to="/settings/system" replace />} />
        <Route path="logs" element={<Navigate to="/settings/logs" replace />} />
        <Route path="ssh-keys" element={<Navigate to="/settings/ssh-keys" replace />} />
        <Route path="admin" element={<Navigate to="/settings/admin" replace />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
