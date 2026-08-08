# Requirements: Settings Consolidation under `/settings` (Nested Routes)

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

## Introduction

The OctoCam web UI is a React 19 + React Router v7 single-page app. Today every settings
page is a **top-level route** (`/identity`, `/wifi`, `/stream-settings`, `/rtsp`, `/homekit`,
`/matter`, `/system`, `/logs`, `/ssh-keys`, `/admin`, `/settings`), with the Dashboard at `/`.
RTSP is its own page even though it configures the same stream as the Stream Config page.

This feature reorganizes the information architecture without changing any backend API:

1. **Consolidation** — all settings pages move under a single `/settings` base using
   **nested path routes** (React Router `<Outlet>`), e.g. `/settings/identity`,
   `/settings/stream`, `/settings/account`. The Dashboard stays at `/`.
2. **RTSP merge** — the standalone RTSP page is folded into the Stream Config page as a
   section; the separate RTSP route and sidebar entry are removed.
3. **Back-compat** — the previous top-level URLs redirect to their new nested locations so
   existing bookmarks and deep links keep working.

This is a **frontend-only** change: the `/api/*` contract, authentication, and RBAC behavior
are unchanged. Value: a coherent, conventional URL structure (`/settings/*`), one place to
configure streaming (HD/SD + RTSP), and no broken bookmarks.

### Domain terms

- **Settings shell** — the persistent chrome (topbar + sidebar) shown around every settings
  page, distinct from the full-width Dashboard.
- **Settings page** — one of: Identity, Wi-Fi, Stream Config (incl. RTSP), HomeKit, Matter,
  System info, System logs, SSH keys, Admin, Account.
- **Nested route** — a child route rendered into the settings shell's `<Outlet>`, addressed
  by a path segment under `/settings/`.

### Approved decisions (from the user)

- URL scheme: **nested path routes** (`/settings/<page>`), not query params or hash.
- Scope: **all** settings pages, **including Account**, move under `/settings`; Dashboard stays at `/`.

### Assumptions (open to change at this gate)

- **A1.** The canonical path slugs are: `identity`, `wifi`, `stream`, **homekit**, `matter`,
  `system`, `logs`, `ssh-keys`, `admin`, `account`. (Stream Config → `stream`; there is no
  `rtsp` slug — RTSP lives inside `stream`.)
- **A2.** Visiting the bare `/settings` redirects to `/settings/account`, preserving the
  behavior of the old `/settings` URL (which was the Account page). If you'd prefer `/settings`
  to land on a different default (e.g. Identity), say so.
- **A3.** RTSP retains its own save action within the Stream Config page (no forced unification
  of the two save flows); exact layout is a design decision.

## Requirements

### Requirement 1: Settings pages served under a nested `/settings` base

**User Story:** As an OctoCam admin, I want every settings screen under one `/settings` URL
space, so that the address bar reflects a clear, consistent hierarchy.

#### Acceptance Criteria

1. **R1.1** WHEN a user navigates to `/settings/<slug>` for a valid slug (per A1), THE SPA Router SHALL render the corresponding settings page inside the settings shell.
2. **R1.2** THE SPA Router SHALL serve the following pages at these paths: Identity at `/settings/identity`, Wi-Fi at `/settings/wifi`, Stream Config at `/settings/stream`, HomeKit at `/settings/homekit`, Matter at `/settings/matter`, System info at `/settings/system`, System logs at `/settings/logs`, SSH keys at `/settings/ssh-keys`, Admin at `/settings/admin`, Account at `/settings/account`.
3. **R1.3** THE SPA Router SHALL continue to render the Dashboard at `/` as a full-width page without the settings sidebar.
4. **R1.4** WHEN a user navigates to the bare `/settings`, THE SPA Router SHALL redirect to `/settings/account` (per A2).
5. **R1.5** WHILE any `/settings/<slug>` page is displayed, THE Settings_Shell SHALL show the persistent topbar and settings sidebar.

### Requirement 2: RTSP merged into the Stream Config page

**User Story:** As an admin, I want RTSP configured on the same screen as the HD/SD stream, so
that all streaming settings live in one place.

#### Acceptance Criteria

1. **R2.1** WHEN the Stream Config page (`/settings/stream`) is displayed, THE Stream_Config_Page SHALL present the RTSP controls (service-enabled toggle, path, max-clients) and the RTSP stream URLs (HD and SD) as a section of that page.
2. **R2.2** THE Stream_Config_Page SHALL let the user edit and save the RTSP settings with the same fields, validation, and persisted effect as the previous standalone RTSP page.
3. **R2.3** THE SPA Router SHALL NOT expose a standalone RTSP route; there is no `/settings/rtsp` or `/rtsp` page that renders separately.
4. **R2.4** THE Sidebar SHALL NOT show a separate "RTSP" navigation item.
5. **R2.5** WHEN the user copies an RTSP stream URL from the Stream Config page, THE Stream_Config_Page SHALL copy the same URL value that the standalone page provided.

### Requirement 3: Sidebar and in-app navigation target the new locations

**User Story:** As a user, I want every link and the sidebar to point at the new URLs and show
which page I'm on, so that navigation is correct and unambiguous.

#### Acceptance Criteria

1. **R3.1** THE Sidebar SHALL link each settings entry to its `/settings/<slug>` path.
2. **R3.2** WHILE a settings page is displayed, THE Sidebar SHALL mark exactly one navigation entry as active — the one matching the current path.
3. **R3.3** WHEN the user activates the Account Settings control (sidebar entry and topbar gear), THE Settings_Shell SHALL navigate to `/settings/account`.
4. **R3.4** WHEN the user activates an in-app link that previously pointed to a legacy settings URL (e.g. the "RTSP" shortcut on the Dashboard stream preview), THE SPA SHALL land the user on the correct new page (`/settings/stream` for the RTSP shortcut).
5. **R3.5** WHILE the Dashboard is displayed at `/`, THE Sidebar SHALL NOT be shown (unchanged full-width behavior).

### Requirement 4: Legacy URLs redirect for back-compat

**User Story:** As a user with existing bookmarks, I want old settings URLs to still work, so
that saved links and shared deep-links don't break.

#### Acceptance Criteria

1. **R4.1** WHEN an authenticated user navigates to a legacy top-level settings URL, THE SPA Router SHALL redirect to the corresponding new nested URL: `/identity`→`/settings/identity`, `/wifi`→`/settings/wifi`, `/stream-settings`→`/settings/stream`, `/homekit`→`/settings/homekit`, `/matter`→`/settings/matter`, `/system`→`/settings/system`, `/logs`→`/settings/logs`, `/ssh-keys`→`/settings/ssh-keys`, `/admin`→`/settings/admin`.
2. **R4.2** WHEN an authenticated user navigates to the legacy `/rtsp` URL, THE SPA Router SHALL redirect to `/settings/stream`.
3. **R4.3** WHEN an authenticated user navigates to the legacy `/settings` URL, THE SPA Router SHALL land the user on `/settings/account` (via R1.4).
4. **R4.4** WHEN a redirect from a legacy URL occurs, THE SPA Router SHALL replace the history entry so the browser Back button does not return to the legacy URL.
5. **R4.5** WHEN an authenticated session is absent and a user loads a protected legacy settings URL, THE AuthGate SHALL route the user to the login flow before any legacy-to-canonical redirect is rendered, preserving the current protected-route behavior.

### Requirement 5: No functional or authorization regression on moved pages

**User Story:** As an admin, I want each relocated page to work exactly as before, so that the
reorganization is purely structural.

#### Acceptance Criteria

1. **R5.1** THE SPA SHALL preserve every user-visible capability of each moved page (view, edit, save, copy, and any page-specific actions) after relocation.
2. **R5.2** WHILE the signed-in user is not an admin, THE Sidebar SHALL hide Identity, Wi-Fi, Stream Config, HomeKit, Matter, System info, System logs, SSH keys, and Admin while continuing to show Account, exactly as before the move.
3. **R5.3** THE SPA SHALL continue to call the same `/api/*` endpoints with the same request payloads for each moved page; no backend API change is introduced by this feature.
4. **R5.4** IF a moved page performed a data mutation (e.g. save settings, add/revoke SSH key, reset pairing) before the change, THEN THE SPA SHALL perform the identical mutation from its new location.
5. **R5.5** WHEN an authenticated non-admin directly loads a canonical admin-only settings URL, THE SPA and backend SHALL produce the same page and API authorization outcomes as the corresponding legacy URL produced before the move; this feature SHALL add or remove no route-level authorization rule.
6. **R5.6** WHEN a non-admin request reaches an admin-protected backend mutation used by a moved page, THE backend SHALL continue to deny that request under its existing authorization rules.

### Requirement 6: Direct navigation and refresh on nested URLs

**User Story:** As a user, I want to bookmark or refresh a nested settings URL and land on that
exact page, so that deep-linking is reliable.

#### Acceptance Criteria

1. **R6.1** WHEN a user loads a valid `/settings/<slug>` URL directly (fresh navigation or page refresh), THE web server SHALL return the SPA application shell (not a 404), and THE SPA Router SHALL render the matching page.
2. **R6.2** WHEN an authenticated session is absent and a user loads any `/settings/<slug>` URL, THE AuthGate SHALL route the user to the login flow exactly as it does for other protected routes today.
3. **R6.3** IF a user navigates to `/settings/<unknown-slug>`, THEN THE SPA Router SHALL redirect to a defined destination (the bare-`/settings` default per R1.4) rather than rendering a blank or broken view.
4. **R6.4** IF a user navigates to any other unknown path outside `/settings`, THEN THE SPA Router SHALL redirect to `/` (preserving today's catch-all behavior).

## Risk classification

- **Authorization (must-hold):** sidebar visibility, direct-route behavior, and backend mutation
  enforcement remain unchanged. Account remains visible to all signed-in users. Covered by
  R5.2, R5.5, and R5.6.
- **Back-compat / rollout:** external bookmarks and shared links depend on legacy URLs; covered
  by R4. No server route removal that would 404 a legacy path (the SPA fallback serves the shell
  and the router redirects).
- **Regression:** the main risk is losing a page capability during the move; covered by R5.
- **Deep-link robustness:** server SPA fallback must cover `/settings/*`; covered by R6.1.
- **No data/migration/privacy surface:** no persisted schema, cookie, or API change; this is a
  client-side routing and component-composition change only.

## Out of scope

- Any change to backend endpoints, auth, session cookies, or RBAC rules.
- Visual redesign of the individual pages beyond composing RTSP into Stream Config.
- Changing the Dashboard, Login, or Setup routes/behavior.
