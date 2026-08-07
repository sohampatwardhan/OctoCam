# UI Overhaul — Phase 3 (slice 1): React App Shell + Auth + Dashboard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement task-by-task. Implementers should also use frontend-design for the React UI work. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make the redesigned UI visible: a themed React app shell (topbar + sidebar nav), a login page + auth gate wired to the real session, and a fully working dashboard (stream preview, stream-health, clients) — all under `/app`, backed by the Phase 2 JSON API.

**Architecture:** Extend the Phase 1 React SPA. Add React Router (basename `/app`), an auth layer (`/api/me` + `<AuthGate>`), a shared `<AppShell>` (topbar/sidebar), a `<Login>` page (`/api/login`), and a `<Dashboard>` page. Re-skin the shadcn theme to OctoCam's existing brand tokens. One small backend addition: `browser_stream_urls` on `/api/status`.

**Tech Stack:** React 19 + TS + Vite, shadcn/ui + Tailwind v4, TanStack Query, react-router-dom v7, lucide-react; Rust/Axum (one additive change).

## Global Constraints

- **Served under `/app`** (Vite `base: "/app/"`); React Router uses `basename="/app"`. Does not touch the live Askama pages.
- **Match OctoCam's existing brand, not generic shadcn.** Design tokens (hex, from `static/styles.css:1-58`): `--background:#f5f7f8`, `--foreground:#16212b`, `--card:#ffffff`, `--muted:#f9fbfc`, `--muted-foreground:#60707f`, `--border:#d6dee5`, `--ring:#2f7dd3`, `--primary:#c5462d`, `--primary-foreground:#ffffff`, `--accent:#fff0ec`, `--destructive:#c5462d` (=primary), `--success:#08734f`, `--radius:8px`. Font: system stack `ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` (NO Geist). Dark mode is AUTO (see the dark-mode constraint below).
- **Auto dark mode** via `@media (prefers-color-scheme: dark)` — follows the OS, NO manual toggle. Wire shadcn's `dark:` variant to the media query and provide dark token overrides (base them on the existing `--inverse:#111820`/`--inverse-foreground:#e8eef3` surfaces + a brightened terracotta primary for contrast).
- **Stream preview stays an `<iframe>`** pointing at mediamtx's player (`http://{host}:8889/{path}`); start/stop and HD/SD are `src` swaps. Do NOT build a native WHEP client in this slice.
- **Terminal is excluded** — no Terminal nav item, no terminal page.
- **Auth:** all `/app` routes except `/app/login` require a session (checked via `/api/me`); a `401` from any query routes to `/app/login`. Admin-only nav items gate on `role`/`is_admin` from `/api/me` (server still enforces).
- **Efficiency (Pi Zero 2 W):** status polling at 5s via TanStack Query; keep the bundle lean (only pull shadcn components used); no new heavy deps beyond react-router-dom.
- **Rust:** additive only; `cargo build --release --locked`; keep `Cargo.lock` in sync.

## Design decisions baked in (call out on review)

1. **Dashboard stays full-width (no sidebar)** — matches today's Askama dashboard (topbar only). The `Sidebar` component is deferred to the first sidebar-bearing page slice; slice-1's shell is topbar + full-width content. (User decision.)
2. **Keep the iframe** stream preview (faithful, zero new WebRTC code).
3. **Auto dark mode** via `prefers-color-scheme` (no toggle). (User decision — reverses the earlier light-only default.)
4. **Passkey login included** in this slice — the `/app` login page ports the passkey login flow (`/api/passkey/login/start`+`/finish`) alongside password login. (User decision.)
5. **Cross-links to not-yet-migrated pages** (e.g. the dashboard's "RTSP" button, sidebar items for pages not built in this slice) point at the existing Askama routes (`/rtsp`, `/wifi`, …) so navigation works during migration. They'll be repointed to `/app/*` as those pages land.

---

## File Structure

**Backend — Modify:**
- `rust/octocam-web/src/main.rs` — add `browser_stream_urls` to the `StatusResponse` in `api_status`.

**Frontend — Create:**
- `frontend/src/main.tsx` — mount with `<BrowserRouter basename="/app">` + QueryClient (modify existing).
- `frontend/src/App.tsx` — route table (replace pilot).
- `frontend/src/lib/api.ts` — extend: typed `Status`, `Me`, `login`, `logout`, `apiPost`.
- `frontend/src/hooks/useAuth.ts` — `useMe()` query + helpers.
- `frontend/src/components/AuthGate.tsx` — redirect-to-login wrapper.
- `frontend/src/components/AppShell.tsx` — topbar + sidebar layout + `<Outlet/>`.
- `frontend/src/components/Topbar.tsx`, `frontend/src/components/PowerDialog.tsx`. (Sidebar deferred — dashboard is full-width.)
- `frontend/src/routes/Login.tsx`, `frontend/src/routes/Dashboard.tsx`; `frontend/src/lib/webauthn.ts` (base64url helpers for passkey login).
- `frontend/src/components/dashboard/StreamPreview.tsx`, `StreamHealthCard.tsx`, `ClientsCard.tsx`.
- shadcn components as needed: `dialog`, `separator`, `input`, `label`, `skeleton` (add via CLI).

---

## Task 1: Re-skin the shadcn theme to OctoCam brand tokens

**Files:** Modify `frontend/src/index.css`; add shadcn components.

**Interfaces:** Produces the themed CSS variables all components consume. No TS API.

- [ ] **Step 1: Replace the generic theme block in `frontend/src/index.css`.** Keep `@import "tailwindcss";` and the shadcn `@theme inline`/`@layer base` wiring, but replace the `:root` token values (and remove the `.dark` block and the Geist `@import`/`--font-sans: 'Geist…'`) with OctoCam's tokens. Set:
```css
:root {
  --background: #f5f7f8;
  --foreground: #16212b;
  --card: #ffffff;
  --card-foreground: #16212b;
  --popover: #ffffff;
  --popover-foreground: #16212b;
  --primary: #c5462d;
  --primary-foreground: #ffffff;
  --secondary: #f9fbfc;
  --secondary-foreground: #16212b;
  --muted: #f9fbfc;
  --muted-foreground: #60707f;
  --accent: #fff0ec;
  --accent-foreground: #16212b;
  --destructive: #c5462d;
  --border: #d6dee5;
  --input: #fbfcfd;
  --ring: #2f7dd3;
  --radius: 0.5rem; /* 8px */
}
```
Set the sans font var to the system stack (remove Geist): `--font-sans: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;` and drop the `@fontsource` Geist import line.

**Auto dark mode (no toggle):** wire shadcn's dark variant to the OS preference and provide dark tokens via media query. (a) Redefine the custom variant so `dark:` utilities key off the media query: `@custom-variant dark (@media (prefers-color-scheme: dark));`. (b) Add a `@media (prefers-color-scheme: dark) { :root { … } }` block overriding the tokens for dark surfaces — base them on OctoCam's existing inverse surfaces and a brightened primary, e.g. `--background:#111820; --foreground:#e8eef3; --card:#16212b; --card-foreground:#e8eef3; --popover:#16212b; --muted:#1c2732; --muted-foreground:#9fb0bd; --accent:#2a1a15; --accent-foreground:#e8eef3; --secondary:#1c2732; --border:#2a3742; --input:#1c2732; --primary:#e0674c; --primary-foreground:#ffffff; --destructive:#e0674c; --ring:#4a9de8;` (tune for legibility). This gives fully automatic light/dark with no JS and no toggle.

- [ ] **Step 2: Add the shadcn components this slice needs.**
```bash
cd frontend && npx --yes shadcn@latest add dialog input label separator skeleton
```

- [ ] **Step 3: Build and visually verify the theme.**
```bash
cd frontend && npm run build 2>&1 | tail -5
```
Expected: build clean. (Visual check happens once the shell renders in Task 3; here just confirm the token edit compiles and no Geist import remains: `grep -i geist src/index.css` returns nothing.)

- [ ] **Step 4: Commit.**
```bash
cd .. && git add frontend && git commit -m "feat(ui): re-skin shadcn theme to OctoCam brand tokens (light-only, system font)"
```

---

## Task 2: Backend — add `browser_stream_urls` to `/api/status`

**Files:** Modify `rust/octocam-web/src/main.rs` (the `api_status` handler / its `StatusResponse`).

**Interfaces:**
- Consumes: `stream_urls_for(&settings, request_hostname(&headers), "webrtc")` (main.rs:~3206) → `StreamUrls { main, sub, has_sub }`.
- Produces: `/api/status` JSON gains `browser_stream_urls: { main, sub, has_sub }` (available to any logged-in user, since `api_status` uses `require_user_login`).

- [ ] **Step 1: Read `api_status` (main.rs:~2212) and `stream_urls_for` (~3206).** Confirm `api_status` already takes `headers: HeaderMap` (it does — needed for `request_hostname`). Confirm the `"webrtc"` protocol arg produces `http://{host}:8889/{path}` URLs.

- [ ] **Step 2: Add the field to the `StatusResponse` struct in `api_status`.** It currently flattens `SystemStatus` + `viewers` + `motion_detected`. Add:
```rust
    #[derive(Serialize)]
    struct BrowserStreamUrls { main: String, sub: String, has_sub: bool }
    // in StatusResponse:
    browser_stream_urls: BrowserStreamUrls,
```
Populate it: `let urls = stream_urls_for(&settings, request_hostname(&headers), "webrtc");` then `browser_stream_urls: BrowserStreamUrls { main: urls.main, sub: urls.sub, has_sub: urls.has_sub }`. (Load `settings` if `api_status` doesn't already.)

- [ ] **Step 3: Add a unit test** asserting the `StatusResponse` (or a small helper) serializes with a `browser_stream_urls` object containing `main`/`sub`/`has_sub`. Then:
```bash
cd rust/octocam-web && cargo build 2>&1 | grep -iE 'error' | head && cargo test 2>&1 | grep 'test result' | tail -1
```

- [ ] **Step 4: Commit.**
```bash
cd ../.. && git add rust/octocam-web/src/main.rs && git commit -m "feat(api): expose browser_stream_urls on /api/status"
```

---

## Task 3: App shell + router + auth gate + login

**Files:** Modify `frontend/src/main.tsx`, `frontend/src/App.tsx`, `frontend/src/lib/api.ts`; create `hooks/useAuth.ts`, `components/{AuthGate,AppShell,Sidebar,Topbar,PowerDialog}.tsx`, `routes/Login.tsx`.

**Interfaces:**
- Produces: `useMe()` (TanStack Query for `/api/me` → `{authenticated, username, role, is_admin, setup_required}`); `<AuthGate>` (renders children if authed, else `<Navigate to="/login">`); `<AppShell>` (renders topbar+sidebar+`<Outlet/>`); `apiPost<T>(path, body)`.
- Consumes: Task 1 theme; `/api/me`, `/api/login`, `/api/logout`, `/api/power` (Phase 2).

- [ ] **Step 1: Install router.** `cd frontend && npm install react-router-dom`

- [ ] **Step 2: Extend `lib/api.ts`.** Add `apiPost<T>(path, body): Promise<T>` (POST JSON, `credentials:"same-origin"`, throws on !ok with the parsed `{error}` message when present). Add types: `interface Me { authenticated: boolean; username?: string; role?: string; is_admin?: boolean; setup_required: boolean }`. Extend `Status` to the real shape used by the dashboard: `services: { rtsp: {state:string}, octocam_web: {state:string} }`, `camera: { available: boolean, message: string }`, `uptime: string | null`, `motion_detected: boolean`, `viewers: ViewerReport | null`, `browser_stream_urls: { main:string; sub:string; has_sub:boolean }`. Define `ViewerReport`/`PathViewers`/`ClientView` to match `streams.rs` (main/sub each `{ total, capacity, clients: ClientView[] }`; `ClientView { label, client_type, remote_addr, user_agent, connected_at }`). (Confirm exact field names against `system.rs`/`streams.rs` while implementing.)

- [ ] **Step 3: `hooks/useAuth.ts`.**
```tsx
import { useQuery } from "@tanstack/react-query"
import { apiGet, type Me } from "@/lib/api"
export function useMe() {
  return useQuery({ queryKey: ["me"], queryFn: () => apiGet<Me>("/api/me"), retry: false })
}
```
(`/api/me` returns 401 when logged out; `apiGet` throws → `isError`. `AuthGate` treats error/`!authenticated` as logged-out.)

- [ ] **Step 4: `components/AuthGate.tsx`.** Renders a `<Skeleton>`/spinner while `isLoading`; if error or `!data.authenticated` → `<Navigate to="/login" replace />`; else render `children`. If `data.setup_required` → `<Navigate to="/login" />` too (setup handled by Askama `/setup` for now — the login page can link there).

- [ ] **Step 5: (Sidebar DEFERRED.)** The dashboard is full-width (no sidebar) per the user decision, so this slice does NOT build `Sidebar.tsx`. Skip it — the `AppShell` renders topbar + full-width `<Outlet/>` only. The sidebar (nav list, admin-gating, active highlighting, mobile drawer) will be built in the first slice that adds a sidebar-bearing page. Do not create `Sidebar.tsx` now.

- [ ] **Step 6: `components/PowerDialog.tsx`.** shadcn `<Dialog>` with three actions (`restart_service`, `restart_device`, `shutdown_device`) each calling `apiPost("/api/power", { action })`; show a toast/confirmation. Admin-only (only mounted when `is_admin`).

- [ ] **Step 7: `components/Topbar.tsx`.** Brand "OctoCam" (links `/`), a live status chip (from `useStatus()` — the `services.rtsp.state`, added in Task 4's hook; for this task a placeholder that fills in once Dashboard's query exists is fine, or fetch `/api/status` here too), a settings gear link (`/settings` Askama for now), the `<PowerDialog>` trigger (admin), and a Logout button → `apiPost("/api/logout", {})` then `queryClient.clear()` + navigate to `/login`.

- [ ] **Step 8: `components/AppShell.tsx`.** Topbar-only full-width layout (NO sidebar this slice): a column with `<Topbar>` at top and `<Outlet/>` filling the width below, on `--background`. (When the first sidebar-bearing page lands in a later slice, AppShell grows a sidebar column conditionally.)

- [ ] **Step 9: `routes/Login.tsx` (with passkey).** Centered `<Card>` with username (default "admin") + password inputs, submit → `apiPost("/api/login", {username, password})`; on success `queryClient.invalidateQueries({queryKey:["me"]})` + navigate to `/`; on error show the message. Link to Askama `/setup` if setup needed.
  **Passkey login (included this slice):** port the flow from `login.html`'s inline script (read it). Add a "Sign in with a passkey" button (hidden if `!window.PublicKeyCredential`): `POST /api/passkey/login/start` → decode base64url `publicKey.challenge`/`allowCredentials[].id` → `navigator.credentials.get({publicKey})` → re-encode binary fields to base64url → `POST /api/passkey/login/finish` with `{challenge_id, id, rawId, response:{clientDataJSON, authenticatorData, signature, userHandle}}` → on `{success}` invalidate `["me"]` + navigate to `/`. Also run conditional-mediation autofill on mount (`mediation:"conditional"`, silently no-op if unsupported). Swallow `NotAllowedError`/`AbortError`; surface other errors. Put the base64url helpers in `lib/webauthn.ts`.

- [ ] **Step 10: `App.tsx` route table + `main.tsx`.**
```tsx
// App.tsx
<Routes>
  <Route path="/login" element={<Login/>} />
  <Route element={<AuthGate><AppShell/></AuthGate>}>
    <Route path="/" element={<Dashboard/>} />
  </Route>
  <Route path="*" element={<Navigate to="/" replace/>} />
</Routes>
```
`main.tsx`: wrap in `<QueryClientProvider>` + `<BrowserRouter basename="/app">`.
(Use a placeholder `<Dashboard/>` — "Dashboard coming in Task 4" — so this task is testable on its own.)

- [ ] **Step 11: Build + verify locally.**
```bash
cd frontend && npm run build 2>&1 | tail -5
```
Then run the dev server (preview tooling) and confirm: `/app/login` renders the themed login card; logging in is blocked without a backend, but the route/render works; `/app/` with no session redirects to `/app/login`. (Full auth verified on-Pi in Task 4.)

- [ ] **Step 12: Commit.**
```bash
cd .. && git add frontend && git commit -m "feat(ui): React app shell, router, auth gate, and login page"
```

---

## Task 4: Dashboard page (stream preview + health + clients) + on-Pi verification

**Files:** Create `frontend/src/routes/Dashboard.tsx`, `components/dashboard/{StreamPreview,StreamHealthCard,ClientsCard}.tsx`; add a `useStatus()` hook (in `hooks/useAuth.ts` or a new `hooks/useStatus.ts`).

**Interfaces:** Consumes `/api/status` (with `browser_stream_urls` from Task 2), Task 1 theme, Task 3 shell.

- [ ] **Step 1: `useStatus()` hook** — `useQuery({ queryKey:["status"], queryFn:()=>apiGet<Status>("/api/status"), refetchInterval:5000 })`. Used by the dashboard cards and the topbar status chip.

- [ ] **Step 2: `StreamPreview.tsx`.** Props/state:
  - Read `browser_stream_urls {main, sub, has_sub}` from `useStatus()`.
  - Local state `{ activeStream: "main"|"sub", playing: boolean }`, initialized from `localStorage["octocam.streamPreview"]` (default `{activeStream: has_sub ? "sub" : "main", playing: false}` — default STOPPED to avoid auto-starting a WebRTC session on the Pi on every load), persisted on change.
  - Render: segmented control HD(`main`)/SD(`sub`) (SD disabled if `!has_sub`), a Start/Stop toggle, an `<iframe class="live-video">` whose `src` = `activeSource()` when playing else `about:blank`, and a "Preview stopped" placeholder overlay when not playing.
  - HD-capacity fallback: compute `mainIsFull` from `status.viewers?.main` (`main.total >= main.capacity`); if the user picks HD while full and `has_sub`, redirect to `sub` and show a small "HD at capacity" note (port `mainIsFull()` logic).
  - RTSP button: link to Askama `/rtsp` for now.
- [ ] **Step 3: `StreamHealthCard.tsx`.** shadcn `<Card>` with a 2×2 metric grid from `useStatus()`: Service = `services.rtsp.state`; Camera = `camera.available ? "Camera online" : (camera.message||"Camera unavailable")`; Uptime = `uptime ?? "—"`; Web UI = `services.octocam_web.state`. A live status pill reflecting `services.rtsp.state` (green when `active`). Use `<Skeleton>` while loading; "device unreachable" state on error.
- [ ] **Step 4: `ClientsCard.tsx`.** For `main` and `sub`: a row "HD Stream"/"SD Stream" with a `{total} / {capacity}` badge from `status.viewers`, expandable (`<details>`/shadcn accordion) to list `viewers.{path}.clients` as rows (`label`, `client_type`, `remote_addr`). "No clients connected." when empty; "unavailable" when `viewers` is null.
- [ ] **Step 5: `Dashboard.tsx`.** Compose an `<h1>Dashboard</h1>` + a responsive grid: `<StreamPreview>` (main area) beside `<StreamHealthCard>` + `<ClientsCard>` (right column), matching the current layout intent. Wire the topbar status chip to `useStatus()` too.
- [ ] **Step 6: Build + local visual check.**
```bash
cd frontend && npm run build 2>&1 | tail -5
```
Run dev server; confirm the dashboard renders themed (terracotta primary, system font), cards show skeleton→"device unreachable" without a backend, and the preview toggle flips the iframe/placeholder.
- [ ] **Step 7: Deploy + on-Pi verification.**
```bash
cd /Users/soham/GitRepos/OctoCam && scripts/deploy-pi-web.sh
```
Then in a browser (you, logged in): open `https://octocam.local/app`, log in with admin creds, and confirm: the themed shell renders with sidebar nav; the dashboard shows live Service/Camera/Uptime/Web UI and client counts; toggling HD/SD + Start plays the mediamtx preview; logout returns to `/app/login`. Controller (me) verifies unauthenticated `/app` → login redirect and that `/api/status` now returns `browser_stream_urls` via `curl` on the Pi.
- [ ] **Step 8: Commit.**
```bash
git add frontend && git commit -m "feat(ui): dashboard page (stream preview, health, clients) on /app"
```

---

## Self-Review

**Spec coverage (Phase 3 slice 1):** themed shell ✅ (T1+T3), auth gate + login ✅ (T3), dashboard with stream preview/health/clients ✅ (T4), backend stream-urls gap ✅ (T2). Terminal excluded ✅. Passkey login, and migrating the other pages (identity/wifi/settings/etc.), are explicitly later slices.

**Placeholder scan:** No TBD steps. T3 field-shape and T4 viewer-shape steps say "confirm against `system.rs`/`streams.rs`" — that's real verification against source, not a placeholder; exact field names are pinned in the referenced files.

**Type consistency:** `useMe`/`Me`, `useStatus`/`Status`, `apiGet`/`apiPost`, `AuthGate`/`AppShell` names consistent across tasks. `browser_stream_urls` field name identical between Task 2 (backend) and Task 4 (frontend consumer).

**Risk notes:** (1) The default-STOPPED preview is deliberate — auto-starting WebRTC on every dashboard load would tax the Pi Zero and consume a viewer slot. (2) The iframe loads mediamtx from `:8889` (different port, same host) — confirm nginx/CORS allow it (the current Askama dashboard already does exactly this, so it should already work). (3) `react-router-dom` v7 adds a modest bundle increment — acceptable, watch the size baseline from Phase 1.
