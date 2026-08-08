/**
 * The settings pages that only admins may reach.
 *
 * Two consumers derive from this list: the router groups these slugs under
 * `AdminGate`, and the sidebar hides their entries from non-admins. Keeping
 * both on one list is what stops navigation visibility and route access from
 * drifting apart. The router maps every slug here to a page element through a
 * total `Record`, so adding a slug without a page (or a page without a slug)
 * is a type error rather than a silently unguarded route.
 *
 * Account is deliberately absent: every signed-in user owns their own account
 * page. This list is a client-side convenience only — `require_admin_login` on
 * the backend remains the authority on what a non-admin may actually do.
 */
export const ADMIN_ONLY_SETTINGS_SLUGS = [
  "identity",
  "wifi",
  "stream",
  "motion",
  "homekit",
  "matter",
  "system",
  "logs",
  "ssh-keys",
  "mqtt",
  "admin",
] as const

export type AdminOnlySettingsSlug = (typeof ADMIN_ONLY_SETTINGS_SLUGS)[number]

const ADMIN_ONLY_SETTINGS_PATHS: ReadonlySet<string> = new Set(
  ADMIN_ONLY_SETTINGS_SLUGS.map((slug) => `/settings/${slug}`)
)

/** True for a canonical admin-only settings path, e.g. `/settings/wifi`. */
export function isAdminOnlySettingsPath(pathname: string): boolean {
  return ADMIN_ONLY_SETTINGS_PATHS.has(pathname)
}
