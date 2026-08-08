import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react"

export interface UnsavedSection {
  /** Stable key for the form reporting the change. */
  id: string
  /** Human name for the form, e.g. "Stream settings". */
  label: string
  /** DOM id to scroll to when the indicator is activated. */
  anchorId: string
}

export interface UnsavedChangesValue {
  sections: UnsavedSection[]
  report: (id: string, section: UnsavedSection | null) => void
}

export const UnsavedChangesContext = createContext<UnsavedChangesValue | null>(null)

/**
 * Backing state for UnsavedChangesProvider.
 *
 * Settings pages deliberately keep independent forms and independent save
 * actions, so this never submits anything — it only lets the shell surface
 * that unsaved work exists and point at it. Saving stays with the form that
 * owns the data.
 */
export function useUnsavedChangesState(): UnsavedChangesValue {
  const [registry, setRegistry] = useState<Record<string, UnsavedSection>>({})

  const report = useCallback((id: string, section: UnsavedSection | null) => {
    setRegistry((current) => {
      if (!section) {
        if (!(id in current)) return current
        const next = { ...current }
        delete next[id]
        return next
      }
      const existing = current[id]
      if (existing && existing.label === section.label && existing.anchorId === section.anchorId) {
        return current
      }
      return { ...current, [id]: section }
    })
  }, [])

  const sections = useMemo(() => Object.values(registry), [registry])
  return useMemo(() => ({ sections, report }), [sections, report])
}

export function useUnsavedChanges(): UnsavedChangesValue {
  const context = useContext(UnsavedChangesContext)
  if (!context) {
    throw new Error("useUnsavedChanges must be used within UnsavedChangesProvider")
  }
  return context
}

/** Reports one form's dirty state to the shell, clearing it on unmount. */
export function useReportUnsavedChanges(section: UnsavedSection, dirty: boolean) {
  const { report } = useUnsavedChanges()
  const { id, label, anchorId } = section

  useEffect(() => {
    report(id, dirty ? { id, label, anchorId } : null)
  }, [report, id, label, anchorId, dirty])

  // Leaving the page cannot leave a stale badge behind in the shell.
  useEffect(() => () => report(id, null), [report, id])
}

/** Compares flat form state, tolerating the bigint fields JSON cannot handle. */
export function isFormDirty<T extends object>(current: T | null, initial: T | null): boolean {
  if (!current || !initial) return false
  return (Object.keys(current) as (keyof T)[]).some((key) => current[key] !== initial[key])
}
