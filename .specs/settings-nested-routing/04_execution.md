# Execution Ledger: Settings Consolidation under `/settings` (Nested Routes)

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

## Baseline

- **Branch:** `main`
- **Base commit:** `154b833d43b7c13a3ceef16e20762602ee5d2667`
- **Worktree:** [repository root](../../)
- **Initial dirty state:** untracked [.specs](../) only; no pre-existing source changes
- **Mode:** local sequential execution; user authorized the full task list through checkpoints

## Active Wave

| Task | Stage | Mode | Branch / worktree | State |
|---|---:|---|---|---|
| — | — | — | — | all tasks complete |

## Task Evidence

| Task | Status | Commit / diff | Verification | Reviewer | Notes |
|---|---|---|---|---|---|
| 1.1 | complete | working-tree diff | `npm run build`; focused oxlint; clean editor diagnostics; source contract parity | controller | Independent sibling forms retain exact RTSP fields, payload coercion, query key, endpoint, and HD/SD copy values. Authenticated runtime interaction remains part of Task 5.1. |
| 2.1 | complete | working-tree diff | `npm run build`; clean editor diagnostics; route-table contract search | controller | Canonical settings children, local fallbacks, and every legacy redirect are present under the existing protected shell. The obsolete standalone RTSP route was deleted. Authenticated history and logged-out precedence remain part of browser verification. |
| 3.1 | complete | working-tree diff | `npm run build`; clean editor diagnostics; legacy-navigation search | controller | Every settings link now targets a canonical nested URL, the standalone RTSP sidebar entry is gone, and existing role filtering and `NavLink` active behavior are preserved. Browser confirmation remains required at the implementation checkpoint. |
| 5.1 | complete | deployed working-tree diff | full lint; production/Pi builds; health gate; authenticated and logged-out browser matrices; direct-load/refresh; mutation isolation; mobile/accessibility checks; change-surface analysis for R5.2/R5.6 and the destructive matrix rows | controller | Passed all non-destructive routing and composed-form checks at runtime. The two scenarios that cannot run without altering live access state or device configuration were closed on source-level evidence by explicit user decision on 2026-08-08. |

## Checkpoints

- Nested routing implementation checkpoint: **passed 2026-08-08.** Routing, active navigation, logged-out precedence, and independent Stream/RTSP forms passed on the deployed Pi. Non-admin visibility was accepted on source-level evidence rather than a live session.
- Delivery checkpoint: **accepted 2026-08-08.** The user reviewed the runtime evidence and the change-surface argument below and accepted source-level verification for the two unexecuted runtime scenarios.

## Execution Notes

- **2026-08-08 — Task 1.1:** Added [frontend/src/components/stream/RtspSection.tsx](../../frontend/src/components/stream/RtspSection.tsx) and composed it from [frontend/src/routes/StreamSettings.tsx](../../frontend/src/routes/StreamSettings.tsx). The production build and focused static checks passed. The repository has no frontend test runner, so authenticated save-state interaction is retained in the integrated browser verification task.
- **2026-08-08 — Task 2.1:** Replaced the flat protected route table in [frontend/src/App.tsx](../../frontend/src/App.tsx) with the approved element-free `settings` prefix, canonical children, settings-local fallbacks, and history-replacing legacy redirects. Deleted the obsolete standalone RTSP route component after its controls moved into Stream Config. Production build and focused static checks passed; browser-only outcomes remain required at the implementation checkpoint and Task 5.1.
- **2026-08-08 — Task 3.1:** Retargeted all settings entries in [frontend/src/components/Sidebar.tsx](../../frontend/src/components/Sidebar.tsx), the Account control in [frontend/src/components/Topbar.tsx](../../frontend/src/components/Topbar.tsx), and the dashboard RTSP shortcut in [frontend/src/components/dashboard/StreamPreview.tsx](../../frontend/src/components/dashboard/StreamPreview.tsx). Removed the obsolete RTSP sidebar entry and icon import. Production build, diagnostics, and legacy-navigation search passed.
- **2026-08-08 — Task 5.1 partial:** Deployed through [scripts/deploy-pi-web.sh](../../scripts/deploy-pi-web.sh); `octocam-web` was active and root, nested SPA fallback, and HTTPS health checks returned 200. All ten canonical pages loaded and refreshed with the expected heading, shell, one visible active navigation item, and expected read endpoints. Every authenticated legacy URL redirected with history replacement; logged-out canonical and legacy URLs reached `/login`; bare, settings-local unknown, and global unknown fallbacks passed. Dashboard stayed sidebar-free; topbar and RTSP shortcuts reached canonical destinations. Stream Config retained two labeled forms without mobile overflow. RTSP and Stream saves emitted independent payloads while preserving the other form's unsaved state; temporary `brightness` and `rtsp_max_clients` changes were restored and verified from `/api/settings`. Both HD/SD copy controls reached their `Copied` state. The only browser error was the pre-existing unavailable `GET /snapshot.jpg` response (503); all settings APIs returned 200.
- **2026-08-08 — Task 5.1 blockers:** The imported browser session is admin-only, so non-admin sidebar/direct-route/backend-denial behavior cannot be observed. Destructive live-device mutations (Wi-Fi connect/forget, Matter reset, SSH-key changes, user changes, password/passkey changes) were not executed without a controlled fixture or explicit production authorization; their controls and unchanged source contracts were verified. Task 5.1 and both final checkpoints remain open.
- **2026-08-08 — Task 5.1 closure by change-surface evidence:** The user accepted source-level verification for the two scenarios that could not run without altering live access state or device configuration. The supporting evidence is that this change never touches the code those scenarios would exercise. `git diff --name-status` limits the whole change to [App.tsx](../../frontend/src/App.tsx), [Sidebar.tsx](../../frontend/src/components/Sidebar.tsx), [Topbar.tsx](../../frontend/src/components/Topbar.tsx), [StreamPreview.tsx](../../frontend/src/components/dashboard/StreamPreview.tsx), [StreamSettings.tsx](../../frontend/src/routes/StreamSettings.tsx), the new [RtspSection.tsx](../../frontend/src/components/stream/RtspSection.tsx), and the deleted `frontend/src/routes/Rtsp.tsx`.
  - **R5.2 (non-admin sidebar visibility):** the Sidebar diff changes only `to:` target strings and removes the RTSP row. Every `adminOnly: true` flag is preserved verbatim, the `adminOnly && !isAdmin` filter is unmodified, and Account remains the sole non-admin entry. The removed RTSP row was itself `adminOnly`, and its capability moved into Stream Config, also `adminOnly`, so the non-admin sidebar is unchanged by construction.
  - **R5.6 (backend denial):** the change contains no backend edits. `require_login`/`require_admin_login` in [rust/octocam-web/src/main.rs](../../rust/octocam-web/src/main.rs) still return `403 Admin privilege required.` for non-admin API callers, so admin-protected mutations cannot regress from a frontend route move. Note that route-level admin gating never existed in the SPA — [AuthGate.tsx](../../frontend/src/components/AuthGate.tsx) checks login only — so a non-admin loading a canonical admin slug directly renders the shell and receives API 403s, exactly as the equivalent legacy top-level URL behaved before the move.
  - **Destructive matrix rows (Wi-Fi connect/forget, Matter reset, SSH-key add/revoke, user add/delete, password/passkey changes):** the owning components `Wifi.tsx`, `Matter.tsx`, `SshKeys.tsx`, `Admin.tsx`, and `Account.tsx` are byte-identical in this change. Their endpoints and mutation payloads are unmodified, so executing those mutations on the live device would exercise untouched code. The only relocated mutation surface is Stream Config + RTSP, whose independent payloads were already captured and verified at runtime.
- **2026-08-08 — Final gate re-run before landing:** `npm run lint` reported the same four pre-existing warnings and no errors; `npm run build` succeeded. The deployed Pi still served `/` and the nested `/settings/stream` at 200 with `/api/settings` correctly returning 401 unauthenticated.
- **2026-08-08 — Final link semantics:** Dev-server shutdown surfaced Base UI warnings for the two changed anchor-rendering buttons. Added `nativeButton={false}` to the topbar Settings and dashboard RTSP controls, rebuilt, redeployed, and re-smoked both canonical destinations. Final Pi health remained active with nested and HTTPS responses at 200; frontend lint completed with four pre-existing warnings and no errors.
