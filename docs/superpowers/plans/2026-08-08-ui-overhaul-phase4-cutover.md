# UI Overhaul — Phase 4: Root Cutover + Askama Removal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** Make the React SPA the root UI (`/app`→`/`), delete the entire legacy Askama frontend (templates, page + form-POST handlers, `askama` dep, `static/` = styles.css/app.js/sw.js), remove the Terminal feature, and stop shipping `static/` to the Pi. Full parity was reached in Phase 3, so no feature is lost.

**Architecture:** Three tightly-sequenced tasks, each ending with a GREEN build (`cargo build` + `cargo test`, `npm run build`). Task 1 flips serving to root + drops the Askama route registrations (handlers go dead-but-compiling). Task 2 deletes all the now-dead Askama code + `static/` + the `askama` dep. Task 3 updates the deploy script, deploys, verifies at `/`, and removes `static/` from the Pi. The deploy script's existing binary-backup + health-gate rollback is the safety net.

**Tech Stack:** Rust/Axum, React/Vite; the change is mostly deletions in `main.rs` + `spa.rs` + config.

## Global Constraints

- **KEEP (must not break) — verified still used:** all `/api/*`; `/backup` (`backup_download`, React `<a href="/backup">`); `/snapshot.jpg` (`snapshot`, React `<img src="/snapshot.jpg">`); the captive-portal / first-boot listener + `/hotspot-detect.html` + `/generate_204`; `/internal/snapshot.jpg` (loopback listener); **`proc.rs`** (shared by camera/system/wifi/wifi_setup/ssh_keys/api_power — NOT terminal-related).
- **KEEP shared helpers** (used by kept `/api/*` handlers — do NOT delete): `homekit_view`/`HomeKitView`/`HomeKitStatus`; `matter::view`/`MatterView`; `stream_urls_for`/`stream_url_for`/`StreamUrls`; `time_zone_views`/`TimeZoneView`; `settings::preset_views`/`PresetView`/`RESOLUTION_PRESETS`/`SUB_RESOLUTION_PRESETS`; `system::stored_wifi_profiles`/`StoredWifiProfile`; `settings::WEEKDAYS` const.
- **Root-cutover, not shadowing:** kept routes are explicit `.route(...)` and always win; the SPA is `.route("/", spa_index)` + `.fallback(spa_asset)` (axum 0.8 — fallback only fires when no explicit route matches). Do NOT reintroduce a `{*path}` catch-all.
- **No behavior change to kept features.** This phase only removes the legacy UI and moves the SPA to root. The build must stay green at each task boundary; the final `cargo test` (currently 121) must pass (minus any tests that were asserting deleted Askama behavior — none expected).
- **Rollback net:** `deploy-pi-web.sh` backs up the binary and health-gates before committing; keep that. Verify carefully on the Pi after cutover.
- **Do NOT push to origin** as part of this (user pushes on request).

---

## Task 1: Root cutover (serve SPA at `/`, drop Askama route registrations)

**Files:** `frontend/vite.config.ts`, `frontend/src/main.tsx`, `rust/octocam-web/src/spa.rs`, `rust/octocam-web/src/main.rs` (Router only).

- [ ] **Step 1: Frontend base/basename.** `vite.config.ts`: `base: "/app/"` → `base: "/"` (or remove `base`). `frontend/src/main.tsx`: `<BrowserRouter basename="/app">` → `<BrowserRouter>` (drop basename). Embed folder path in `spa.rs` (`../../frontend/dist`) is UNCHANGED.
- [ ] **Step 2: `spa.rs` serve-at-root.** Change `spa_asset` to read the path from the request URI instead of an `AxumPath<String>` extractor (so it works as a `.fallback` handler): take `uri: axum::http::Uri`, compute `let path = uri.path().trim_start_matches('/');` then `into_response(resolve_spa(path))`. Keep `spa_index` (bare `/`) and `resolve_spa`/`cache_control_for`/`body_from` unchanged. Update the spa.rs tests if they referenced the `/app` mount (the resolve_spa unit tests are path-based and unaffected).
- [ ] **Step 3: `main.rs` Router.** Replace the three `/app*` route registrations (`.route("/app", ...)`, `.route("/app/", ...)`, `.route("/app/{*path}", ...)`) with:
  ```rust
  .route("/", get(spa::spa_index))
  // ...(all kept routes remain)...
  .fallback(spa::spa_asset)
  ```
  and REMOVE the route registrations (the `.route("/path", get(handler))` lines only — NOT the handler fns yet) for every Askama page + form-POST route: `/identity`, `/wifi`, `/stream-settings`, `/rtsp`, `/homekit`, `/matter`, `/admin`, `/advanced`, `/system`, `/logs`, `/terminal`, `/ssh-keys`, `/dashboard`, `/stream`, `/setup` GET+POST, `/login` GET+POST, `/logout`, `/settings` GET+POST, `/power`, `/wifi/scan|connect|delete`, `/matter/reset`, `/time/sync`, `/ssh-keys/add|revoke`, `/restore`, `/sw.js`, `/static/{*path}`. KEEP: `/`, all `/api/*`, `/backup`, `/snapshot.jpg`, `/hotspot-detect.html`, `/generate_204`. (Removing `/hotspot-detect.html`+`/generate_204` from the MAIN router is optional — they're also on the captive listener — but leave them for the online captive-probe behavior.)
- [ ] **Step 4: Build (green, with dead-code warnings expected).** `cd rust/octocam-web && cargo build` — expect it to compile with `dead_code` warnings for the now-unregistered handlers (that's fine this task; Task 2 removes them). `cargo test`. `cd frontend && npm run build`. Confirm the built `index.html` references `/assets/...` (root-relative, no `/app/`).
- [ ] **Step 5: Commit.** `git add frontend rust/octocam-web/src/spa.rs rust/octocam-web/src/main.rs && git commit -m "feat(ui): serve the React SPA at root (/) and drop /app + Askama route registrations"`.

---

## Task 2: Delete the Askama layer (handlers, templates, static/, askama dep)

**Files:** `rust/octocam-web/src/main.rs` (bulk deletions), `rust/octocam-web/src/system.rs` + `wifi.rs` (dead view fns), `rust/octocam-web/templates/` (whole dir), `static/` (whole dir), `rust/octocam-web/Cargo.toml`.

- [ ] **Step 1: Delete Askama page + form handler fns in main.rs.** Remove these fns (now unregistered/dead): `dashboard_redirect`, `identity`, `render_identity_page`, `settings_page`, `wifi_page`, `stream_settings`, `rtsp_page`, `homekit`, `matter_page`, `admin`, `system_page`, `logs`, `terminal`, `ssh_keys_page`, `stream`, `setup` (GET), `login` (GET), `authenticate`, `logout`, `update_settings`, `power_action`, `scan_wifi`, `connect_wifi`, `delete_wifi_profile`, `matter_reset`, `sync_time`, `ssh_keys_add`, `ssh_keys_revoke`, `restore_upload`, `complete_setup`. Also the `/sw.js`+`/static` handlers: `service_worker`, `static_asset`, `content_type_for`, and consts `SERVICE_WORKER_JS`, `STATIC_CACHE_CONTROL`.
- [ ] **Step 2: Delete now-orphaned support code in main.rs.** Template structs: `IdentityTemplate, WifiTemplate, StreamSettingsTemplate, RtspTemplate, HomeKitTemplate, MatterTemplate, AdminTemplate, SystemTemplate, SshKeysTemplate, LogsTemplate, TerminalTemplate, StreamTemplate, SetupTemplate, LoginTemplate`. Helpers: `render`, `weekday_options`+`WeekdayOption`, `rotation_views`+`RotationView`, `ssh_key_message`, `clean_return_path`. Form/query structs: `PowerForm, SshKeyAddForm, SshKeyRevokeForm, SavedQuery, SshKeysQuery, SystemQuery, SetupQuery, LoginQuery`. Askama glue: `use askama::Template;` and `impl From<askama::Error> for AppError`. (Keep `PowerReq`/`SshKeyAddReq`/`SshKeyDeleteReq` — the JSON siblings.)
- [ ] **Step 3: Delete now-dead view builders.** `rust/octocam-web/src/system.rs`: `system::view()` + `SystemView` (+ any structs used only by it — verify via `cargo build` warnings). `rust/octocam-web/src/wifi.rs`: `network_views()` + `WifiNetworkView`. (These were only ever called by the deleted page handlers — confirm no `/api/*` caller via a grep before deleting.)
- [ ] **Step 4: Delete files + dep.** `rm -rf rust/octocam-web/templates/`; `rm -rf static/`; in `Cargo.toml` remove the `askama = "0.12"` line.
- [ ] **Step 5: Build clean.** `cd rust/octocam-web && cargo build` — must now compile with NO dead-code warnings from this work (chase any residual unused-import/fn warnings the deletions surface and remove them). `cargo test` — full suite green (delete/adjust any test that asserted deleted Askama behavior — none expected; the spa/api tests stay). `cd frontend && npm run build` clean. Confirm `grep -rn askama rust/octocam-web/src` returns nothing (except code comments) and `Cargo.lock` updated (`cargo build` regenerates; commit it).
- [ ] **Step 6: Commit.** `git add -A && git commit -m "chore(ui): remove legacy Askama frontend (templates, page/form handlers, static/, askama dep, terminal)"`.

---

## Task 3: Deploy-script cutover + deploy + on-Pi verification

**Files:** `scripts/deploy-pi-web.sh`.

- [ ] **Step 1: Deploy script.** Remove the `static/` rsync block (the `mkdir -p '$REMOTE_DIR/static'` and the `rsync ... "$PROJECT_DIR/static/" "...:$REMOTE_DIR/static/"`). Add a Pi-side cleanup in the remote bash block: `sudo -n rm -rf '$REMOTE_DIR/static'`. Update the health gate: change the `/app` probe to `/` (SPA index), keep the `/login` probe (now served by the SPA fallback → 200 index.html), and optionally add an `/api/me` probe expecting 401 (proves the API serves, not just static). `build-pi-web.sh` needs no change (it references no `static/`).
- [ ] **Step 2: Build + full test.** `cd frontend && npm run build`; `cd rust/octocam-web && cargo test` (green). Commit: `git add scripts/deploy-pi-web.sh && git commit -m "build: stop shipping static/ to the Pi; health-gate root instead of /app"`.
- [ ] **Step 3: Deploy.** `cd /Users/soham/GitRepos/OctoCam && scripts/deploy-pi-web.sh` — the health gate must pass (`/` + `/login` serve the SPA). If it fails, the script auto-rolls-back the binary; investigate before retrying.
- [ ] **Step 4: Controller on-Pi verification.** Confirm on the Pi:
  - `GET /` → 200, HTML references `/assets/...` (root, not `/app/`).
  - `GET /dashboard`, `/wifi`, `/admin`, `/settings`, `/login`, `/setup` → 200 (SPA fallback → index.html; client router renders).
  - `GET /app` → 404 (mount removed) — acceptable; nothing links there anymore.
  - `GET /api/me` (no cookie) → 401; `/backup` (no cookie) → 401/redirect (route exists); `/snapshot.jpg` route exists; `/hotspot-detect.html` still responds.
  - `GET /static/styles.css` → 404 (removed); confirm `$REMOTE_DIR/static` is gone on the Pi (`ssh ... ls`).
  - Browser (user, logged in): the SPA loads at `https://octocam.local/` (root), sidebar + all pages work, login/logout cycle works. This is the finish line.
- [ ] **Step 5:** Commit any verification fixes. Report completion + that the overhaul is done (Askama fully removed, SPA at root).

---

## Self-Review

**Spec coverage:** root cutover ✅ T1, Askama+static+askama+terminal removal ✅ T2, deploy/static-removal/health-gate + verify ✅ T3. `proc.rs` kept; all shared `/api` helpers kept; captive-portal + backup + snapshot kept.

**Placeholder scan:** No TBD. The dead-view-builder deletions (T2 Step 3) say "confirm via cargo build warnings / grep" — that's the correct mechanical way to catch residual dead code, not a placeholder.

**Risk notes:** (1) **Deleting a shared helper by mistake** breaks a kept `/api/*` handler — the KEEP list above is explicit; the reviewer must confirm each kept helper still compiles/used. (2) **Build must stay green at each task boundary** — T1 tolerates dead-code warnings, T2 must end warning-clean. (3) **Health-gate paths** — after cutover `/app` is gone; the gate must probe `/` or it will false-fail and roll back a good deploy. (4) Two React components use plain `<a href="/rtsp">`/`<a href="/settings">` (StreamPreview, Topbar) — after root cutover these correctly full-page-nav into the SPA fallback (a latent mismatch at `/app` is actually fixed); optional follow-up to make them `<Link>` for client-side nav (not required). (5) The deploy is destructive/outward — rely on the script's rollback and verify before declaring done.
