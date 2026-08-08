# Spec State: Settings Consolidation under `/settings` (Nested Routes)

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

| Gate | Status | Evidence |
|---|---|---|
| Requirements | approved | Audit-fixed requirements approved by user 2026-08-08 |
| Design | approved | Audit-fixed design approved by user 2026-08-08 |
| Tasks | approved | Audit-fixed task plan approved by user 2026-08-08 |
| Audit | fixes_applied | Medium audit fixes applied and affected artifacts re-approved 2026-08-08 |
| Execution | complete | All tasks complete and deployed; both checkpoints passed 2026-08-08 with runtime evidence plus accepted source-level verification for the two unexecutable runtime scenarios |

## Summary

Frontend-only reorganization of the OctoCam React SPA: move all settings pages under a nested
`/settings/<slug>` base (React Router v7 `<Outlet>`), merge the standalone RTSP page into the
Stream Config page, and redirect legacy top-level URLs for bookmark back-compat. No backend/API,
auth, or RBAC change.

## Approved decisions

- URL scheme: nested path routes (`/settings/<page>`).
- Scope: all settings pages including Account move under `/settings`; Dashboard stays at `/`.

## Open questions (surface at requirements gate)

- **A2:** bare `/settings` redirect target — assumed `/settings/account` (old behavior). Confirm
  or choose a different default landing page.
- **A3:** RTSP save flow within Stream Config — assumed RTSP keeps its own save action.

## Change Control

- Design approved without material changes on 2026-08-08.
- Task plan approved on 2026-08-08; execution remains not started pending the selected audit.
- Medium audit completed on 2026-08-08 with traceability and technical/factual reviewers;
  execution was blocked until findings were applied and affected artifacts re-approved.
- Audit fixes applied on 2026-08-08. Requirements now define logged-out legacy behavior and the
  existing authorization model; design and tasks now require exhaustive behavioral verification.
  The user re-approved requirements, design, and tasks on 2026-08-08.
- Tasks 1.1, 2.1, and 3.1 were implemented, built, and deployed on 2026-08-08. Canonical routes,
  redirects, deep-link refreshes, active navigation, composed RTSP behavior, and device health passed.
- Task 5.1 closed on 2026-08-08. The user accepted source-level verification for non-admin behavior
  (R5.2, R5.6) and the destructive matrix rows, on the evidence that this change touches neither the
  backend authorization middleware nor any of the owning page components. Both checkpoints passed and
  the feature is accepted for delivery.
