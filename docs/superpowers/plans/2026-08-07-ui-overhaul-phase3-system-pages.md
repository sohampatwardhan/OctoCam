# UI Overhaul — Phase 3 (slice 4): System Pages (System info + Logs + SSH keys) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Implementers also use frontend-design. Steps use checkbox (`- [ ]`).

**Goal:** Migrate the System info, Logs, and SSH keys pages to React under `/app` at full parity, wiring them into the sidebar. One additive backend endpoint (`POST /api/restore`, JSON) closes the only gap.

**Architecture:** Extends the slice-1/2/3 SPA + shared hooks (`useSettings`/`useUpdateSettings`, `useStatus`, `CopyButton`). Adds `/system`, `/logs`, `/ssh-keys` routes inside AuthGate+AppShell. Reuses `lib/wifi.ts` signal helpers (slice 2) for the system status Wi-Fi rows.

**Tech Stack:** React 19 + TS + Vite, shadcn/ui + Tailwind v4, TanStack Query, react-router-dom v7; Rust/Axum (one additive endpoint).

## Global Constraints

- **SPA stays at `/app`.** New in-SPA routes: `/system`, `/logs`, `/ssh-keys`; mark those three `inApp:true` in the Sidebar. Admin-only (inside AuthGate+AppShell; APIs admin-gated).
- **Do NOT duplicate power actions** — restart/reboot/shutdown already live in the topbar `PowerDialog` (slice 1). The System page does not render them.
- **Out of scope (other pages/slices):** device-name (`/identity`), time_server + timezone (`/stream-settings`). Do not build those here. The timezone-list backend gap is deferred with them.
- **Settings writes** (scheduled maintenance) go through `PUT /api/settings` via the existing `useUpdateSettings` — send the per-day boolean keys (`scheduled_service_restart_day_mon` … `_sun`, and the reboot equivalents) as real booleans (NOT a CSV; the server re-serializes). Send only changed fields.
- **Backup** = a plain authenticated download link `<a href="/backup">` (cookie auth carries; no endpoint needed). **Restore** goes through the NEW `POST /api/restore` (multipart in, JSON out) — with a confirm dialog (destructive).
- **Logs** = fixed last-40-lines snapshot from `GET /api/logs` (`{lines:[]}`), polled ~5s. Real streaming/tailing is explicitly deferred (not implemented server-side).
- **SSH last-key flow:** `DELETE /api/ssh-keys` returns `409` when removing the last key; the UI treats 409 as the trigger for an "are you sure — this ends root SSH" confirm, then resends with `confirm:true`.
- **Reuse** `lib/wifi.ts` `signalPercent`/`signalLevel` (slice 2) for the Wi-Fi signal row; re-derive memory/swap % client-side (clamp 0-100). Theme tokens only; auto dark mode already wired.
- **Rust:** additive only; `cargo build --release --locked`; keep `Cargo.lock` in sync.

## Backend gap this slice fills

`POST /restore` (`main.rs:~1035-1093`, `restore_upload`) is multipart-in / redirect-out — a `fetch()` SPA can't read the result. Add `POST /api/restore`: same multipart body (`backup` file), same `cross_origin()` CSRF guard, same `MAX_RESTORE_BYTES` (256 KiB) cap, same `backup::restore(...)` application logic — but return JSON: `200 {success:true, keys_added:N, keys_failed:M}` on success, `4xx {error, code}` for the failure cases (`invalid`/`too_large`/`empty`/`csrf`). Reuse the existing `backup` module logic; do not reimplement restore semantics.

---

## File Structure

**Backend — Modify:** `rust/octocam-web/src/main.rs` (`api_restore` handler + route). Possibly reference `rust/octocam-web/src/backup.rs` (restore logic).

**Frontend — Create:** `frontend/src/routes/System.tsx`, `Logs.tsx`, `SshKeys.tsx`; `frontend/src/hooks/useSshKeys.ts`; small helpers as needed. **Modify:** `frontend/src/lib/api.ts` (add `apiUpload` for multipart; types: extend `Status`/`SystemStatus` subset, `Settings` maintenance fields, `SshKeyDto`, `LogsResponse`, `RestoreResult`), `Sidebar.tsx` (3 items `inApp:true`), `App.tsx` (3 routes).

---

## Task 1: Backend — `POST /api/restore` (JSON)

**Files:** Modify `rust/octocam-web/src/main.rs`.

**Interfaces:** Produces `POST /api/restore` (multipart `backup` file → JSON), admin-gated + CSRF-guarded.

- [ ] **Step 1:** Read `restore_upload` (main.rs:~1035-1093), `MAX_RESTORE_BYTES` (~1033), `cross_origin` (~1139), and `backup::restore` (backup.rs). Understand the multipart parse, the size cap, the CSRF check, and the restore-application result (how many keys added/failed, and the error cases).
- [ ] **Step 2:** Add `api_restore` mirroring `restore_upload`'s guards + application but returning JSON. Admin-gated (`require_admin_login(..., true)`); `cross_origin(&headers)` false → `403 {error:"Cross-origin request rejected"}`; body over cap → `413`/`400 {error, code:"too_large"}`; empty/invalid → `400 {error, code}`; success → `200 {success:true, keys_added, keys_failed}`. Use `DefaultBodyLimit`/multipart the same way `restore_upload` does. Do NOT change `restore_upload` (the Askama route stays).
- [ ] **Step 3:** Add `.route("/api/restore", post(api_restore))` (with the same body-limit layer `restore_upload` uses if applicable). `cargo build` + `cargo test` clean. If a focused test is feasible (e.g. the error-code mapping), add it; else note manual/on-Pi verification. Commit: `feat(api): JSON POST /api/restore for config restore`.

---

## Task 2: System info page (`/app/system`)

**Files:** Create `frontend/src/routes/System.tsx`; modify `frontend/src/lib/api.ts` (types + `apiUpload`), `Sidebar.tsx`, `App.tsx`.

**Interfaces:** Consumes `useStatus()` (`/api/status` flattens full SystemStatus incl. resources/wifi/services), `useSettings`/`useUpdateSettings` (maintenance fields), `GET /backup` (link), `POST /api/restore` (Task 1).

- [ ] **Step 1: Types + `apiUpload`.** In `lib/api.ts`: extend the `Status`/SystemStatus TS type with the fields the page shows — READ `system.rs` `SystemStatus`/`ResourceStatus`/`MemoryStatus`/`WifiStatus`/`Services`/`ServiceStatus` for exact names: `ip_addresses:string[], uptime:string|null, cpu_temp_c:number|null, resources:{cpu_usage_percent:number|null, load_average:string|null, memory:{used_mb,total_mb,used_percent}, swap_total_mb,swap_used_mb,swap_used_percent}, services:{octocam_web:{state}, rtsp:{state}, homekit:{state}}, wifi:{...full}`. Extend `Settings` with maintenance fields (READ settings.rs:54-59 + the `scheduled_*_day_*` naming): `scheduled_service_restart_enabled:boolean, scheduled_service_restart_time:string, scheduled_service_restart_day_mon..sun:boolean` and the `scheduled_reboot_*` equivalents (confirm exact key names in settings.rs `weekdays_value` ~402-422). Add `apiUpload<T>(path, formData: FormData): Promise<T>` (POST, `credentials:"same-origin"`, NO Content-Type header — let the browser set the multipart boundary; parse `{error}` on !ok). Add `RestoreResult {success:boolean; keys_added?:number; keys_failed?:number; error?:string; code?:string}`.
- [ ] **Step 2: Status cards** (from `useStatus()`, matching `system.html`): a Wi-Fi signal row (reuse `signalPercent`/`signalLevel` from `lib/wifi.ts`), Address/Uptime/CPU temp+usage/Load, Memory + Swap meters (progress bars from `used_percent`, clamp 0-100; hide swap if `swap_total_mb===0`), Service states (Web UI/RTSP/HomeKit from `services.*.state`), and a Wi-Fi details list (interface, IP, MAC, BSSID, security, PHY/generation, frequency+band+channel, channel width, RSSI/signal_dbm, RX/TX rate, TX power, gateway — derive rows from raw `wifi` fields, mirroring `system.rs` `wifi_details()`). Skeleton/error states. (Power actions are NOT here — they're in the topbar.)
- [ ] **Step 3: Scheduled maintenance card.** Two sub-forms (service-restart, reboot): an enable switch, a time input (HH:MM), and 7 day toggles (Mon–Sun). Initialize from `useSettings()`; Save via `useUpdateSettings().mutate({...only the maintenance fields...})` sending per-day booleans (e.g. `scheduled_reboot_day_mon:true`). Disable Save while pending; success/error feedback.
- [ ] **Step 4: Backup/Restore card.** Backup: a download button/link `<a href="/backup" download>` (plain navigation — cookie auth). Restore: a file input (`.json`/backup file) + a Restore button behind a confirm `Dialog` ("This overwrites current config"); on confirm build `FormData` with the `backup` field and call `apiUpload<RestoreResult>("/api/restore", fd)`; show `keys_added`/`keys_failed` on success or the `error` on failure. (256 KiB server cap — surface the `too_large` error.)
- [ ] **Step 5: Wire up.** Sidebar "System info" `inApp:true`; `App.tsx` `<Route path="/system" element={<System/>}/>`. `npm run build` clean; eyeball. Commit: `feat(ui): System info page (status, maintenance, backup/restore)`.

---

## Task 3: Logs page (`/app/logs`)

**Files:** Create `frontend/src/routes/Logs.tsx`; modify `lib/api.ts` (`LogsResponse`), `Sidebar.tsx`, `App.tsx`.

**Interfaces:** Consumes `GET /api/logs` (`{lines:string[]}`).

- [ ] **Step 1:** `LogsResponse {lines:string[]}`. `useQuery(["logs"], () => apiGet<LogsResponse>("/api/logs"), { refetchInterval: 5000 })` (mirror the legacy 5s poll of the fixed 40-line snapshot).
- [ ] **Step 2: `Logs.tsx`** — a monospace `<pre>` (or scrollable code block) rendering `lines.join("\n")`, on the `--inverse`-style dark log surface if desired (use tokens; a `bg-muted`/`text-foreground` mono block is fine). A small "auto-refreshing every 5s" hint and a manual Refresh button (`refetch()`). Skeleton on first load; "logs unavailable" on error. (No streaming/tailing — deferred.)
- [ ] **Step 3: Wire up.** Sidebar "System logs" `inApp:true`; `App.tsx` `<Route path="/logs"/>`. Build clean; eyeball. Commit: `feat(ui): Logs page (journalctl snapshot, 5s refresh)`.

---

## Task 4: SSH keys page (`/app/ssh-keys`) + on-Pi verification

**Files:** Create `frontend/src/routes/SshKeys.tsx`, `frontend/src/hooks/useSshKeys.ts`; modify `lib/api.ts` (`SshKeyDto`), `Sidebar.tsx`, `App.tsx`.

**Interfaces:** Consumes `GET/POST/DELETE /api/ssh-keys` (Phase 2).

- [ ] **Step 1: Types + hook.** `SshKeyDto {key_type, comment, fingerprint, preview}`. `useSshKeys.ts`: `useSshKeys()` = `useQuery(["ssh-keys"], () => apiGet<SshKeyDto[]>("/api/ssh-keys"))` (a 503 → "couldn't read authorized_keys" error state). `useAddSshKey()` = mutation `POST /api/ssh-keys {public_key}` (map the 400 `code` — `bad_key|too_long|duplicate|write_failed` — to friendly copy; reuse `ssh_key_message` wording from main.rs:~1167-1203), invalidate `["ssh-keys"]`. `useRevokeSshKey()` = mutation `apiDelete("/api/ssh-keys", {fingerprint, confirm})`; on `409` surface a "this is your last key — confirm to end root SSH" state that resends with `confirm:true`; invalidate on success.
- [ ] **Step 2: `SshKeys.tsx`** — an `ssh root@<target>` hint (target = first `ip_addresses` else `${hostname}.local` from `useStatus()`); the keys list (key_type, comment or "(none)", fingerprint, preview; empty + read-error states); an add form (textarea `public_key` → `useAddSshKey`); per-key Revoke (normal → immediate; last-key 409 → confirm dialog → resend `confirm:true`). Theme tokens; skeleton/error.
- [ ] **Step 3: Wire up.** Sidebar "SSH keys" `inApp:true`; `App.tsx` `<Route path="/ssh-keys"/>`. Build clean; eyeball. Commit: `feat(ui): SSH keys page (list, add, revoke with last-key confirm)`.
- [ ] **Step 4: Deploy + on-Pi verification.**
```bash
cd /Users/soham/GitRepos/OctoCam && scripts/deploy-pi-web.sh
```
Controller verifies on the Pi: `/app/system`, `/app/logs`, `/app/ssh-keys` serve (200); `POST /api/restore` route exists (unauth → 401/403); `GET /api/logs`, `/api/ssh-keys` 401-gated. Browser (user, logged in): System shows live status + meters + Wi-Fi details, maintenance save persists, backup downloads, restore accepts a file; Logs auto-refreshes; SSH keys list/add/revoke work (test the last-key confirm carefully — don't lock yourself out). Commit any fixes.

---

## Self-Review

**Spec coverage (slice 4):** System info (status + maintenance + backup/restore) ✅ T2 (+T1 for restore JSON), Logs ✅ T3, SSH keys ✅ T4. Power actions correctly NOT duplicated (topbar owns them). Device-name/timezone correctly deferred to other slices.

**Placeholder scan:** No TBD. "Read settings.rs / system.rs for exact names" are real verification anchors (maintenance day-key naming, SystemStatus fields).

**Type consistency:** maintenance `scheduled_*` field names consistent between `useSettings`/`useUpdateSettings` and the System maintenance form; `RestoreResult` consistent between T1 (backend JSON) and T2 (apiUpload consumer); `SshKeyDto` matches `/api/ssh-keys`.

**Risk notes:** (1) SSH last-key revoke can lock the user out of root SSH — the confirm flow must be explicit; on-Pi verification must be careful. (2) Restore is destructive (overwrites config) + 256 KiB capped — confirm dialog + surface `too_large`/`invalid`. (3) `apiUpload` must NOT set `Content-Type` (browser sets the multipart boundary). (4) Maintenance day toggles send per-day booleans, not a CSV — the server re-serializes. (5) Reuse the slice-2 `lib/wifi.ts` signal helpers rather than reimplementing.
