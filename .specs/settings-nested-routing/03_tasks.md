# Tasks: Settings Consolidation under `/settings` (Nested Routes)

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

> [!WARNING]
> Execute dependency stages in order. Run tasks concurrently only when each is marked
> `parallel-safe`, their ownership is disjoint, and isolated worktrees are available. Stop at
> every checkpoint for human review.

- [x] 1. Compose RTSP into Stream Config
  - [x] 1.1 Extract the standalone RTSP controls into `RtspSection` and render them from Stream Config
    - Move the existing RTSP enabled, path, max-clients, save mutation, and HD/SD URL behavior into a self-contained `RtspSection` component without changing API endpoints, payloads, validation, or copy values.
    - Render `RtspSection` after the existing Stream Config form so the stream and RTSP forms retain independent save actions and mutation state.
    - Keep the standalone route component temporarily so this stage remains buildable; remove it during the router cutover.
    - **Files:** [frontend/src/components/stream/RtspSection.tsx](../../frontend/src/components/stream/RtspSection.tsx), [frontend/src/routes/StreamSettings.tsx](../../frontend/src/routes/StreamSettings.tsx)
    - **Depends on:** none
    - **Stage:** 1
    - **Documentation:** add a concise native component comment documenting that `RtspSection` owns an independent form and `useUpdateSettings()` mutation so submitting either Stream or RTSP cannot submit or reset the other.
    - **Verification:** `cd frontend && npm run build`; on the existing `/stream-settings` route, compare every RTSP control, validation rule, `PUT /api/settings` payload, and copied `/api/rtsp` HD/SD URL with the standalone page; edit both forms, submit each separately, and confirm the other form's unsaved values and mutation state remain intact; review the required component documentation.
    - **Risk:** medium; rollback removes `RtspSection` and its render call, with no data migration or backend change.
    - **Delegation:** parallel-safe
    - _Requirements: 2.1, 2.2, 2.5, 5.1, 5.3, 5.4_

- [x] 2. Cut over the router to nested settings routes
  - [x] 2.1 Add the `/settings` route prefix, legacy redirects, and remove the standalone RTSP page
    - Consult the approved design's React Router v7 technology evidence; re-query current documentation only if the installed version or routing question has changed.
    - Add the element-free `settings` prefix with all approved child slugs, an index redirect to `/settings/account`, and a settings-local unknown-path redirect to the same destination.
    - Add history-replacing redirects for every legacy settings URL, including `/rtsp` and `/stream-settings` to `/settings/stream`, while preserving `/`, public routes, `AuthGate`, `AppShell`, and the top-level catch-all.
    - Remove the obsolete `Rtsp` import and route, then delete the standalone route component.
    - **Files:** [frontend/src/App.tsx](../../frontend/src/App.tsx), `frontend/src/routes/Rtsp.tsx` (deleted)
    - **Depends on:** 1.1
    - **Stage:** 2
    - **Documentation:** no new public API; use `code-documenting` to review whether the non-obvious element-free prefix warrants a concise rationale comment.
    - **Verification:** `cd frontend && npm run build`; directly navigate to every canonical slug and confirm its page and settings chrome render; load bare and unknown settings paths; test every authenticated legacy redirect and Back behavior; load a canonical and legacy protected URL while logged out and confirm `/login` wins before child redirects; load an unknown non-settings path; review documentation surface.
    - **Risk:** medium; rollback restores the flat route table and standalone RTSP component, with no server or data migration.
    - **Delegation:** sequential subagent
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 2.3, 4.1, 4.2, 4.3, 4.4, 4.5, 5.1, 5.5, 6.2, 6.3, 6.4_

- [x] 3. Retarget canonical in-app navigation
  - [x] 3.1 Move settings navigation to canonical nested paths and remove the RTSP entry
    - Retarget every settings sidebar entry to `/settings/<slug>`, preserving `adminOnly` filtering and exact active-link behavior.
    - Remove the standalone RTSP sidebar item and any import made unused by that removal.
    - Point the topbar Account Settings control to `/settings/account` and the Dashboard RTSP shortcut to `/settings/stream`.
    - **Files:** [frontend/src/components/Sidebar.tsx](../../frontend/src/components/Sidebar.tsx), [frontend/src/components/Topbar.tsx](../../frontend/src/components/Topbar.tsx), [frontend/src/components/dashboard/StreamPreview.tsx](../../frontend/src/components/dashboard/StreamPreview.tsx)
    - **Depends on:** 2.1
    - **Stage:** 3
    - **Documentation:** no public surface; review existing navigation declarations for clarity without adding mechanical comments.
    - **Verification:** `cd frontend && npm run build`; activate every sidebar entry and confirm it reaches the canonical route with exactly one active item; verify the Account controls and Dashboard RTSP shortcut; as a non-admin, compare the sidebar against the baseline and confirm only Account remains among settings entries; confirm Dashboard remains full-width; review documentation surface.
    - **Risk:** low; rollback restores the previous link targets and RTSP item while the legacy redirects keep those links functional.
    - **Delegation:** sequential subagent
    - _Requirements: 2.4, 3.1, 3.2, 3.3, 3.4, 3.5, 5.2_

- [x] 4. Checkpoint — nested routing implementation complete
  - Confirm tasks 1.1, 2.1, and 3.1 build cleanly and review their behavior-scoped browser evidence: independent RTSP/Stream forms, canonical and legacy route outcomes, logged-out precedence, active navigation, and non-admin visibility. Do not proceed on build-only evidence.
  - Passed 2026-08-08. Runtime browser evidence covered routing, active navigation, logged-out precedence, and independent Stream/RTSP forms on the deployed Pi. Non-admin visibility was accepted on source-level evidence by user decision; see the Task 5.1 record.

- [x] 5. Verify the complete routing and settings workflow
  - [x] 5.1 Exercise canonical, legacy, authorization, mutation, and deep-link behavior
    - Run the frontend build and behavior-scoped checks, then use the browser against the development proxy to visit every canonical slug and verify exactly one active sidebar item and persistent settings chrome.
    - Verify `/` remains full-width; bare and unknown settings paths land on Account; all legacy paths replace history; other unknown paths land on `/`; and logged-out nested navigation follows the existing login flow.
    - Execute every row of the design's behavioral regression matrix, recording each page's view/copy/action result plus the observed endpoint and mutation payload; no representative sampling.
    - On Stream Config, edit both forms, save each independently, copy both RTSP URLs, and verify unsaved state and mutation state in the other form are preserved.
    - As a non-admin, compare sidebar visibility and direct legacy/canonical route outcomes with the baseline, then confirm an admin-protected backend mutation remains denied.
    - Verify keyboard order, labels, submit-button ownership, and mobile overflow on the composed Stream Config page.
    - Deploy with the repository script, confirm the health gate, directly load and refresh every canonical nested URL on the Pi, and smoke-test every legacy redirect.
    - **Files:** [frontend/src/App.tsx](../../frontend/src/App.tsx), [frontend/src/components/Sidebar.tsx](../../frontend/src/components/Sidebar.tsx), [frontend/src/components/Topbar.tsx](../../frontend/src/components/Topbar.tsx), [frontend/src/components/dashboard/StreamPreview.tsx](../../frontend/src/components/dashboard/StreamPreview.tsx), [frontend/src/components/stream/RtspSection.tsx](../../frontend/src/components/stream/RtspSection.tsx), [frontend/src/routes/StreamSettings.tsx](../../frontend/src/routes/StreamSettings.tsx)
    - **Depends on:** 3.1
    - **Stage:** 4
    - **Documentation:** review all changed modules with `code-documenting`; confirm comments describe only non-obvious contracts and rationale.
    - **Verification:** `cd frontend && npm run build`; browser checks for every correctness property in [.specs/settings-nested-routing/02_design.md](02_design.md); [scripts/deploy-pi-web.sh](../../scripts/deploy-pi-web.sh) and direct-load/refresh smoke tests against the deployed Pi.
    - **Risk:** medium; stop before deployment on any build or browser regression, and redeploy the previous `main` artifact if the device smoke test fails. No schema migration is involved.
    - **Delegation:** controller
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 2.1, 2.2, 2.3, 2.4, 2.5, 3.1, 3.2, 3.3, 3.4, 3.5, 4.1, 4.2, 4.3, 4.4, 4.5, 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 6.1, 6.2, 6.3, 6.4_

- [x] 6. Checkpoint — ready for delivery
  - Review browser and Pi evidence, confirm all required tasks are complete, and obtain final human acceptance.
  - Accepted 2026-08-08. The user reviewed the runtime evidence plus the change-surface argument for the two unexecuted runtime scenarios and accepted source-level verification for both.

> [!IMPORTANT]
> Approval gate: approve these tasks before implementation begins.
