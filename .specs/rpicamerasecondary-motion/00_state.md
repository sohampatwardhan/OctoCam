# Spec State: rpiCameraSecondary as the Motion-Detection Source

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Discovery](01_discovery.md) · [Requirements](02_requirements.md) · [Design](03_design.md) · [Tasks](04_tasks.md) · [Execution](05_execution.md)
<!-- spec-nav:end -->

| Gate | Status | Evidence |
|---|---|---|
| Discovery | approved | 2026-09-05 |
| Requirements | approved | 2026-09-05 |
| Design | approved | 2026-09-05 |
| Tasks | approved | 2026-09-05 |
| Audit | not_run | |
| Execution | complete | All 11 tasks verified and checked; deployed to device 2026-09-05 |

## Change Control

- 2026-09-05: Feature initiated after a scheduled watcher routine confirmed mediamtx v1.20.1
  ships the `rpiCameraSecondary` crash fix (bluenviron/mediamtx#6060 / PR #6061). Discovery
  written; no downstream artifacts exist yet.
- 2026-09-05: Discovery approved. Proceeding to requirements.
- 2026-09-05: Requirements approved (`motion_use_secondary_stream` defaults to disabled,
  per R6.2, pending on-device feasibility verification). Proceeding to design.
- 2026-09-05: Design approved. Proceeding to tasks.
- 2026-09-05: Tasks approved (10 tasks, 5 stages; user directive: auto-approve all spec gates
  and execute the tasklist). Proceeding to execution.
- 2026-09-05: Execution complete. Mid-execution repair added task 2.1 (mediamtx binary upgrade
  to v1.20.1, discovered necessary from task 1.1's on-device finding of v1.19.2 installed),
  growing the plan to 11 tasks across 5 stages. On-device feasibility test (3.1) passed 10/10
  crash-free with no regression for existing secondary-stream consumers. Final thorough review
  found and fixed one async-safety issue (unwrapped blocking subprocess call in
  `apply_settings_side_effects`) before the feature was deployed to the device with
  `motion_use_secondary_stream` defaulted off (R6.2) — no behavior change until an operator
  opts in. See [05_execution.md](05_execution.md) for full evidence.
- 2026-09-05: Pushed `feat/rpicamerasecondary-motion` and opened
  https://github.com/sohampatwardhan/OctoCam/pull/6 against `main`, per user's chosen
  integration path.
