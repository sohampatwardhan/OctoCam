import { Navigate, Route, Routes } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { AuthGate } from "@/components/AuthGate"
import Dashboard from "@/routes/Dashboard"
import Login from "@/routes/Login"
import Wifi from "@/routes/Wifi"

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route
        element={
          <AuthGate>
            <AppShell />
          </AuthGate>
        }
      >
        <Route path="/" element={<Dashboard />} />
        <Route path="/wifi" element={<Wifi />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
