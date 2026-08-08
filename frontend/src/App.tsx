import { Navigate, Route, Routes } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { AuthGate } from "@/components/AuthGate"
import Account from "@/routes/Account"
import Dashboard from "@/routes/Dashboard"
import Homekit from "@/routes/Homekit"
import Identity from "@/routes/Identity"
import Logs from "@/routes/Logs"
import Login from "@/routes/Login"
import Matter from "@/routes/Matter"
import Rtsp from "@/routes/Rtsp"
import Setup from "@/routes/Setup"
import SshKeys from "@/routes/SshKeys"
import StreamSettings from "@/routes/StreamSettings"
import System from "@/routes/System"
import Wifi from "@/routes/Wifi"

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
        <Route path="/identity" element={<Identity />} />
        <Route path="/wifi" element={<Wifi />} />
        <Route path="/stream-settings" element={<StreamSettings />} />
        <Route path="/rtsp" element={<Rtsp />} />
        <Route path="/homekit" element={<Homekit />} />
        <Route path="/matter" element={<Matter />} />
        <Route path="/system" element={<System />} />
        <Route path="/logs" element={<Logs />} />
        <Route path="/ssh-keys" element={<SshKeys />} />
        <Route path="/settings" element={<Account />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
