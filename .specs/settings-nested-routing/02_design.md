# Design: Settings Consolidation under `/settings` (Nested Routes)

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

## Overview

A frontend-only refactor of the OctoCam React Router v7 SPA. All settings pages move under a
`/settings/<slug>` prefix using nested routes; the standalone RTSP page becomes a section of the
Stream Config page; legacy top-level URLs redirect (history-replace) to the new locations. No
backend, API, auth, or RBAC change. The Dashboard stays at `/`.

### Current technology evidence

- **Library:** React Router v7 (`/remix-run/react-router`, declarative `<Routes>`/`<Route>` under
  `BrowserRouter`). Queried 2026-08-08 via context7.
  - **Path-prefix route without `element`:** `<Route path="settings">…children…</Route>` adds the
    `settings` URL segment to its children without introducing a layout; children render into the
    nearest ancestor `<Outlet>` (here, `AppShell`). Confirmed by the docs' "Route Prefixes" example.
  - **Index redirect:** an `index` route may render `<Navigate to="/settings/account" replace />` to
    define the default child for the bare parent path. Confirmed by the "Index Routes" + navigate
    examples. (We use absolute `to` targets to avoid relative-splat ambiguity.)
  - **Unknown sub-path:** a `<Route path="*">` inside the `settings` prefix catches unknown
    `/settings/<x>` and can redirect. The existing top-level `<Route path="*">` still catches
    everything else.
- No new dependency is introduced; this uses APIs the app already relies on (`Routes`, `Route`,
  `Navigate`, `Outlet`, `NavLink`).

## Route architecture

The slug map below is the authoritative route topology. A diagram is intentionally omitted:
the design describes a static route hierarchy, not an executable control flow, and the table is
the more precise verification surface.

### Slug map

| Page | New path | Legacy path (redirects) |
|---|---|---|
| Identity | `/settings/identity` | `/identity` |
| Wi-Fi | `/settings/wifi` | `/wifi` |
| Stream Config (incl. RTSP) | `/settings/stream` | `/stream-settings`, `/rtsp` |
| HomeKit | `/settings/homekit` | `/homekit` |
| Matter | `/settings/matter` | `/matter` |
| System info | `/settings/system` | `/system` |
| System logs | `/settings/logs` | `/logs` |
| SSH keys | `/settings/ssh-keys` | `/ssh-keys` |
| Admin | `/settings/admin` | `/admin` |
| Account | `/settings/account` | `/settings` |
| Dashboard | `/` (unchanged) | — |

## Components and changes

### 1. [frontend/src/App.tsx](../../frontend/src/App.tsx) — router restructure (primary change)

Within the existing `<AuthGate><AppShell/></AuthGate>` layout route:

- Keep `/` → `Dashboard`.
- Add a `settings` **path-prefix route with no `element`** containing:
  - `index` → `<Navigate to="/settings/account" replace />` (satisfies A2 / R1.4).
  - one child per slug (`identity`, `wifi`, `stream`, **homekit**, `matter`, `system`, `logs`,
    `ssh-keys`, `admin`, `account`) rendering the existing page component.
  - `path="*"` → `<Navigate to="/settings/account" replace />` (R6.3).
- Add legacy redirect routes as siblings, each `<Route path="<legacy>" element={<Navigate to="<new>" replace/>}/>` (R4). `/rtsp` and `/stream-settings` both target `/settings/stream`.
- Leave the top-level `<Route path="*" element={<Navigate to="/" replace/>}/>` unchanged (R6.4).
- Remove the `Rtsp` import/route (page is deleted; see §4).

Because the `settings` prefix has no `element`, children render into `AppShell`'s `<Outlet>` — no
new layout component or nested Outlet is needed.

### 2. [frontend/src/components/AppShell.tsx](../../frontend/src/components/AppShell.tsx) — no functional change

`withSidebar = pathname !== "/"` already yields the sidebar for every `/settings/*` path and keeps
`/` full-width. No edit required; noted here to make the "unchanged" explicit.

### 3. [frontend/src/components/Sidebar.tsx](../../frontend/src/components/Sidebar.tsx) — retarget nav + drop RTSP

- Change each settings `NavItem.to` to its `/settings/<slug>` value; Account → `/settings/account`.
- Remove the "RTSP" `NavItem` (and now-unused `Radio` import if no longer referenced).
- `adminOnly` flags and the `is_admin` filter are unchanged (sidebar visibility preserved, R5.2).
- `NavLink` active matching: settings items use default (non-`end`) matching; since no slug nests
  further, exactly one entry is active per path (R3.2). The Dashboard entry keeps `end`.

### 4. RTSP merge into Stream Config

- **New** [frontend/src/components/stream/RtspSection.tsx](../../frontend/src/components/stream/RtspSection.tsx): a self-contained section extracted
  from `frontend/src/routes/Rtsp.tsx` — its own local form state seeded from `useSettings`, its own
  `useUpdateSettings()` instance, its own **"Save RTSP settings"** button (A3 / R2.2), and the
  RTSP stream-URLs card (`useQuery(["rtsp"], /api/rtsp)`), shown when RTSP is enabled. Because each
  `useUpdateSettings()` call site is an independent React Query mutation, the RTSP save is isolated
  from the stream-settings save. The component receives a concise native documentation comment
  stating this independent-form and independent-mutation contract.
- [frontend/src/routes/StreamSettings.tsx](../../frontend/src/routes/StreamSettings.tsx) renders `<RtspSection />` after the main stream form's save card, as a
  distinct block (two independent forms on one page).
- **Delete** `frontend/src/routes/Rtsp.tsx`; remove its route and import from [frontend/src/App.tsx](../../frontend/src/App.tsx).
- `/api/rtsp` and the `rtsp_enabled/rtsp_path/rtsp_max_clients` settings fields are used exactly as
  before — the code moves, the behavior does not (R2.1, R2.2, R2.5, R5.1, R5.3).

### 5. In-app links

- [frontend/src/components/Topbar.tsx](../../frontend/src/components/Topbar.tsx) settings gear: `href="/settings"` → `/settings/account` (R3.3).
- [frontend/src/components/dashboard/StreamPreview.tsx](../../frontend/src/components/dashboard/StreamPreview.tsx) "RTSP" shortcut: `href="/rtsp"` → `/settings/stream` (R3.4).
  (Even if missed, the `/rtsp` redirect would still land correctly, but links are updated to point
  directly at the canonical path.)

### 6. Server / deep-linking

No server change. The axum SPA fallback already serves the application shell for any unmatched path
(`spa_asset` fallback), so a direct load or refresh of `/settings/<slug>` returns the shell and the
router renders the matching page (R6.1). `AuthGate` continues to gate the whole `AppShell` layout,
so an unauthenticated canonical or legacy protected deep-link bounces to `/login` before child
route redirects render (R4.5, R6.2).

## Data models

Unchanged. No new types, no `/api/*` shape change. `WifiStatusSummary`, `Settings`, `RtspUrls`,
`StreamOptions`, etc. are used as-is.

## Behavioral regression matrix

This matrix defines the exhaustive moved-page verification contract for R5.1, R5.3, and R5.4.
Relocation must preserve each listed capability, endpoint, and mutation payload.

| Page | Capabilities to preserve | Existing API contract to preserve |
|---|---|---|
| Identity | View and save device name, room, and camera label | `GET /api/settings`; `PUT /api/settings` with the same partial identity fields |
| Wi-Fi | View saved/available networks; scan, connect, and forget | `GET /api/wifi/saved`; `GET /api/wifi/networks`; unchanged `POST /api/wifi/scan`, `POST /api/wifi/connect`, and `DELETE /api/wifi/delete` payloads |
| Stream Config + RTSP | View and save stream fields; view options; independently edit/save RTSP; copy HD/SD URLs | `GET /api/settings`; `GET /api/stream-options`; unchanged partial `PUT /api/settings` payloads; `GET /api/rtsp` |
| HomeKit | View pairing/status data and save enabled state | `GET /api/settings`; `GET /api/status`; `GET /api/homekit`; unchanged partial `PUT /api/settings` payload |
| Matter | View pairing/status data, save enabled state, and reset pairing | `GET /api/settings`; `GET /api/matter`; unchanged partial `PUT /api/settings` payload; `POST /api/matter/reset` with the same payload |
| System info | View system status | `GET /api/status` |
| System logs | View and refresh logs | `GET /api/logs` |
| SSH keys | View, add, copy, and revoke keys | `GET /api/ssh-keys`; unchanged `POST /api/ssh-keys` and `DELETE /api/ssh-keys` payloads; `GET /api/status` for target display |
| Admin | View users; add and delete users | `GET /api/users`; unchanged `POST /api/users/add` and `DELETE /api/users/:id` payloads |
| Account | Change password; view, register, rename, and delete passkeys | Unchanged partial `PUT /api/settings` payload; `GET /api/passkeys`; unchanged passkey registration, rename, and delete endpoint payloads |

## Correctness properties

1. **Every settings slug resolves to its page inside the shell.** Loading `/settings/<slug>` for
   each slug in the map renders that page with the topbar + sidebar. **Validates: Requirements 1.1, 1.2, 1.5.**
2. **Dashboard stays root and full-width.** `/` renders the Dashboard with no sidebar. **Validates: Requirements 1.3, 3.5.**
3. **Bare `/settings` lands on Account.** Navigating to `/settings` ends on `/settings/account`.
   **Validates: Requirements 1.4, 4.3.**
4. **RTSP is a section of Stream Config, independently savable.** `/settings/stream` shows the RTSP
   enable/path/max-clients controls and (when enabled) the HD/SD URLs; saving RTSP persists those
   fields without submitting the stream-settings form, and vice-versa. **Validates: Requirements 2.1, 2.2, 2.5.**
5. **No standalone RTSP surface.** There is no route or sidebar entry that renders RTSP on its own.
   **Validates: Requirements 2.3, 2.4.**
6. **Sidebar + links target new paths with correct active state.** Each sidebar entry links to its
   `/settings/<slug>`; exactly one entry is active per path; the gear and the Dashboard RTSP shortcut
   navigate to `/settings/account` and `/settings/stream`. **Validates: Requirements 3.1, 3.2, 3.3, 3.4.**
7. **Legacy URLs preserve authenticated and logged-out behavior.** Each authenticated legacy path
  lands on its new path with history-replace; without a session, `AuthGate` reaches `/login`
  before a child redirect renders. **Validates: Requirements 4.1, 4.2, 4.4, 4.5.**
8. **No capability or authorization regression.** Every matrix row retains its actions, endpoints,
  and payloads; non-admin sidebar visibility is unchanged; direct canonical URLs have the same
  page/API result as their legacy counterparts; backend denials remain authoritative.
  **Validates: Requirements 5.1, 5.2, 5.3, 5.4, 5.5, 5.6.**
9. **Deep-link / refresh / unknown paths behave.** Direct load or refresh of a valid nested path
   renders it; unauthenticated deep-link bounces to `/login`; unknown `/settings/*` → `/settings/account`;
   other unknown paths → `/`. **Validates: Requirements 6.1, 6.2, 6.3, 6.4.**

## Risk gates

- **Authorization/RBAC:** *Failure mode* — relocation changes navigation visibility, direct-route
  behavior, or backend enforcement. *Verification* — compare non-admin sidebar visibility and each
  legacy/canonical direct-route pair; confirm an admin-protected mutation is still rejected by the
  backend. No route-level guard is added or removed. *Owner:* this change keeps the existing model
  (R5.2, R5.5, R5.6).
- **Rollout / back-compat:** *Failure mode* — an external bookmark 404s. *Verification* — legacy
  routes redirect (R4); the server serves the SPA shell for any path, so no legacy URL 404s.
- **Regression:** *Failure mode* — RTSP loses a field or the two saves interfere. *Verification* —
  RtspSection is a move of existing logic with an independent mutation instance; verified by saving
  RTSP and stream settings separately on `/settings/stream`.
- **Deep-link robustness:** *Failure mode* — refresh on `/settings/stream` 404s. *Verification* —
  axum SPA fallback already covers arbitrary paths (unchanged); confirmed by direct load/refresh.
- **Accessibility:** nav semantics unchanged (NavLink active state, mobile drawer). No new failure
  surface.
- **Performance:** negligible — the bundle uses existing dependencies. Visiting Stream Config now
  also fetches `/api/rtsp` while RTSP is enabled because that section is composed into the page;
  no polling or request is added elsewhere.
- **Privacy / migration / observability:** not applicable — no persisted state, cookie, schema, or
  logging change.

## Testing strategy

- **Type + build:** `tsc -b` and `vite build` clean.
- **Dev-server (proxied to Pi) behavioral checks:** visit every `/settings/<slug>`; confirm sidebar
  active state; exercise every row in the behavioral regression matrix and capture endpoint/payload
  evidence; verify independent Stream/RTSP saves and unsaved-state isolation; test every legacy URL,
  history replacement, logged-out canonical and legacy loads, direct non-admin canonical loads,
  backend denial, both unknown-path classes, and Dashboard full-width behavior.
- **Accessibility and responsive checks:** verify keyboard order, labels, submit-button ownership,
  and mobile overflow on the composed Stream Config page.
- **On-Pi:** deploy, health-gate, then directly load and refresh every canonical nested URL and
  smoke-test every legacy redirect in the browser.

## Alternatives considered

- **Query param / hash (`?page=` / `#`):** rejected per approved requirements — nested path routes
  are the idiomatic React Router convention with cleaner deep-links and active states.
- **A dedicated `SettingsLayout` component with its own `<Outlet>`:** unnecessary — `AppShell`
  already provides the sidebar chrome for non-dashboard routes, so a pathless prefix suffices and
  avoids a redundant nested layout.
- **Merging RTSP into the single stream-settings form (one Save):** rejected by A3; RTSP keeps its
  own save to preserve current behavior and avoid coupling two unrelated settings groups.

## Out of scope

Backend/API/auth/RBAC changes; visual redesign of pages beyond composing RTSP into Stream Config;
Dashboard/Login/Setup behavior.
