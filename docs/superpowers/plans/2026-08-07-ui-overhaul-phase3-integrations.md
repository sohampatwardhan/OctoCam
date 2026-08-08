# UI Overhaul — Phase 3 (slice 3): Integrations (RTSP + HomeKit + Matter) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Implementers also use frontend-design for the React UI. Steps use checkbox (`- [ ]`).

**Goal:** Migrate the RTSP, HomeKit, and Matter pages to React under `/app`, at full parity with the Askama versions (including their enable/settings toggles and the Matter reset), wiring them into the sidebar. One additive backend fix (`qr_payload` on `/api/matter`).

**Architecture:** Extends the slice-1/2 SPA. Adds a shared settings hook (`useSettings` read `/api/settings` + `useUpdateSettings` → `PUT /api/settings`) reused by all three pages (and future settings pages). Adds `/rtsp`, `/homekit`, `/matter` routes inside AuthGate+AppShell (sidebar). QR: HomeKit is an `<img>` data-URI; Matter is a raw SVG via `dangerouslySetInnerHTML`.

**Tech Stack:** React 19 + TS + Vite, shadcn/ui + Tailwind v4, TanStack Query, react-router-dom v7; Rust/Axum (one additive line).

## Global Constraints

- **SPA stays at `/app`** (root cutover is Phase 4). New in-SPA routes: `/rtsp`, `/homekit`, `/matter`. Mark these three `inApp:true` in the Sidebar (they're currently external `<a href>` links).
- **These pages are admin-only** — they sit inside AuthGate+AppShell (sidebar), use admin-gated APIs (`/api/rtsp`, `/api/homekit`, `/api/matter`, `/api/settings`). A non-admin who reaches them gets API 401s (graceful).
- **Settings writes go through `PUT /api/settings`** (Phase 2 Task 6): it seeds a full map from CURRENT settings then overlays the fields you send, runs `validate_map` (accepts NATIVE JSON booleans/numbers — NOT the setup endpoint's presence-semantics) → `enforce_matter_requires_admin`/`enforce_hksv_requires_motion` → save → side-effects. So: send only the changed fields with real typed values (e.g. `{ "matter_enabled": false }`), and re-fetch `/api/settings` (+ the page's own endpoint) after a successful save.
- **Matter enable toggle** is disabled client-side when `admin_password_set` is false (mirror the Askama `disabled` gate); the server also enforces `enforce_matter_requires_admin`.
- **QR rendering:** HomeKit → `<img src={qr_data_url}>` (data-URI passthrough, gated on `has_qr`). Matter → `<div dangerouslySetInnerHTML={{__html: qr_svg}}>` (raw server SVG string). No client QR library.
- **Matter reset** (`POST /api/matter/reset`) — the Askama form has NO confirmation; the React version SHOULD add a confirm `Dialog` (improvement, not a regression). After a successful reset, re-fetch `/api/matter` (reset rotates the QR/manual code).
- **Theme tokens only**; auto dark mode already wired. Reuse the copy-to-clipboard affordance where the legacy page has it (RTSP URLs); optional elsewhere.
- **Rust:** additive only; `cargo build --release --locked`; keep `Cargo.lock` in sync.

## Backend gap this slice fills

`/api/matter` (`main.rs:~2375-2388`) omits `qr_payload`, which the Askama template renders under the QR (`matter.html:51`). `MatterView.qr_payload` exists (matter.rs:322,361). Add `"qr_payload": view.qr_payload,` to the JSON block. (Not a secret — it's the same onboarding payload the QR encodes.)

---

## File Structure

**Backend — Modify:** `rust/octocam-web/src/main.rs` (`api_matter` JSON block: add `qr_payload`).

**Frontend — Create:** `frontend/src/hooks/useSettings.ts` (`useSettings`, `useUpdateSettings`); `frontend/src/components/CopyButton.tsx`; `frontend/src/routes/Rtsp.tsx`, `Homekit.tsx`, `Matter.tsx`. **Modify:** `frontend/src/lib/api.ts` (add `apiPut`; types `Settings` subset, `RtspUrls`, `HomeKitInfo`, `MatterInfo`), `frontend/src/components/Sidebar.tsx` (rtsp/homekit/matter → `inApp:true`), `frontend/src/App.tsx` (three routes).

---

## Task 1: Backend — add `qr_payload` to `/api/matter`

**Files:** Modify `rust/octocam-web/src/main.rs`.

- [ ] **Step 1:** In `api_matter`'s `serde_json::json!({...})` block (main.rs:~2375-2388), add `"qr_payload": view.qr_payload,` alongside `manual_code`/`qr_svg`. Confirm `MatterView.qr_payload` field name at matter.rs:322.
- [ ] **Step 2:** Update/extend the existing `api_matter` DTO/shape test (or add one) to assert `qr_payload` is present. `cargo build` + `cargo test`. Commit: `fix(api): include qr_payload in /api/matter response`.

---

## Task 2: Shared settings hook + RTSP page

**Files:** Create `frontend/src/hooks/useSettings.ts`, `frontend/src/components/CopyButton.tsx`, `frontend/src/routes/Rtsp.tsx`; modify `frontend/src/lib/api.ts`, `Sidebar.tsx`, `App.tsx`.

**Interfaces:** Produces `useSettings()`, `useUpdateSettings()`, `<CopyButton value>`, `apiPut`. Consumes `/api/settings` (GET+PUT), `/api/rtsp`.

- [ ] **Step 1: `apiPut` + types.** In `lib/api.ts` add `apiPut<T>(path, body)` (identical to `apiPost` but `method:"PUT"`; reuse the shared error-parse — consider factoring `apiPost`/`apiPut`/`apiDelete` onto one helper to resolve the earlier DRY note). Add a `Settings` type covering the fields these pages read/write: at least `rtsp_enabled:boolean, rtsp_path:string, rtsp_max_clients:number, homekit_enabled:boolean, matter_enabled:boolean` (read `settings::Settings`/`public_settings` in settings.rs for exact names/types; include only what this slice needs). Add `RtspUrls {main,sub,has_sub}`.
- [ ] **Step 2: `useSettings.ts`.** `useSettings()` = `useQuery(["settings"], () => apiGet<Settings>("/api/settings"))`. `useUpdateSettings()` = `useMutation((patch: Partial<Settings>) => apiPut<{success:boolean; settings:Settings}>("/api/settings", patch), { onSuccess: invalidate ["settings"] + ["status"] })`. (PUT merges the patch onto current server settings — send only changed fields.)
- [ ] **Step 3: `CopyButton.tsx`.** A small button that copies a string via `navigator.clipboard.writeText` (with a `document.execCommand` fallback), showing a brief "Copied" state. Theme tokens.
- [ ] **Step 4: `Rtsp.tsx`.** Admin page: a settings card with `rtsp_enabled` switch, `rtsp_path` input, `rtsp_max_clients` number — initialized from `useSettings()`, saved via `useUpdateSettings().mutate({rtsp_enabled, rtsp_path, rtsp_max_clients})` (Save button; disable while pending; show success/error). A URLs card showing `main` (and `sub` if `has_sub`) from `useQuery(["rtsp"], GET /api/rtsp)`, each with a `<CopyButton>`, shown only when `rtsp_enabled`. Skeleton/error states. Read `rtsp.html` for exact labels/layout.
- [ ] **Step 5: Wire up.** Sidebar: set RTSP item `inApp:true`. `App.tsx`: `<Route path="/rtsp" element={<Rtsp/>}/>` inside AuthGate+AppShell.
- [ ] **Step 6:** `npm run build` clean; dev-server eyeball. Commit: `feat(ui): RTSP page (settings form + stream URLs) + shared settings hook`.

---

## Task 3: HomeKit page

**Files:** Create `frontend/src/routes/Homekit.tsx`; modify `lib/api.ts` (HomeKitInfo type), `Sidebar.tsx`, `App.tsx`.

**Interfaces:** Consumes `/api/homekit`, `/api/status` (`services.homekit.state`), `useSettings`/`useUpdateSettings` (Task 2).

- [ ] **Step 1: Type.** `HomeKitInfo {status:string, paired:boolean, has_pairing:boolean, pincode:string, setup_uri:string, has_qr:boolean, qr_data_url:string, error:string, has_error:boolean}` (match `api_homekit` at main.rs:~2341-2351). Extend `Status.services` with `homekit: {state:string}` (read system.rs `Services`).
- [ ] **Step 2: `Homekit.tsx`.** `useQuery(["homekit"], GET /api/homekit)`. Render, matching `homekit.html`:
  - Status list: **Service** = `useStatus().data?.services.homekit.state` (NOT from /api/homekit — from /api/status; polled 5s via the shared status query), **Pairing** = `paired?"paired":"not paired"`, **Accessory** = `status`.
  - Enable toggle: `homekit_enabled` switch from `useSettings()`, saved via `useUpdateSettings().mutate({homekit_enabled})`.
  - If `has_error` → error box (`error`).
  - Unpaired + `has_pairing`: `<img src={qr_data_url}>` (only if `has_qr`), manual `pincode`, and `setup_uri` in small text.
  - Unpaired + !has_pairing: "starting" placeholder. Disabled state: prompt. Paired: static confirmation.
  - Skeleton/error states.
- [ ] **Step 3: Wire up.** Sidebar HomeKit `inApp:true`; `App.tsx` `<Route path="/homekit"/>`.
- [ ] **Step 4:** Build clean; eyeball. Commit: `feat(ui): HomeKit pairing page`.

---

## Task 4: Matter page + on-Pi verification

**Files:** Create `frontend/src/routes/Matter.tsx`; modify `lib/api.ts` (MatterInfo type incl. `qr_payload`), `Sidebar.tsx`, `App.tsx`.

**Interfaces:** Consumes `/api/matter` (now with `qr_payload` from Task 1), `POST /api/matter/reset`, `useSettings`/`useUpdateSettings`.

- [ ] **Step 1: Type.** `MatterInfo {status, commissioned:boolean, fabric_count:number, orphaned_fabrics:boolean, manual_code:string, qr_svg:string, qr_payload:string, stream_source:string, error:string, has_error:boolean, ipv6_ok:boolean, admin_password_set:boolean, snapshot_endpoint_down:boolean}` (match `api_matter` main.rs:~2375-2388 + Task 1's `qr_payload`).
- [ ] **Step 2: `Matter.tsx`.** `useQuery(["matter"], GET /api/matter)`. Render, matching `matter.html`:
  - If `!admin_password_set` → warning banner AND the enable toggle is `disabled` (client gate).
  - Enable toggle: `matter_enabled` from `useSettings()` → `useUpdateSettings().mutate({matter_enabled})` (disabled when `!admin_password_set`); on save, also invalidate `["matter"]`.
  - Status list: Accessory=`status`, Commissioned=`commissioned` (+ `fabric_count` with `!= 1` pluralization), stream source=`stream_source`.
  - Warnings: `!ipv6_ok`, `snapshot_endpoint_down`, `orphaned_fabrics` (with fabric_count), `has_error`→`error`.
  - Enabled + `manual_code` non-empty: QR via `<div dangerouslySetInnerHTML={{__html: qr_svg}}>`, manual `manual_code`, and `qr_payload` in small text.
  - **Reset:** a "Reset Matter pairing" button → confirm `Dialog` → `apiPost("/api/matter/reset", {})` → on success invalidate `["matter"]` (rotates QR/code) + toast/banner. (Legacy had no confirm; adding one is an intentional improvement.)
  - Disabled state: prompt. Skeleton/error states.
- [ ] **Step 3: Wire up.** Sidebar Matter `inApp:true`; `App.tsx` `<Route path="/matter"/>`.
- [ ] **Step 4:** Build clean; eyeball (QR/reset render; toggle disabled when admin password unset — simulate via mock or reason). Commit: `feat(ui): Matter commissioning page (status, QR, reset)`.
- [ ] **Step 5: Deploy + on-Pi verification.**
```bash
cd /Users/soham/GitRepos/OctoCam && scripts/deploy-pi-web.sh
```
Controller verifies on the Pi: `/app/rtsp`, `/app/homekit`, `/app/matter` serve (200); `/api/matter` (unauth 401 route exists; and confirm it now includes `qr_payload` — check via an authenticated curl if creds available, else confirm the field is in the built binary/source). Browser (user, logged in): each page renders with the sidebar (RTSP/HomeKit/Matter active); RTSP URLs + copy work and the enable/path save persists; HomeKit shows pairing state + QR; Matter shows status + QR + reset (with confirm). Commit any verification fixes.

---

## Self-Review

**Spec coverage (slice 3):** RTSP (settings form + URLs) ✅ T2, HomeKit (pairing + service state + toggle) ✅ T3, Matter (status + QR + toggle + reset) ✅ T4, backend `qr_payload` gap ✅ T1, sidebar wiring ✅ (T2-4). Full parity incl. enable toggles and Matter reset.

**Placeholder scan:** No TBD. "Read X for exact fields/labels" items point at concrete source (settings.rs field names, the three templates) — real verification.

**Type consistency:** `Settings` subset field names consistent between `useSettings`/`useUpdateSettings` and each page's toggle; `MatterInfo.qr_payload` consistent between T1 (backend) and T4 (consumer); `HomeKitInfo`/`MatterInfo` match the `api_homekit`/`api_matter` JSON.

**Risk notes:** (1) `PUT /api/settings` is a seeded full-map merge — sending only changed fields is correct and intended; DO NOT send a partial that drops others expecting them to persist by omission (they persist because the server seeds from current). (2) Native-boolean settings values are fine here (validate_map), unlike the setup endpoint — do not reintroduce presence-semantics. (3) Matter `qr_svg` via `dangerouslySetInnerHTML` is server-generated trusted content (our own `qrcode` crate output), acceptable. (4) Matter enable is gated by `admin_password_set` client + server; keep both.
