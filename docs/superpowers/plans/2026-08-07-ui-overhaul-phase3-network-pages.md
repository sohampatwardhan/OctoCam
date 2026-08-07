# UI Overhaul — Phase 3 (slice 2): Network Pages (Sidebar + Wi-Fi + Setup) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Implementers should also use frontend-design for the React UI. Steps use checkbox (`- [ ]`).

**Goal:** Migrate the Wi-Fi page and the first-run Setup wizard to React under `/app`, and build the persistent sidebar (Wi-Fi is the first sidebar-bearing page). One small additive backend endpoint exposes saved Wi-Fi profiles as JSON.

**Architecture:** Extends the slice-1 React SPA. Adds a `Sidebar` + makes `AppShell` render it on all authenticated routes except the full-width dashboard. Adds `/wifi` (authenticated admin page) and `/setup` (pre-auth wizard, outside `AuthGate`). Backend gains `GET /api/wifi/saved`.

**Tech Stack:** React 19 + TS + Vite, shadcn/ui + Tailwind v4, TanStack Query, react-router-dom v7; Rust/Axum (one additive endpoint).

## Global Constraints

- **SPA stays at `/app`** (root cutover is Phase 4). New in-SPA routes: `/` (dashboard, slice 1), `/wifi`, `/setup`. All OTHER sidebar nav items link to the existing Askama pages via absolute `<a href="/identity">` etc. (they leave the SPA) until their own slices land.
- **Sidebar built this slice; dashboard stays full-width.** `AppShell` renders the sidebar for every authenticated route EXCEPT `/` (the dashboard remains topbar-only full-width, per the slice-1 decision). Nav excludes **Terminal**.
- **Theme tokens only** (no raw hex / raw Tailwind palette). Auto dark mode is already wired (slice 1). Reuse `useMe()` for admin-gating nav, `useStatus()` for live Wi-Fi state.
- **Setup is PRE-AUTH** — `/app/setup` sits OUTSIDE `<AuthGate>`. When `GET /api/setup` reports `setup_required:false`, the setup route should redirect to `/` (setup isn't gated server-side; the SPA adds this).
- **`homekit_enabled` presence landmine (Setup):** `api_setup_post` treats the KEY'S PRESENCE as true (sending `false` still enables it). The React setup form MUST omit the key entirely when HomeKit is off, and include it (any value) when on.
- **Setup soft-failures are HTTP 200** with `{success:false, field, message}` — `apiPost` will NOT throw on them (only non-2xx throws). Branch on `body.success`, surfacing `field`/`message`; navigate only when `success:true`.
- **Wi-Fi active-network delete guard** is enforced server-side (400) — the UI should also hide/disable delete on the connected network, and reflect the server 400 message otherwise.
- **Rust:** additive only; `cargo build --release --locked`; keep `Cargo.lock` in sync.

## Backend gap this slice fills

`GET /api/wifi/saved` (admin-gated) → JSON array of saved profiles. Source: `system::stored_wifi_profiles(...)` (used by `wifi_page`, main.rs:~739). `StoredWifiProfile` (system.rs:99-107: `name, security, source, active, can_delete, delete_source`) — add `#[derive(Serialize)]` (or a DTO) and return the vec. The React saved-list sends `delete_source` as the `source` field to `DELETE /api/wifi/delete`.

---

## File Structure

**Backend — Modify:** `rust/octocam-web/src/main.rs` (new `api_wifi_saved` handler + route); `rust/octocam-web/src/system.rs` (derive `Serialize` on `StoredWifiProfile`, or a DTO in main.rs).

**Frontend — Create:** `frontend/src/components/Sidebar.tsx`; `frontend/src/routes/Wifi.tsx`, `frontend/src/routes/Setup.tsx`; `frontend/src/lib/wifi.ts` (signal %/level + password-validation helpers); `frontend/src/hooks/useWifi.ts` (queries/mutations). **Modify:** `frontend/src/components/AppShell.tsx` (conditional sidebar), `frontend/src/App.tsx` (routes), `frontend/src/lib/api.ts` (types: `SavedWifiProfile`, `WifiNetwork`, `WifiCache`, `WifiStatus` subset).

---

## Task 1: Backend — `GET /api/wifi/saved`

**Files:** Modify `rust/octocam-web/src/system.rs` (derive Serialize on `StoredWifiProfile`), `rust/octocam-web/src/main.rs` (handler + route).

**Interfaces:** Produces `GET /api/wifi/saved` → `[{name, security, source, active, can_delete, delete_source}]`, admin-gated.

- [ ] **Step 1:** Read `stored_wifi_profiles` usage in `wifi_page` (main.rs:~739) and the `StoredWifiProfile` struct (system.rs:99-107). Add `#[derive(Serialize)]` to `StoredWifiProfile` (it's plain `String`/`bool` fields — safe; confirm no secret fields). If it already derives Serialize, skip.
- [ ] **Step 2:** Add `api_wifi_saved`, mirroring the guard pattern used by the other wifi handlers (`require_admin_login(..., true).map_err(|e| api::ApiError::internal(e.0))?`):
```rust
async fn api_wifi_saved(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> ApiResult {
    if let Some(resp) = require_admin_login(&state, &headers, &uri, true).map_err(|e| api::ApiError::internal(e.0))? { return Ok(resp); }
    let profiles = run_blocking(system::stored_wifi_profiles).await.map_err(|e| api::ApiError::internal(e.to_string()))?;
    Ok(api::ok_json(profiles))
}
```
(Read `stored_wifi_profiles`' real signature — does it take args? is it sync (needs run_blocking) or async? Adjust. `.to_string()` on the join error, not `e.0`, since that's a JoinError not AppError.)
- [ ] **Step 3:** Register `.route("/api/wifi/saved", get(api_wifi_saved))` near the other `/api/wifi/*` routes.
- [ ] **Step 4:** Add a focused test (SavedWifiProfile/StoredWifiProfile serializes with expected fields incl. `delete_source`). `cargo build` + `cargo test`. Commit: `feat(api): expose saved Wi-Fi profiles on GET /api/wifi/saved`.

---

## Task 2: Sidebar + AppShell conditional layout

**Files:** Create `frontend/src/components/Sidebar.tsx`; modify `frontend/src/components/AppShell.tsx`.

**Interfaces:** Consumes `useMe()` (admin gating) and `react-router-dom` `NavLink`/`useLocation`.

- [ ] **Step 1: `Sidebar.tsx`** — a nav from a static list `{ label, to, adminOnly, inApp, icon }`, EXCLUDING Terminal:
  - Dashboard `/` (all users, `inApp:true`).
  - "Basic Settings" (admin): Identity `/identity`, Wi-Fi `/wifi` (`inApp:true`), Stream Config `/stream-settings`, RTSP `/rtsp`, HomeKit `/homekit`, Matter `/matter`.
  - "Advanced Settings" (admin): System info `/system`, System logs `/logs`, SSH keys `/ssh-keys`, Admin `/admin`.
  - Non-admin: Account Settings `/settings`.
  - `inApp:true` items → `<NavLink to>` (react-router, active styling via `isActive`). Others → `<a href>` (absolute, leaves SPA; no active state). Gate `adminOnly` on `useMe().data?.is_admin`. Section headers as small muted labels. lucide-react icons (Wifi, Home, Server, ScrollText, KeyRound, Shield, IdCard, Radio, Settings, SlidersHorizontal, LayoutDashboard — pick sensible ones). Active style with theme tokens: `bg-muted` + a left `inset` primary bar (`box-shadow: inset 4px 0 0 var(--primary)` → Tailwind `shadow-[inset_4px_0_0_var(--color-primary)]` or a `border-l-4 border-primary`).
- [ ] **Step 2: Mobile drawer** — under `md` (768px), the sidebar hides and a hamburger in the Topbar toggles it (off-canvas). Use a shadcn `Sheet` if available, else a simple state-driven overlay. Auto-close on nav click and on resize back to desktop. (Add the hamburger button to `Topbar.tsx`, shown only when a sidebar is present, i.e. not on `/`.)
- [ ] **Step 3: `AppShell.tsx`** — render the sidebar on every route EXCEPT `/`. Use `useLocation()`: `const withSidebar = pathname !== "/"`. When `withSidebar`, a two-column grid (`~228px` sidebar + `minmax(0,1fr)` content); else full-width (slice-1 behavior). Topbar spans the top in both.
- [ ] **Step 4:** Build clean; dev-server eyeball: `/` full-width (no sidebar), a temporary link to `/wifi` shows the sidebar with Wi-Fi active. Commit: `feat(ui): persistent sidebar with admin-gated nav (dashboard stays full-width)`.

---

## Task 3: Wi-Fi page (`/app/wifi`)

**Files:** Create `frontend/src/routes/Wifi.tsx`, `frontend/src/lib/wifi.ts`, `frontend/src/hooks/useWifi.ts`; modify `frontend/src/lib/api.ts` (types), `frontend/src/App.tsx` (route).

**Interfaces:** Consumes `/api/status` (`wifi` block), `GET /api/wifi/saved` (Task 1), `POST /api/wifi/scan`, `POST /api/wifi/connect`, `DELETE /api/wifi/delete`.

- [ ] **Step 1: Types + helpers.** In `lib/api.ts` add `WifiNetwork {ssid, security, raw_security, signal}`, `WifiCache {scanned_at, networks: WifiNetwork[]}`, `SavedWifiProfile {name, security, source, active, can_delete, delete_source}`, and extend `Status` with the `wifi` block subset needed (ssid, state, signal_dbm, ip_addresses, band, wifi_generation_label — from system.rs WifiStatus). In `lib/wifi.ts`: `signalPercent(dbm)` = clamp0-100 `((parseFloat(dbm)+100)/50)*100`; `signalLevel(pct)` → high≥67 / low≥34 / zero; `passwordMeetsCriteria(security, pw)` (open→true; wep→5|13 chars or 10|26 hex; else 8-63 chars) — port from app.js:493-546.
- [ ] **Step 2: `useWifi.ts`** — `useSavedWifi()` (query `/api/wifi/saved`), `useScanWifi()` (mutation POST `/api/wifi/scan` → WifiCache; also expose the cached `GET /api/wifi/networks` as the initial list), `useConnectWifi()` (mutation POST `/api/wifi/connect`, invalidate saved + status on success), `useDeleteWifi()` (mutation DELETE `/api/wifi/delete`, invalidate saved on success).
- [ ] **Step 3: Current-connection card** — from `useStatus().wifi`: SSID (or "Not connected"), a signal icon via `signalLevel(signalPercent(signal_dbm))`, IP (`ip_addresses`), band/generation label. Skeleton/error states.
- [ ] **Step 4: Saved-networks card** — list `useSavedWifi()`: each row `name` + `security · source`; if `active` show a "Connected" badge and NO delete; else if `can_delete` a delete button → confirm dialog (shadcn `Dialog`/`AlertDialog`) → `useDeleteWifi().mutate({name, source: delete_source})`; surface the server 400 message (esp. the active-network guard) as an inline error/toast.
- [ ] **Step 5: Add-network flow** — a `Dialog` with: a "Scan" button → `useScanWifi()`, a network `<select>` populated from the scan/cache (label each `{ssid} · {SECURITY} · {signal}%`), a manual-SSID input (overrides the select when non-empty), a security `<select>` (open/wep/wpa2/wpa2-wpa3/wpa3, default wpa2), a password input (hidden when security=open), Save disabled until SSID present + `passwordMeetsCriteria`. Submit → `useConnectWifi().mutate({ssid, password, security})`; on success close + toast the returned message; on 400 show the error.
- [ ] **Step 6: `Wifi.tsx`** composes the three cards; add `<Route path="/wifi" element={<Wifi/>}/>` inside the AuthGate/AppShell group in `App.tsx`. Build clean; dev-server eyeball (cards show skeleton→error without backend). Commit: `feat(ui): Wi-Fi page (status, saved networks, scan + connect)`.

---

## Task 4: Setup wizard (`/app/setup`) + on-Pi verification

**Files:** Create `frontend/src/routes/Setup.tsx`; modify `frontend/src/App.tsx` (public route), `frontend/src/lib/api.ts` (setup types).

**Interfaces:** Consumes `GET /api/setup` (`{setup_required}`), `POST /api/setup` (JSON body), `POST /api/wifi/scan` (optional in-wizard scan).

- [ ] **Step 1: Route wiring** — add `<Route path="/setup" element={<Setup/>}/>` OUTSIDE `<AuthGate>` (public, like `/login`). In `Setup.tsx`, first `useQuery(["setup"], GET /api/setup)`; if `!setup_required` → `<Navigate to="/" replace/>` (setup already done). Update `Login.tsx`'s "setup needed" link to point at `/setup` (SPA route) instead of the Askama page.
- [ ] **Step 2: The form** (single page, matches the Askama "wizard"): Identity (`device_name`, `room`, `camera_label`), Network (`wifi_ssid` — manual input or a select if a scan was run; optional `wifi_password`), Admin (`admin_username` default "admin", `admin_password`, `admin_password_confirm`), Stream (`resolution` select, `framerate`, `rtsp_path`), HomeKit (`homekit_enabled` checkbox). Include the fixed hidden sub-stream fields the Askama form sends (`camera_enabled:"on"` is injected server-side, but `sub_stream_enabled`, `sub_resolution:"640x480"`, `sub_framerate:10`, `sub_bitrate_kbps:600`, `sub_rtsp_path:"sub"`, `sub_rtsp_max_clients:2` should be sent — read setup.html for the exact set).
- [ ] **Step 3: Submit** — build a JSON object of all fields. **CRITICAL: only include `homekit_enabled` in the body when the checkbox is checked** (omit the key entirely when off — presence = true server-side). Client-side check `admin_password === admin_password_confirm` before sending (nicer UX), but ALSO handle the server soft-failures: `const r = await apiPost<SetupResult>("/api/setup", body)` where `SetupResult = {success:true} | {success:false, field, message}` — remember these come back as HTTP 200, so `apiPost` returns the body without throwing. If `!r.success` → show the error against `r.field` (`admin_password_confirm` or `wifi`). If `r.success` → the session cookie is already set by the response; `queryClient.clear()` (or refetch `["me"]`) and `navigate("/")`.
- [ ] **Step 4: Build + local check.** Build clean; dev-server: `/app/setup` renders the form; validation disables submit appropriately.
- [ ] **Step 5: Deploy + on-Pi verification.**
```bash
cd /Users/soham/GitRepos/OctoCam && scripts/deploy-pi-web.sh
```
Controller verifies on the Pi: `GET /api/wifi/saved` returns 401 unauth (route exists); `/app/wifi` and `/app/setup` serve the SPA (200). Browser (user, logged in): `/app/wifi` shows current connection + saved networks + scan/connect works; the sidebar renders with Wi-Fi active and Dashboard full-width; `/app/setup` redirects to `/` since setup is complete. Commit: `feat(ui): first-run Setup wizard on /app/setup`.

---

## Self-Review

**Spec coverage (slice 2):** sidebar ✅ (T2), Wi-Fi page ✅ (T3 + backend T1), Setup wizard ✅ (T4). Terminal excluded from nav ✅. Other pages still link to Askama ✅.

**Placeholder scan:** No TBD steps. "Read X to confirm" items point at concrete source (stored_wifi_profiles signature, setup.html hidden fields) — real verification, not placeholders.

**Type consistency:** `SavedWifiProfile` field names (esp. `delete_source`) consistent between T1 (backend) and T3 (frontend consumer + delete call). `useStatus`/`Status.wifi`, `useMe`/admin gating consistent with slice 1.

**Risk notes:** (1) the `homekit_enabled` presence landmine and (2) the setup soft-failure-is-200 handling are the two most likely correctness misses — both called out explicitly in constraints and T4 Step 3. (3) `stored_wifi_profiles` may be sync or take args — confirm before wrapping in `run_blocking`. (4) Sidebar mobile drawer needs the Topbar hamburger, which slice 1 built topbar-only without — T2 Step 2 adds it, shown only when a sidebar is present.
