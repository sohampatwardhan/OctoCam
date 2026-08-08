# UI Overhaul — Phase 3 (slice 5): Settings-Family (Identity + Stream-Settings + Account + Admin) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Implementers also use frontend-design. Steps use checkbox (`- [ ]`).

**Goal:** Migrate the last four Askama pages to React under `/app`, reaching FULL parity so Phase 4 (delete Askama + root-cutover) can follow. One additive backend endpoint (`GET /api/stream-options`) closes the resolution-preset + timezone-list gaps.

**Architecture:** Extends the SPA + shared hooks. Adds `/identity`, `/stream-settings`, `/settings` (Account), `/admin` routes. Reuses `useSettings`/`useUpdateSettings` (PUT partial patch), `useStatus`, the passkey base64url helpers from `lib/webauthn.ts` (login flow, slice 1) extended with a registration helper.

**Tech Stack:** React 19 + TS + Vite, shadcn/ui + Tailwind v4, TanStack Query, react-router-dom; Rust/Axum (one additive endpoint).

## Global Constraints

- **SPA stays at `/app`** (root cutover is Phase 4). New in-SPA routes: `/identity`, `/stream-settings`, `/settings`, `/admin`. This slice makes ALL sidebar pages in-app — after it, no sidebar item links out to Askama.
- **Account/Admin restructure (deliberate, parity-preserving):** legacy `/settings` behaves inconsistently by role and the admin page has a no-op username field + a never-implemented "all users' passkeys" view. The React version:
  - **`/settings` = Account** (any logged-in user): change own password (`PUT /api/settings {admin_password, admin_password_confirm}`) + manage own passkeys (list `/api/passkeys`, register via WebAuthn, rename, delete). Sidebar: show "Account" to EVERYONE (remove the current non-admin-only gate).
  - **`/admin` = User accounts / RBAC** (admin only): list/add/delete users (`/api/users*`). No username-rename UI (no backend support — omit it, don't build a no-op field). No cross-user passkey management (no backend support).
- **Settings writes** via `PUT /api/settings` (partial patch, native types, seeded merge). The `enforce_hksv_requires_motion` and `enforce_matter_requires_admin` invariants run server-side — the UI should reflect HKSV-requires-motion (disable HKSV when motion off) but the server is the real gate.
- **Theme tokens only**; auto dark mode already wired. Admin/Account are security-sensitive (passwords, users, passkeys) — review closely.
- **Rust:** additive only; `cargo build --release --locked`; keep `Cargo.lock` in sync.

## Backend gap this slice fills

`GET /api/stream-options` (admin-gated) → `{ resolution_presets: [{value,label}], sub_resolution_presets: [...], timezones: [string], rotations: [0,90,180,270] }`. Sources: `settings::preset_views()` (RESOLUTION_PRESETS/SUB_RESOLUTION_PRESETS, settings.rs:80-162,467-476), `time_zone_views()` (main.rs:3388, from `system::available_time_zones()` — dynamic, must be server-sourced), rotations hardcoded. This is the only new endpoint; everything else reuses existing APIs.

---

## File Structure

**Backend — Modify:** `rust/octocam-web/src/main.rs` (`api_stream_options` handler + route).

**Frontend — Create:** `frontend/src/routes/Identity.tsx`, `StreamSettings.tsx`, `Account.tsx`, `Admin.tsx`; `frontend/src/hooks/useUsers.ts`, `frontend/src/hooks/usePasskeys.ts`. **Modify:** `frontend/src/lib/api.ts` (extend `Settings` with all stream/image/motion/overlay fields; add `StreamOptions`, `UserDto`, `PasskeyDto` types), `frontend/src/lib/webauthn.ts` (add `registerPasskey` helper), `frontend/src/components/Sidebar.tsx` (Account for all; Identity/Stream Config/Admin `inApp:true`), `frontend/src/App.tsx` (4 routes).

---

## Task 1: Backend — `GET /api/stream-options`

**Files:** Modify `rust/octocam-web/src/main.rs`.

- [ ] **Step 1:** Read `stream_settings()` (main.rs:765-795) to see how `resolution_presets`/`sub_resolution_presets`/timezone views/rotations are built (`preset_views`, `time_zone_views` main.rs:3388, `rotation_views` main.rs:3378). Read `PresetView`/`TimeZoneView` shapes.
- [ ] **Step 2:** Add `api_stream_options` (admin-gated via `require_admin_login(..., true).map_err(|e| api::ApiError::internal(e.0))?`) returning `api::ok_json(json!({ "resolution_presets": preset_views(&RESOLUTION_PRESETS), "sub_resolution_presets": preset_views(&SUB_RESOLUTION_PRESETS), "timezones": <Vec<String> from available_time_zones + current>, "rotations": [0,90,180,270] }))`. Reuse the exact same builders `stream_settings()` uses so the lists match. Register `.route("/api/stream-options", get(api_stream_options))`.
- [ ] **Step 3:** Add a focused test (the DTO/response serializes with the four keys; presets non-empty). `cargo build` + `cargo test` clean. Commit: `feat(api): GET /api/stream-options (resolution presets + timezone list)`.

---

## Task 2: Identity page (`/app/identity`)

**Files:** Create `frontend/src/routes/Identity.tsx`; modify `lib/api.ts` (Settings fields), `Sidebar.tsx`, `App.tsx`.

- [ ] **Step 1:** Extend `Settings` type with `device_name:string, room:string, camera_label:string` (confirm names in settings.rs:20-22).
- [ ] **Step 2:** `Identity.tsx` — a card with three text inputs initialized from `useSettings()`; Save → `useUpdateSettings().mutate({device_name, room, camera_label})`; disable while pending; success/error. Skeleton/error. Read identity.html for labels/maxlengths (80 chars).
- [ ] **Step 3:** Sidebar Identity `inApp:true`; `App.tsx` `<Route path="/identity" element={<Identity/>}/>`. Build clean. Commit: `feat(ui): Identity page`.

---

## Task 3: Stream-settings page (`/app/stream-settings`) — the large one

**Files:** Create `frontend/src/routes/StreamSettings.tsx` (+ optional `components/stream/*`); modify `lib/api.ts` (many Settings fields + `StreamOptions` type), `Sidebar.tsx`, `App.tsx`.

- [ ] **Step 1: Types.** Extend `Settings` with (READ settings.rs for exact names/types/ranges): `camera_enabled:boolean`; HD `resolution:string` (a preset value string), `framerate:number` (1-60), `bitrate_kbps:number` (250-25000); SD `sub_stream_enabled:boolean, sub_resolution:string, sub_framerate:number` (1-30), `sub_bitrate_kbps:number` (150-5000); image `rotation:number` {0,90,180,270}, `contrast:number` (0-4 float), `brightness:number` (-100..100), `hflip:boolean, vflip:boolean, noir_mode:boolean`; motion `motion_enabled:boolean, motion_sensitivity:number` (1-100), `motion_zones:number` (u64 bitmask; use `number` — JS safe-int OK for 64 bits? NOTE: a full u64 exceeds JS safe int; but zone bitmask uses 64 cells → up to 2^64-1 which overflows Number. CONFIRM how the server serializes motion_zones — if it's a JSON number it may lose precision above 2^53. If so, represent as string and parse carefully, OR confirm zones fit in <=53 bits. READ settings.rs:61 + how it's serialized. This is a correctness risk — resolve in Step 1.); HKSV `hksv_enabled:boolean`; overlay `text_overlay_enabled:boolean, text_overlay_timezone:string, text_overlay_date_format:"dd/mm/yyyy"|"mm/dd/yyyy"|"yyyy-mm-dd", text_overlay_clock_format:"24h"|"12h"`; `time_server:string`. Add `StreamOptions {resolution_presets:{value,label}[]; sub_resolution_presets:{value,label}[]; timezones:string[]; rotations:number[]}`.
- [ ] **Step 2: Data.** `useQuery(["stream-options"], () => apiGet<StreamOptions>("/api/stream-options"))` for the selects; `useSettings()` for current values.
- [ ] **Step 3: Form sections** (match stream_settings.html): Camera enable; HD stream (resolution select from options, framerate, bitrate); SD stream (enable, resolution, framerate, bitrate); Image (rotation select, contrast, brightness, hflip/vflip/noir switches); Motion (enable, sensitivity, and the **8×8 zone grid** — a clickable 64-cell grid toggling bits of `motion_zones`; port the bit math from stream_settings.html:157-260); HKSV (enable — disabled when `!motion_enabled`, mirroring `enforce_hksv_requires_motion`); Overlay (enable, timezone select from options, date-format select, clock-format select). Save → `useUpdateSettings().mutate({...changed fields...})` with native typed values.
- [ ] **Step 4: Time sync.** A `time_server` input + a "Sync now" button → `apiPost("/api/time/sync", {time_server})` (persists + syncs); a plain Save persists via `PUT /api/settings`. Mirror the legacy two-action behavior.
- [ ] **Step 5:** Sidebar "Stream Config" `inApp:true`; `App.tsx` `<Route path="/stream-settings" element={<StreamSettings/>}/>`. Build clean; eyeball. Commit: `feat(ui): Stream settings page (camera, streams, image, motion zones, overlay, time)`.

> This is the biggest page — decompose into sub-components (`StreamSection`, `ImageSection`, `MotionSection`, `OverlaySection`) for reviewability. The **motion-zone bitmask precision** (Step 1) is the top correctness risk — resolve it before building the grid.

---

## Task 4: Account page (`/app/settings`) — password + own passkeys

**Files:** Create `frontend/src/routes/Account.tsx`, `frontend/src/hooks/usePasskeys.ts`; modify `lib/api.ts` (`PasskeyDto`), `lib/webauthn.ts` (registration helper), `Sidebar.tsx`, `App.tsx`.

- [ ] **Step 1: Types + webauthn.** `PasskeyDto {id:number; name:string; created_at:string; last_used_at:string|null}` (the fields the UI needs; `/api/passkeys` returns more — ignore the extra `credential_id`/`public_key` arrays). In `lib/webauthn.ts` add `registerPasskey(name:string): Promise<void>` porting admin.html:286-340: `POST /api/passkey/register/start {name}` → decode `publicKey.challenge` + `publicKey.user.id` base64url→ArrayBuffer → `navigator.credentials.create({publicKey})` → `POST /api/passkey/register/finish {challenge_id, id, rawId, response:{clientDataJSON, attestationObject}, name}` (encode binary base64url). Reuse the existing base64url helpers.
- [ ] **Step 2: `usePasskeys.ts`.** `usePasskeys()` = `useQuery(["passkeys"], () => apiGet<PasskeyDto[]>("/api/passkeys"))`. `useRegisterPasskey()` = mutation calling `registerPasskey`, invalidate `["passkeys"]`. `useRenamePasskey()` = `POST /api/passkey/{id}/rename {name}`. `useDeletePasskey()` = `DELETE /api/passkey/{id}` (no body). Invalidate on success.
- [ ] **Step 3: `Account.tsx`.** Two cards: **Password** (admin_password + confirm, client-side match check, → `useUpdateSettings().mutate({admin_password, admin_password_confirm})`; on success clear fields + toast) and **Passkeys** (list with name/created/last-used; a "Register a passkey" button → name prompt → `useRegisterPasskey`; per-row rename + delete; hide register button if `!window.PublicKeyCredential`). Available to ALL users. Skeleton/error.
- [ ] **Step 4:** Sidebar: change the "Account" item to show for EVERYONE (remove `hideForAdmin`) at `/settings`, `inApp:true`. `App.tsx` `<Route path="/settings" element={<Account/>}/>`. Build clean. Commit: `feat(ui): Account page (password + own passkeys)`.

---

## Task 5: Admin page (`/app/admin`) — user accounts / RBAC

**Files:** Create `frontend/src/routes/Admin.tsx`, `frontend/src/hooks/useUsers.ts`; modify `lib/api.ts` (`UserDto`), `Sidebar.tsx`, `App.tsx`.

- [ ] **Step 1: Types + hook.** `UserDto {id:number; username:string; role:"admin"|"viewer"; created_at:string}`. `useUsers.ts`: `useUsers()` = `useQuery(["users"], () => apiGet<UserDto[]>("/api/users"))`. `useAddUser()` = `POST /api/users/add {username, password, role}` (role default "viewer"; the response is `{success, error?}` — note this endpoint returns success:false WITH 200 in some paths per the inventory — check status vs body; surface `error`, which may be a raw rusqlite string like "UNIQUE constraint failed" → map duplicate-username to friendly copy). `useDeleteUser()` = `DELETE /api/users/{id}` (self-delete guard is server-side → surface "Cannot delete your own active user account"). Invalidate `["users"]`.
- [ ] **Step 2: `Admin.tsx`** (admin only): users table (username, role badge, created); add-user form (username, password, role select viewer/admin); per-row delete (disable/hide delete on the current user's own row — read current username from `useMe()` — plus the server guard as backstop). NO username-rename (no backend support). Friendly errors (duplicate username, etc.). Skeleton/error.
- [ ] **Step 3:** Sidebar "Admin" `inApp:true` (stays admin-only); `App.tsx` `<Route path="/admin" element={<Admin/>}/>`. Build clean. Commit: `feat(ui): Admin page (user accounts / RBAC)`.

---

## Task 6: Full-parity check + deploy + on-Pi verification

- [ ] **Step 1:** Confirm every sidebar nav item now resolves to an in-SPA route (no remaining `<a href>` to Askama except intentional externals). `grep` the Sidebar for `inApp:false`/plain links — there should be none left (terminal excluded, not in nav).
- [ ] **Step 2:** `cd frontend && npm run build` clean; full `cd rust/octocam-web && cargo test`.
- [ ] **Step 3: Deploy.** `cd /Users/soham/GitRepos/OctoCam && scripts/deploy-pi-web.sh`.
- [ ] **Step 4: Controller on-Pi verify:** `/app/identity`, `/app/stream-settings`, `/app/settings`, `/app/admin` serve 200; `GET /api/stream-options` 401-gated (route exists). Browser (user, logged in): each page renders + saves; stream-settings motion-zone grid toggles + persists; Account password change + passkey register works; Admin add/delete user works (careful not to delete your own account). This reaches **full parity** — every legacy page now has a React equivalent. Commit any fixes.

---

## Self-Review

**Spec coverage (slice 5 → full parity):** identity ✅ T2, stream-settings ✅ T3 (+backend T1), account/password+passkeys ✅ T4, admin/users ✅ T5, deploy+parity ✅ T6. After this, the only legacy-only page is terminal (intentionally excluded/removal candidate).

**Placeholder scan:** No TBD. The motion_zones precision question (T3 Step 1) is a real "resolve against source before building" item, not a placeholder.

**Type consistency:** `Settings` field names (large extension) must match settings.rs exactly; `StreamOptions` between T1 (backend) and T3 (consumer); `UserDto`/`PasskeyDto` match `/api/users`//api/passkeys`.

**Risk notes:** (1) **motion_zones is a u64 bitmask** — if the server serializes it as a JSON number >2^53 it loses precision in JS; resolve in T3 Step 1 (confirm bit count / use string if needed). (2) Account/Admin are security-sensitive — password change, user CRUD, WebAuthn registration; the self-delete guard and password-match must hold; review T4/T5 closely. (3) The `/settings`=Account restructure is a deliberate deviation from the confusing legacy split — it preserves every real capability and drops only a no-op username field + a non-existent admin-passkey view. (4) `useAddUser`/`useDeleteUser` may get `{success:false}` with a 200 or a raw DB error string — handle both and map to friendly copy.
