# UI Overhaul — Phase 1: Frontend Scaffold & Serving Loop — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a React + TypeScript + shadcn/ui frontend, embed its built bundle into the `octocam-web` binary, and serve it at `/app` — proving the full build → embed → deploy → serve loop with one live page, without touching any existing Askama page.

**Architecture:** A new `frontend/` Vite project (React + TS + Tailwind v4 + shadcn/ui) builds to `frontend/dist/`. The Rust/Axum backend gains a `rust_embed`-backed module that bakes `dist/` into the binary at compile time and serves it (with SPA-fallback routing) under the `/app` URL prefix. The Mac-side build script runs `vite build` on the host *before* the existing Dockerized cross-compile, so the container (which has no Node) only embeds an already-built bundle.

**Tech Stack:** Rust (axum 0.8, tower-http 0.6, rust-embed), React 18 + TypeScript, Vite, Tailwind CSS v4 (`@tailwindcss/vite`), shadcn/ui, lucide-react, TanStack Query.

## Global Constraints

- **Target hardware:** Raspberry Pi Zero 2 W — quad Cortex-A53 @ 1GHz, **512MB RAM shared with the camera pipeline** (libcamera/mediamtx/HKSV). Efficiency is a hard requirement.
- **Never build on the Pi, no Node on the Pi:** `vite build` runs only on the Mac host. The Pi receives one artifact (the binary with the bundle embedded).
- **Do not shadow live Askama pages:** the SPA is mounted only under `/app` in this phase. Do **not** register a root (`/`) SPA fallback — the existing pages at `/`, `/dashboard`, `/settings`, etc. must keep working. (Root cutover happens in Phase 4.)
- **Rust:** edition 2021, axum 0.8, tower-http 0.6. CI/Docker builds with `cargo build --release --locked` — `Cargo.lock` MUST be committed in sync whenever dependencies change.
- **Vite base path:** `/app/` (so the bundle's asset URLs resolve under the mount point).
- **Cache policy:** files under `assets/` (Vite content-hashed) → `public, max-age=31536000, immutable`; `index.html` → `no-cache`; anything else → `public, max-age=3600`.
- **Embed source path:** `rust_embed` folder is `../../frontend/dist` relative to the crate root `rust/octocam-web` (mirrors the existing `include_str!("../../../static/sw.js")` relative-path convention).
- **Prerequisite ordering:** `frontend/dist` must exist on disk before *any* `cargo build`/`cargo test`, because the `rust_embed` derive resolves its folder at compile time. Task 1 produces it before Task 2 depends on it.

---

## File Structure

**Create:**
- `frontend/` — Vite React+TS project (package.json, vite.config.ts, tsconfig*.json, index.html, components.json, src/…)
- `frontend/src/lib/api.ts` — typed fetch helper for the JSON API
- `frontend/src/App.tsx` — pilot page (status card)
- `rust/octocam-web/src/spa.rs` — rust-embed asset struct + pure resolution logic + axum handlers
- `frontend/.gitignore` (or root additions) — ignore `frontend/node_modules`, `frontend/dist`

**Modify:**
- `rust/octocam-web/Cargo.toml` — add `rust-embed`
- `rust/octocam-web/Cargo.lock` — regenerated (via `cargo build`)
- `rust/octocam-web/src/main.rs` — `mod spa;`, mount `/app` + `/app/{*path}` routes
- `scripts/build-pi-web.sh` — run host-side `npm ci` + `npm run build` in `frontend/` before the Docker cargo build

---

## Task 1: Scaffold the frontend project with a live pilot page

**Files:**
- Create: `frontend/` (entire Vite project), `frontend/src/lib/api.ts`, `frontend/src/App.tsx`
- Create: `frontend/.gitignore`

**Interfaces:**
- Produces: a `frontend/dist/` directory containing `index.html` and `assets/*` after `npm run build`, with all asset URLs prefixed `/app/`. Task 2 embeds this directory.

- [ ] **Step 1: Verify Node toolchain is present**

Run:
```bash
node --version && npm --version
```
Expected: Node ≥ 20 and npm present. If missing, stop and install Node 20 LTS (this is a Mac-only build dependency; it never ships to the Pi).

- [ ] **Step 2: Scaffold the Vite React+TS project**

Run from the repo root:
```bash
npm create vite@latest frontend -- --template react-ts
cd frontend && npm install
```
Expected: `frontend/` created with `package.json`, `vite.config.ts`, `tsconfig.json`, `tsconfig.app.json`, `tsconfig.node.json`, `src/`.

- [ ] **Step 3: Install Tailwind v4, path alias tooling, and app deps**

Run in `frontend/`:
```bash
npm install tailwindcss @tailwindcss/vite
npm install -D @types/node
npm install @tanstack/react-query lucide-react
```

- [ ] **Step 4: Wire Tailwind into the CSS entry**

Replace the entire contents of `frontend/src/index.css` with:
```css
@import "tailwindcss";
```

- [ ] **Step 5: Configure Vite (Tailwind plugin, `@` alias, `/app/` base)**

Replace `frontend/vite.config.ts` with:
```typescript
import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// Served under /app on the device, so every asset URL must be prefixed.
export default defineConfig({
  base: "/app/",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
})
```

- [ ] **Step 6: Add the `@` path alias to TypeScript config**

In `frontend/tsconfig.json`, add `compilerOptions` with the alias (merge, do not delete the existing `references`/`files`):
```json
{
  "files": [],
  "references": [
    { "path": "./tsconfig.app.json" },
    { "path": "./tsconfig.node.json" }
  ],
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] }
  }
}
```
Then in `frontend/tsconfig.app.json`, add inside `compilerOptions`:
```json
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] }
```

- [ ] **Step 7: Initialize shadcn/ui and add pilot components**

Run in `frontend/`:
```bash
npx shadcn@latest init -d
npx shadcn@latest add card button badge
```
Expected: `components.json` created, `src/lib/utils.ts` created, `src/components/ui/{card,button,badge}.tsx` created, Tailwind theme variables written into `src/index.css`. `-d` accepts defaults (neutral base color, CSS variables).

- [ ] **Step 8: Write the typed API helper**

Create `frontend/src/lib/api.ts`:
```typescript
// Minimal typed fetch wrapper. Credentialed so the octocam_session cookie rides along.
export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin" })
  if (!res.ok) throw new Error(`${path} -> ${res.status}`)
  return res.json() as Promise<T>
}

export interface Status {
  service: string
  camera: string
  uptime: string
}
```

- [ ] **Step 9: Write the pilot page**

Replace `frontend/src/App.tsx` with a status card that proves React + shadcn + the API all work:
```tsx
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query"
import { Activity } from "lucide-react"
import { apiGet, type Status } from "@/lib/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

const queryClient = new QueryClient()

function StatusCard() {
  const { data, isLoading, isError } = useQuery({
    queryKey: ["status"],
    queryFn: () => apiGet<Status>("/api/status"),
    refetchInterval: 5000,
  })

  return (
    <Card className="w-full max-w-md">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Activity className="size-5" /> OctoCam SPA
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        {isLoading && <p className="text-muted-foreground">Loading status…</p>}
        {isError && <Badge variant="destructive">device unreachable</Badge>}
        {data && (
          <>
            <div>Service: <Badge>{data.service}</Badge></div>
            <div>Camera: <Badge>{data.camera}</Badge></div>
            <div>Uptime: {data.uptime}</div>
          </>
        )}
      </CardContent>
    </Card>
  )
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <main className="min-h-screen flex items-center justify-center p-6">
        <StatusCard />
      </main>
    </QueryClientProvider>
  )
}
```

- [ ] **Step 10: Ignore build/dependency artifacts**

Create `frontend/.gitignore`:
```gitignore
node_modules
dist
*.local
```

- [ ] **Step 11: Type-check and build; verify the bundle**

Run in `frontend/`:
```bash
npm run build
ls dist && ls dist/assets && grep -c '/app/assets/' dist/index.html
```
Expected: `npm run build` succeeds (tsc + vite build); `dist/index.html` exists; `dist/assets/` contains hashed `.js`/`.css`; the `grep` count is ≥ 1 (asset URLs are `/app/`-prefixed, confirming the `base` setting took effect).

- [ ] **Step 12: Commit**

```bash
cd .. && git add frontend
git commit -m "feat(ui): scaffold React+shadcn frontend with pilot status page"
```

---

## Task 2: Embed and serve the bundle at `/app`

**Files:**
- Modify: `rust/octocam-web/Cargo.toml`, `rust/octocam-web/Cargo.lock`
- Create: `rust/octocam-web/src/spa.rs`
- Modify: `rust/octocam-web/src/main.rs` (add `mod spa;` and two routes)
- Test: inline `#[cfg(test)]` module in `rust/octocam-web/src/spa.rs`

**Interfaces:**
- Consumes: `frontend/dist/` (from Task 1) at path `../../frontend/dist` relative to the crate root.
- Produces:
  - `pub fn cache_control_for(path: &str) -> &'static str`
  - `pub struct SpaResponse { pub status: axum::http::StatusCode, pub content_type: String, pub cache_control: &'static str, pub body: Vec<u8> }`
  - `pub fn resolve_spa(path: &str) -> SpaResponse` — empty/unknown paths fall back to `index.html`
  - `pub async fn spa_index() -> axum::response::Response`
  - `pub async fn spa_asset(path: axum::extract::Path<String>) -> axum::response::Response`

- [ ] **Step 1: Add the rust-embed dependency**

In `rust/octocam-web/Cargo.toml`, under `[dependencies]` (keep alphabetical grouping loose, matching the file), add:
```toml
rust-embed = "8"
```

- [ ] **Step 2: Regenerate the lockfile**

Run (requires `frontend/dist` to already exist from Task 1):
```bash
cd rust/octocam-web && cargo build 2>&1 | tail -5
```
Expected: builds successfully; `Cargo.lock` now contains `rust-embed`. (This also confirms the embed folder path resolves.)

- [ ] **Step 3: Write the failing tests for the pure resolution logic**

Create `rust/octocam-web/src/spa.rs` with only the test module first (it will not compile yet — that is the failing state):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_policy_matches_asset_class() {
        assert_eq!(
            cache_control_for("assets/index-a1b2c3.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control_for("index.html"), "no-cache");
        assert_eq!(cache_control_for("favicon.ico"), "public, max-age=3600");
    }

    #[test]
    fn index_is_served_for_root() {
        let r = resolve_spa("");
        assert_eq!(r.status, axum::http::StatusCode::OK);
        assert!(r.content_type.starts_with("text/html"));
        assert!(!r.body.is_empty());
    }

    #[test]
    fn unknown_client_route_falls_back_to_index() {
        // A client-side route like /app/settings is not a real file; SPA must
        // return index.html (200) so the router can take over in the browser.
        let r = resolve_spa("settings");
        assert_eq!(r.status, axum::http::StatusCode::OK);
        assert!(r.content_type.starts_with("text/html"));
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail to compile**

Run:
```bash
cd rust/octocam-web && cargo test --lib spa 2>&1 | tail -15
```
Expected: FAIL — `cannot find function cache_control_for` / `resolve_spa` (unresolved names). This confirms the tests exercise not-yet-written code.

- [ ] **Step 5: Implement the embed struct and pure logic**

Prepend to `rust/octocam-web/src/spa.rs` (above the test module):
```rust
use axum::extract::Path as AxumPath;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// The built SPA bundle, baked into the binary at compile time.
/// Path is relative to the crate root (rust/octocam-web).
#[derive(RustEmbed)]
#[folder = "../../frontend/dist"]
struct SpaAssets;

/// Cache-Control for a bundle path. Vite content-hashes everything under
/// `assets/`, so those are immutable; index.html must never be cached so
/// clients always pick up a new deploy.
pub fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}

pub struct SpaResponse {
    pub status: StatusCode,
    pub content_type: String,
    pub cache_control: &'static str,
    pub body: Vec<u8>,
}

/// Resolve a request path (relative to the /app mount, no leading slash) to an
/// embedded asset, falling back to index.html for client-side routes.
pub fn resolve_spa(path: &str) -> SpaResponse {
    let lookup = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = SpaAssets::get(lookup) {
        return SpaResponse {
            status: StatusCode::OK,
            content_type: file.metadata.mimetype().to_string(),
            cache_control: cache_control_for(lookup),
            body: file.data.into_owned(),
        };
    }

    // Not a real file: hand back index.html so the SPA router handles it.
    match SpaAssets::get("index.html") {
        Some(file) => SpaResponse {
            status: StatusCode::OK,
            content_type: "text/html; charset=utf-8".to_string(),
            cache_control: cache_control_for("index.html"),
            body: file.data.into_owned(),
        },
        None => SpaResponse {
            status: StatusCode::NOT_FOUND,
            content_type: "text/plain; charset=utf-8".to_string(),
            cache_control: "no-cache",
            body: b"SPA bundle missing".to_vec(),
        },
    }
}

fn into_response(r: SpaResponse) -> Response {
    let content_type = HeaderValue::from_str(&r.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    (
        r.status,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, HeaderValue::from_static(r.cache_control)),
        ],
        r.body,
    )
        .into_response()
}

/// Handler for the bare `/app` mount point.
pub async fn spa_index() -> Response {
    into_response(resolve_spa(""))
}

/// Handler for `/app/{*path}` — assets and client routes alike.
pub async fn spa_asset(AxumPath(path): AxumPath<String>) -> Response {
    into_response(resolve_spa(&path))
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run:
```bash
cd rust/octocam-web && cargo test --lib spa 2>&1 | tail -15
```
Expected: PASS — all three tests green. (Requires `frontend/dist` present from Task 1.)

- [ ] **Step 7: Mount the SPA routes**

In `rust/octocam-web/src/main.rs`, add near the other `mod` declarations at the top of the file:
```rust
mod spa;
```
Then in the `Router::new()` chain (around `src/main.rs:528`, alongside the `/static/{*path}` route), add:
```rust
        .route("/app", get(spa::spa_index))
        .route("/app/{*path}", get(spa::spa_asset))
```
Do NOT add any `/` fallback — the existing Askama routes must remain untouched.

- [ ] **Step 8: Verify the whole crate builds and the full test suite passes**

Run:
```bash
cd rust/octocam-web && cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -15
```
Expected: build OK, all tests (existing + new `spa` tests) pass.

- [ ] **Step 9: Commit**

```bash
cd ../.. && git add rust/octocam-web/Cargo.toml rust/octocam-web/Cargo.lock rust/octocam-web/src/spa.rs rust/octocam-web/src/main.rs
git commit -m "feat(ui): embed and serve the SPA bundle at /app"
```

---

## Task 3: Wire the frontend build into the Pi build script

**Files:**
- Modify: `scripts/build-pi-web.sh`

**Interfaces:**
- Consumes: `frontend/` project (Task 1), the `/app` serving code (Task 2).
- Produces: a `dist/pi/octocam-web` binary with the freshly-built SPA embedded, ready for `scripts/deploy-pi-web.sh`.

- [ ] **Step 1: Build the frontend on the host before the Docker cargo build**

In `scripts/build-pi-web.sh`, immediately **before** the `echo "Building OctoCam web UI…"` / `docker run` block (after the Docker availability checks, ~line 48), insert:
```bash
# Build the SPA bundle on the host first. The Rust build container has no Node,
# and rust-embed bakes frontend/dist into the binary at compile time.
FRONTEND_DIR="$PROJECT_DIR/frontend"
if [[ -f "$FRONTEND_DIR/package.json" ]]; then
  if ! command -v npm >/dev/null 2>&1; then
    echo "npm is required to build the web UI bundle. Install Node 20 LTS and retry." >&2
    exit 1
  fi
  echo "Building SPA bundle in $FRONTEND_DIR..."
  ( cd "$FRONTEND_DIR" && npm ci && npm run build )
fi
```

- [ ] **Step 2: Verify the script builds a bundle-embedded binary end to end**

Run (requires Docker running):
```bash
scripts/build-pi-web.sh
```
Expected: the SPA build runs first, then the Docker cross-compile succeeds, and `dist/pi/octocam-web` is (re)written. If Docker is unavailable in this environment, skip execution and note it — the deploy verification below covers the real check.

- [ ] **Step 3: Commit**

```bash
git add scripts/build-pi-web.sh
git commit -m "build: build SPA bundle on host before Pi cross-compile"
```

- [ ] **Step 4: Deploy and verify on the real Pi (manual verification)**

Run:
```bash
scripts/deploy-pi-web.sh
```
Then verify the SPA serves and existing pages are unaffected:
```bash
ssh root@octocam.local 'curl -fsS -m 4 -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8080/app'          # expect 200
ssh root@octocam.local 'curl -fsS -m 4 http://127.0.0.1:8080/app | grep -c "/app/assets/"'                   # expect >=1
ssh root@octocam.local 'curl -fsS -m 4 -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8080/login'         # expect 200 (Askama page still works)
ssh root@octocam.local 'curl -fsS -m 4 -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8080/app/settings'  # expect 200 (SPA fallback -> index.html)
```
Then open `https://octocam.local/app` in a browser and confirm the shadcn status card renders and shows live `/api/status` values. Expected: card renders; Service/Camera/Uptime populate from the API.

---

## Self-Review

**Spec coverage (Phase 1 scope only):**
- ✅ React + TS + shadcn + Tailwind scaffold → Task 1
- ✅ Bundle embedded in the binary (rust-embed, single artifact) → Task 2
- ✅ SPA fallback routing without shadowing Askama pages (mounted at `/app`) → Task 2 Steps 5, 7
- ✅ Build on Mac only, no Node on the Pi → Task 3 Step 1
- ✅ Cache policy (immutable assets, no-cache index) → Task 2 `cache_control_for`
- ✅ Cheap live panel via TanStack Query polling → Task 1 Step 9 (`refetchInterval: 5000`)
- ✅ Device-unreachable state instead of blank screen → Task 1 Step 9 (`isError` badge)
- ⏭️ Full page migration, API completion, auth gate, Askama deletion → **out of scope for Phase 1** (Phases 2–4, separate plans)

**Placeholder scan:** No TBD/TODO/"handle appropriately" steps. All code blocks are concrete; all verification steps have explicit expected outcomes.

**Type consistency:** `resolve_spa`, `cache_control_for`, `SpaResponse`, `spa_index`, `spa_asset` names match between the Interfaces block, the tests (Step 3), and the implementation (Step 5). `apiGet`/`Status` in `api.ts` match their use in `App.tsx`. Routes `/app` and `/app/{*path}` match the handlers they bind.
