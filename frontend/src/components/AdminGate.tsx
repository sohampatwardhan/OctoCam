import { Navigate, Outlet } from "react-router-dom"
import { useMe } from "@/hooks/useAuth"
import { Skeleton } from "@/components/ui/skeleton"

// Gates the admin-only settings group. Renders inside AuthGate, so the viewer
// is already known to be signed in; this only decides whether they may see
// admin pages at all. Non-admins bounce to their own account page.
//
// This is convenience, not enforcement — `require_admin_login` on the backend
// still 403s every admin-protected request, and must keep doing so. Without
// this gate a non-admin who typed an admin URL got the full page shell wrapped
// around a generic "couldn't load" error, which reads as a network failure
// rather than a permissions one and leaks the page's structure.
export function AdminGate() {
  const { data, isLoading } = useMe()

  // AuthGate resolves `/api/me` before we mount, so this is a near-impossible
  // race. Wait it out rather than redirecting, so an admin is never bounced on
  // an unresolved query.
  if (isLoading) {
    return (
      <div className="flex flex-col gap-6 p-4 sm:p-6">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-40 w-full max-w-2xl rounded-xl" />
      </div>
    )
  }

  if (!data?.is_admin) {
    return <Navigate to="/settings/account" replace />
  }

  return <Outlet />
}
