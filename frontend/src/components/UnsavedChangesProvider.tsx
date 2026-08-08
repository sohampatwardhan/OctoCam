import type { ReactNode } from "react"
import { UnsavedChangesContext, useUnsavedChangesState } from "@/hooks/useUnsavedChanges"

/** Scopes unsaved-change reporting to the authenticated shell. */
export function UnsavedChangesProvider({ children }: { children: ReactNode }) {
  const value = useUnsavedChangesState()
  return <UnsavedChangesContext.Provider value={value}>{children}</UnsavedChangesContext.Provider>
}
