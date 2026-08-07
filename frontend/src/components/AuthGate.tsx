import type { ReactNode } from "react"
import { Navigate } from "react-router-dom"
import { useMe } from "@/hooks/useAuth"
import { Skeleton } from "@/components/ui/skeleton"

// Gates the authenticated shell. Renders a skeleton while `/api/me` is in
// flight, bounces to /login on error, logged-out, or unfinished setup.
export function AuthGate({ children }: { children: ReactNode }) {
  const { data, isLoading, isError } = useMe()

  if (isLoading) {
    return (
      <div className="flex min-h-screen flex-col">
        <div className="flex h-12 items-center gap-3 border-b border-border px-4">
          <Skeleton className="h-5 w-24" />
          <div className="ml-auto flex items-center gap-2">
            <Skeleton className="h-6 w-16 rounded-full" />
            <Skeleton className="size-8 rounded-lg" />
            <Skeleton className="size-8 rounded-lg" />
          </div>
        </div>
        <div className="flex-1 p-6">
          <Skeleton className="h-40 w-full max-w-2xl rounded-xl" />
        </div>
      </div>
    )
  }

  if (isError || !data?.authenticated || data.setup_required) {
    return <Navigate to="/login" replace />
  }

  return <>{children}</>
}
