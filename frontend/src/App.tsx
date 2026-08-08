import { Navigate, Route, Routes } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { AuthGate } from "@/components/AuthGate"
import Dashboard from "@/routes/Dashboard"
import Homekit from "@/routes/Homekit"
import Login from "@/routes/Login"
import Matter from "@/routes/Matter"
import Rtsp from "@/routes/Rtsp"
import Setup from "@/routes/Setup"
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
        <Route path="/wifi" element={<Wifi />} />
        <Route path="/rtsp" element={<Rtsp />} />
        <Route path="/homekit" element={<Homekit />} />
        <Route path="/matter" element={<Matter />} />
        <Route path="/system" element={<System />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
