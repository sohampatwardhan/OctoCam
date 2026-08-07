import { QueryClient } from "@tanstack/react-query"

// Shared singleton so components outside the provider tree (route handlers,
// event callbacks) can invalidate/clear the same cache the app renders from.
export const queryClient = new QueryClient()
