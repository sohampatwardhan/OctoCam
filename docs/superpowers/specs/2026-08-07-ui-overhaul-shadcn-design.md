# OctoCam Web UI Overhaul — React + shadcn/ui over a Rust JSON API

**Date:** 2026-08-07
**Status:** Design — approved for planning
**Author:** brainstormed with Claude

## Summary

Replace the current server-rendered `octocam-web` UI (Askama HTML templates +
hand-rolled `styles.css` + vanilla `app.js`) with a **React + TypeScript
single-page app** built on **shadcn/ui + Tailwind**, served by the existing
**Rust/Axum** backend, which becomes a pure **JSON API + static file server**.

This is a full ("big-bang") frontend rewrite: all 18 template pages are migrated
to client-side routes, the backend's partial JSON API is completed to cover every
page, and the Askama templates / `styles.css` / `app.js` are deleted at the end.

### Goals

- Polished, cohesive, modern look and feel via a real component system.
- Lower UI maintenance: reusable primitives + design tokens instead of 35KB of
  hand-written CSS and 27KB of vanilla JS.
- Richer client-side interactivity (dialogs, validated forms, live panels).
- Use shadcn/ui specifically (the actual React component library), not a look-alike.

### Non-goals

- No change to the camera / mediamtx / HomeKit / HKSV pipeline.
- No SSR, no Node runtime on the device, no build step on the Pi.
- No new heavyweight E2E test harness.

## Hard constraint: Raspberry Pi Zero 2 W

Target hardware is a **Pi Zero 2 W** (quad Cortex-A53 @ 1GHz, **512MB RAM**),
and the web server **shares that RAM with the camera pipeline** (libcamera,
mediamtx WebRTC, HKSV encoding). Efficiency is a first-class requirement.

Why a SPA is *lighter on the Pi*, not heavier: all React rendering and
interactivity run in the **viewer's browser**, not on the device. The Pi's only
job becomes serving a small set of pre-built, cacheable static files plus small
JSON responses — less per-request work than even compile-time Askama templating,
and far less than any SSR approach.

Efficiency rules that follow:

- **Never build on the Pi.** Vite/Node build runs on the Mac (same model as the
  existing cross-compile + rsync deploy). The Pi never sees Node.
- **Lean, code-split bundle.** shadcn is copy-in components (only what is used
  ships). Route-level code-splitting, tree-shaking, and brotli+gzip
  precompressed assets with long cache headers.
- **No Node runtime / no SSR on device.** Static files + JSON only.
- **Cheap real-time.** Polling at 3–5s for status panels; reuse the existing SSE
  stream for logs; no unnecessary persistent connections.

## Architecture

```
Viewer's browser                         Pi Zero 2 W
┌───────────────────────────────┐        ┌──────────────────────────────────────┐
│ React SPA (Vite build)         │        │ octocam-web (Rust/Axum, single binary) │
│ • shadcn/ui + Tailwind         │ HTTP   │ • JSON API  /api/*                     │
│ • React Router (client-side)   │◀──────▶│ • SSE  /api/logs                       │
│ • TanStack Query (fetch+cache) │  JSON  │ • serves embedded SPA bundle           │
│ • all rendering/interactivity  │  +SSE  │ • auth: HMAC cookie + PBKDF2 + passkey │
└───────────────────────────────┘        │ • camera / mediamtx / HKSV (unchanged) │
        video (WebRTC/WHEP) ───────────▶  │ mediamtx :8889 (unchanged)             │
                                          └──────────────────────────────────────┘
```

- **Backend (Rust/Axum):** becomes a pure JSON API + static file server. Each
  `get(page)` handler that currently renders Askama is replaced by API
  endpoints. Askama, `styles.css`, and `app.js` are removed at the end of the
  migration.
- **Frontend (Vite + React + TS):** shadcn/ui + Tailwind. Client-side routing;
  Axum serves `index.html` as the fallback for any non-`/api`, non-static path.
- **Bundle delivery:** the built `dist/` is **embedded into the `octocam-web`
  binary** via `rust-embed` at compile time — a single artifact to rsync, served
  from memory. (Decision: embed, not a separate `ServeDir` folder.)

### Repository layout

```
rust/octocam-web/          # backend crate (JSON API + embedded SPA server)
  build.rs / rust-embed    # bakes ../../frontend/dist into the binary
frontend/                  # NEW: Vite + React + TS + shadcn
  src/
    routes/                # one module per page (dashboard, wifi, admin, ...)
    components/            # app components
    components/ui/         # shadcn CLI-generated primitives
    lib/                  # api client, query client, types
    hooks/                # useLogStream, useStatus, useAuth, ...
  dist/                    # build output (gitignored; embedded at build time)
scripts/deploy-pi-web.sh   # extended: vite build → cross-compile → rsync → restart
```

## Phasing

Big-bang rewrite, but with an explicit ordering to bound risk.

- **Phase 0 — Prerequisite (separate from the UI work):** finish, test, and
  commit the in-flight **multi-user + WebAuthn/passkey backend** (`db.rs`,
  `security.rs`, `/api/users/*`, `/api/passkey/*`) against the *current* UI so
  the overhaul starts from a clean, committed base. The React overhaul then only
  needs to surface this already-working auth.
- **Phase 1 — Scaffold + toolchain:** create `frontend/`, Vite + TS + Tailwind +
  shadcn init, rust-embed wiring, SPA fallback route, deploy script changes.
  Prove the whole loop (build on Mac → embed → rsync → serve) with a trivial page.
- **Phase 2 — API completion:** implement/complete every JSON endpoint in the
  mapping table below, with tests for shape + auth/role gating.
- **Phase 3 — Page migration:** migrate all routes to React. Simple pages first
  (identity, rtsp, ssh-keys, system), gnarlier ones last (stream preview, logs
  SSE, setup wizard, terminal). Keep each Askama page live until its React
  counterpart is verified on the real Pi.
- **Phase 4 — Cleanup:** delete Askama templates, `styles.css`, `app.js`; remove
  dead page handlers; final verification pass.

## API surface (page → endpoint mapping)

Legend: ✅ exists · ⚠️ partial · 🆕 new · ❓ needs investigation

| Client route | Backend endpoint(s) | Status |
|---|---|---|
| `/` dashboard | `GET /api/status` (service/camera/uptime/clients) | ✅ |
| `/settings`, `/stream-settings` | `GET` + `PUT /api/settings` | ✅ (POST→PUT) |
| `/wifi` | `GET /api/wifi/networks`, `POST /api/wifi/scan`, `POST /api/wifi/connect`, `DELETE /api/wifi/{profile}` | ⚠️ |
| `/admin` (users + passkeys) | `GET/POST/DELETE /api/users`, `/api/passkey/*`, `/api/passkeys` | ✅ (Phase 0) |
| `/identity` | `GET /api/identity` | 🆕 |
| `/rtsp` | `GET /api/rtsp` (URLs) | 🆕 |
| `/homekit` | `GET /api/homekit` (pairing state + QR SVG) | 🆕 |
| `/matter` | `GET /api/matter`, `POST /api/matter/reset` | ⚠️ |
| `/system` | `GET /api/system`, `POST /api/power`, `POST /api/time/sync` | 🆕 |
| `/ssh-keys` | `GET/POST/DELETE /api/ssh-keys` | ⚠️ |
| `/logs` | `GET /api/logs` (SSE) | ✅ |
| `/setup` (first-run wizard) | `GET` + `POST /api/setup` | ⚠️ |
| `/terminal` | transport TBD (investigate in planning) | ❓ |
| backup / snapshot | `GET /api/backup`, `GET /snapshot.jpg` (plain downloads, unchanged) | ✅ |

### API conventions

- REST-ish JSON. Reads are `GET`, updates `PUT`/`POST`, deletes `DELETE`.
- **QR codes stay server-generated:** the `qrcode` crate already emits SVG; the
  API returns the SVG (or payload) and React renders it. No client QR library.
- **Backup / snapshot** remain plain binary downloads, not JSON.
- **Terminal** transport is deliberately deferred: its current mechanism
  (`proc.rs` / `terminal.html`) is investigated during planning before a decision
  (likely a WebSocket PTY, but not assumed here).

## Data flow & real-time

- **Reads:** TanStack Query over `fetch`. Live panels (dashboard status, client
  counts) poll via `refetchInterval` at ~3–5s — cheap JSON, no persistent
  connections straining the Pi.
- **Logs:** existing **SSE** stream (`main.rs:1850`), consumed via `EventSource`
  in a `useLogStream` hook.
- **Stream preview:** stays **mediamtx WebRTC/WHEP** (`:8889`). A
  `<StreamPreview>` component wraps the mediamtx reader; the Pi's video path is
  untouched. Preserve current behavior: HD/SD toggle, start/stop, RTSP link.
- **Writes:** TanStack Query mutations; invalidate the relevant query on success;
  show a shadcn `toast` (replacing `_settings_toast.html`).

## Auth in the SPA

Keep the existing model — correct for a self-hosted device and reuses working code.

- **Session:** HMAC-signed HttpOnly `octocam_session` cookie (unchanged,
  `SameSite=Lax`). The SPA never reads it; it makes credentialed `fetch` calls.
- **Login:** `POST /api/login` (password / PBKDF2) plus the existing passkey flow
  (`/api/passkey/login/*`). Server sets the cookie.
- **Auth state:** `GET /api/me` → `{ authenticated, username, role }`. An
  `<AuthGate>` wrapper checks it; a `401` from any endpoint routes to `/login`.
- **Setup:** when no users exist, all routes redirect to the first-run `/setup`
  wizard.
- **Roles:** `role` from `/api/me` gates admin-only routes client-side
  (`/admin`, `/system` power actions); the **backend enforces role as the real
  gate**.
- **Captive-portal / WiFi-AP mode:** keeps its own separate minimal router
  (`main.rs` captive fallback). It does **not** load the full SPA — a tiny
  standalone page keeps first-boot WiFi setup working before the SPA is reachable.

## Build & deploy

- `frontend/`: Vite + React + TS, Tailwind, shadcn components generated into
  `components/ui/`. Route-level code-splitting; brotli + gzip precompressed assets.
- **Build on Mac only:** `vite build` → `frontend/dist/`. rust-embed bakes
  `dist/` into the binary at compile time. Cross-compile the binary as today →
  **one artifact** to rsync.
- `scripts/deploy-pi-web.sh` extended: `vite build` → cross-compile (embeds
  dist) → rsync binary → restart service.
- Axum serves embedded assets with **long-cache** headers for content-hashed
  files, `no-cache` for `index.html`, and an **SPA fallback** to `index.html`
  for non-`/api`, non-static paths.

## Error handling

- Uniform API error shape: `{ error: { code, message } }` with the correct HTTP
  status.
- Frontend: React Router `errorElement` per route + a top-level error boundary;
  TanStack Query surfaces fetch failures as inline retry states; mutations show a
  shadcn `toast` on success/failure.
- **Device unreachable:** query error states show a "device unreachable" banner
  rather than a blank screen.

## Testing

- **Backend:** keep/extend Rust unit tests (`security.rs` already has some); add
  handler tests for new JSON endpoints asserting response shape and auth/role
  gating.
- **Frontend:** Vitest + React Testing Library for component/hook logic (auth
  gate, query hooks, form validation). Kept intentionally light — no heavy E2E
  harness on a hobby device.
- **Manual verification:** dogfood each migrated page against the real Pi before
  deleting its Askama counterpart.

## Risks & open questions

- **Terminal transport** (❓): resolved during planning by inspecting
  `proc.rs` / `terminal.html`.
- **Bundle size on the Pi's clients:** mitigated by code-splitting, tree-shaking,
  and only shipping the shadcn components actually used; verify gzipped size
  stays modest.
- **Migration window:** old and new UIs coexist during Phase 3; ensure routes
  don't collide (SPA fallback must not shadow still-live Askama pages until they
  are cut over).
- **rust-embed + cross-compile:** confirm the embed step works cleanly in the
  cross-compilation path used by `deploy-pi-web.sh`.
