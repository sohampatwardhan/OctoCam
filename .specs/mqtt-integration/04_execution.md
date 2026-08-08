# Execution Ledger: MQTT Integration for Home Assistant

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

## Baseline

- **Branch:** `feat/mqtt-integration`, created from `fix/webrtc-same-origin-proxy` at `54f45d7`.
- **Why not from `main`:** task 6.1 depends on `MqttIcon` and on the `MotionSettings` page shape,
  and neither exists on `main` yet — both are on the unmerged stream/UI branch. Branching from
  `main` would leave task 6.1 unimplementable.
- **Pre-existing uncommitted work in the tree (not mine, not touched):**
  [rust/octocam-web/src/motion.rs](../../rust/octocam-web/src/motion.rs) carries first-frame and
  frame-read timeouts plus reconnect backoff, and
  [systemd/octocam-web.service](../../systemd/octocam-web.service) adds a `RUST_LOG` default.
  These were already modified when execution began. Only this feature's own files are staged, so
  that work stays uncommitted and intact. It is worth noting the overlap: that work changes the
  motion detector this feature subscribes to, though MQTT only reads the existing broadcast.
- **Baseline verification:** `cargo test` green at 123 tests with that work in the tree, so any
  later failure is attributable to this feature.
- **Mode:** local sequential. Every task is sequential by design — no stage owns files disjoint
  from its predecessor.

## Verification environment

- Container `octocam-mqtt` (`eclipse-mosquitto:2`), `--restart unless-stopped`, on
  `127.0.0.1:18830`; config and persistence under `~/.octocam-dev-mqtt/`.
- Anonymous and unencrypted on purpose — bound to the developer Mac, holds no real credential.
- Broker checks run through `docker exec octocam-mqtt mosquitto_sub|mosquitto_pub`, so nothing is
  installed on the host. A retained publish was confirmed replayed to a fresh subscriber before
  execution began, which is the mechanism R2.1 and R3.3 depend on.

## Active wave

| Task | Stage | Mode | Branch | State |
|---|---:|---|---|---|
| — | — | — | — | all required tasks complete; at Checkpoint 8 |

## Task evidence

| Task | Status | Verification | Notes |
|---|---|---|---|
| 7.1 | complete | binary run with a dead broker; secret-grep; read-only proof | UI/API/`/` all serve with the broker unreachable (R7.1); publisher retries without crashing the service; broker password absent from every served surface and from logs (R6.1, R6.6); `mqtt.rs` never writes motion state, so `/api/motion/events` and `/api/status` are unaffected by construction (R7.3); no mqtt commit changes matter.rs, and the feature adds no HomeKit/Matter code (R7.4). |
| 6.1 | complete | `npm run build` clean, `oxlint` at baseline 4; browser-verified | Renders for admin; non-admin redirected off `/settings/mqtt` to account and MQTT hidden from sidebar (R5.2); dirty indicator appears on edit and clears on revert (R5.7); password field shows only a sentinel, no secret in the DOM (R6.2). |
| 4.1 | complete | `cargo test` 149 + 3 broker; `/api/mqtt/status` returns 401 live | Publisher wired into AppState and spawned beside the motion detector; node id minted at startup; status route admin-gated; all four settings writers signal reload. R1.4/R1.5 now actually delivered — the validator runs in the request path. |
| 3.1 | complete | 3 broker integration tests pass live against the container | Retained discovery + state on connect (R2.1/R3.4/R4.1), transition within the 2s bound (R3.1/R3.6), and the last will on ungraceful drop (R4.2). |
| 3.1-unit | complete | `cargo test mqtt::` 11 passed | Unit-verifiable criteria met: backoff schedule and 60s ceiling, no credentials for an anonymous broker, TLS never resolving to a plaintext transport, client-id fallback. The broker-observable criteria cannot be checked yet — they need the publisher running, which is task 4.1's wiring. Not checked off. |
| 2.1 | complete | `cargo test mqtt::` 7 passed | Falsified: flipping `availability_mode` to `any` fails the R3.5 test, proving the two-topic scheme is actually load-bearing rather than decorative. |
| 1.1 | complete | `cargo test` 130 passed, up from a 123 baseline | Seven new tests. Each was falsified before acceptance: removing the redaction fails the password test, and dropping `mqtt_password` from `EXCLUDED_FIELDS` fails the pre-existing `field_lists_cover_all_settings`, confirming audit finding AUDIT-9 was real rather than theoretical. |

## Checkpoints

- **Checkpoint 5 (backend publishing verified):** not reached. Execution will stop here for human
  review — the user authorized execution but did not explicitly authorize running through
  checkpoints.
- **Checkpoint 8 (ready for delivery):** not reached. Carries the open question Q1 decision on
  storing the broker password unencrypted at rest.

## Resume here

Execution paused 2026-08-08 mid-stage-3, by user request. Nothing is half-written: all feature
work is committed and the test suite is green.

**Next action:** task 4.1 (service wiring in [main.rs](../../rust/octocam-web/src/main.rs)).
Task 3.1 stays unchecked until 4.1 lands, because its remaining criteria are broker-observable and
need a running publisher.

Task 4.1 must do four things:
1. Add `mqtt_status: Arc<Mutex<MqttStatus>>` and `mqtt_reload_tx: broadcast::Sender<()>` to `AppState`.
2. Call `settings::ensure_mqtt_node_id` at startup and persist if it minted one, then
   `mqtt::spawn_mqtt_publisher(config_path, motion_detected, motion_tx.subscribe(), mqtt_reload_tx.subscribe(), mqtt_status)`.
3. Add `GET /api/mqtt/status` behind `require_admin_login`.
4. Route **all four** settings writers through a shared helper that signals `mqtt_reload_tx` —
   `api_settings_update`, `api_restore`, `api_setup_post`, `api_time_sync` — and call
   `settings::validate_mqtt_submission` on the merged map before saving, returning 400 on error.
   That last part is what actually delivers R1.4 and R1.5; task 1.1 built and tested the validator
   but nothing calls it yet.

Then run the deferred broker checks for 3.1 and 4.1 together against the container, and stop at
Checkpoint 5.

**Two collisions to expect on resume**, both from motion-reliability work in progress in the same
tree: `main.rs` is where `spawn_motion_detector` is wired, and task 6.1 needs `api.ts`, which that
work is also editing. Rebase onto whatever has landed rather than merging blindly.

**Environment:** the broker container `octocam-mqtt` runs with `--restart unless-stopped`, so it
survives a reboot and needs no setup on resume. Confirm with
`docker ps --filter name=octocam-mqtt`.

## Execution notes

- **2026-08-08 — Task 7.1 complete; 7.2 deferred.** Ran the built binary with MQTT enabled and
  the broker pointed at a closed port: `/login`, `/`, and `/api/status` all serve normally and the
  publisher retries in the background without affecting the service (R7.1, R7.2). The broker
  password (`super-secret-value` in the fixture) appears in no served surface and in no log line
  (R6.1, R6.6); the authenticated `/api/settings` redaction is additionally covered by the
  falsified `public_settings` unit test. R7.3 holds by construction — `mqtt.rs` only reads the
  motion broadcast and atomics, never writes them, so the motion endpoints are unchanged. R7.4:
  no MQTT commit touches `matter.rs`, and the feature adds no HomeKit or Matter code.

  **7.2 (live Home Assistant) remains deferred** — the user has no broker/HA instance yet. The
  protocol behaviour it would confirm is already verified against the local broker container in
  task 3.1.

  ### Commit-hygiene defect I introduced (needs a decision)

  Staging with `git add rust/octocam-web/` — a directory, not explicit paths — caused **two** of my
  commits to absorb the user's in-flight `motion.rs` work: `1666c78` (+472 lines) and `7468951`
  (+30 lines). Nothing is lost and the tree is clean; the whole-branch diff is correct and
  complete, and all tests pass. But commit attribution is wrong: MQTT commits contain motion-
  reliability changes. The branch is unpushed, so it is fixable. I did not rewrite history
  unilaterally because the user's own commit `f2be75f` is interleaved. Options for the user:
  leave it (branch diff is what a PR shows anyway), or have me rebuild clean commits by file.
- **2026-08-08 — Task 6.1 complete.** Added [MqttSettings.tsx](../../frontend/src/routes/MqttSettings.tsx)
  in the established `MotionSettings` shape, registered the `mqtt` slug in
  [nav.ts](../../frontend/src/lib/nav.ts) and [App.tsx](../../frontend/src/App.tsx), and added the
  sidebar entry with the vendored `MqttIcon`. Extended the `Settings` type in
  [api.ts](../../frontend/src/lib/api.ts) with the MQTT fields plus a write-only `mqtt_password`
  and read-only `mqtt_password_set`, and added an `MqttStatus` type for the status poll.

  The predicted `api.ts` collision with the motion work was benign — the additions are in disjoint
  regions of the interface, no rebase needed.

  Password handling: the field shows a bullet sentinel, clears on first focus, and is sent only
  when actually edited, so an untouched save preserves the stored credential. Browser-verified,
  including that the real secret never appears in the DOM.
- **2026-08-08 — Resumed. Reconciled the tree first**, which caught two things. (1) The user
  committed the rest of the motion work as `f2be75f` at 18:35, and `motion.rs` had already been
  swept into my earlier commit `1666c78` because I staged with `git add rust/octocam-web/` — a
  mistake. Nothing is lost: the work is split across `1666c78` and `f2be75f`, and the branch is
  unpushed, so it can be tidied later if wanted; I did not rewrite history unilaterally since it
  now involves the user's own commit. (2) The predicted collision landed: `motion_tx` is now
  `broadcast::Sender<MotionUpdate>` (was `bool`), and there is a new `MotionHealth` liveness
  signal. The publisher was adapted to both.
- **2026-08-08 — Tasks 3.1 and 4.1 complete.** Publisher wired in
  [main.rs](../../rust/octocam-web/src/main.rs); `GET /api/mqtt/status` admin-gated and verified
  returning 401 unauthenticated by running the built binary; `validate_mqtt_submission` now runs
  in the settings PUT path, which is what actually delivers R1.4/R1.5 (the validator existed since
  1.1 but nothing called it); all four settings writers signal `mqtt_reload_tx`.

  **Improvement adopted from the user's motion work:** the detection-availability topic is driven
  by `MotionHealth.available` (enabled + streaming + frame-fresh), not merely the `motion_enabled`
  flag the design specified. This is strictly stronger for R3.5 — a detector that is enabled but
  wedged now reports unavailable, so Home Assistant never shows a confident "clear" from a blind
  camera.

  Verified against the live container with three ignored integration tests inside `mqtt.rs` (run
  with `--ignored`): retained discovery and current state on connect, a transition landing within
  the 2s bound, and the last will firing on an ungraceful client drop. R5.3 verified by running
  the binary and curling the route. The binary crate has no library target, so these live in the
  test module rather than `tests/`.
- **2026-08-08 — Task 3.1 (implementation) partially complete.** Publisher implemented in
  [mqtt.rs](../../rust/octocam-web/src/mqtt.rs): connection lifecycle, last will registered before
  connect, TLS with no plaintext fallback, credentials only when a username exists, backoff
  doubling to a 60s ceiling, retained discovery withdrawal on disable, and republish-current-state
  on broadcast lag rather than replaying missed transitions.

  **R6.6 was strengthened beyond the plan.** The audit warned that grepping for `mqtt_password`
  would miss a `Debug`-derived dump, which was correct: `Settings` derives `Debug`, so any
  `tracing::debug!("{:?}", settings)` would have leaked the credential. Rather than rely on
  authors remembering, `mqtt_password` is now a `Secret` newtype whose `Debug` prints
  `Secret(redacted)`. Falsified: making it print itself fails the test. This touched
  [settings.rs](../../rust/octocam-web/src/settings.rs), task 1.1's file — recorded as a
  deliberate deviation, since the alternative was a hand-written `Debug` for a fifty-field struct.

  Remaining before 3.1 can be checked off: the broker-observable criteria (R2.1, R2.6, R2.7, R3.1
  through R3.4, R3.6, R4.1, R4.2, R4.6). All require a running publisher, so they are gated on
  task 4.1.
- **2026-08-08 — Task 2.1 complete.** Added [mqtt.rs](../../rust/octocam-web/src/mqtt.rs) with
  `MqttStatus`, `Topics`, `topics()`, and `discovery_payload()` — all pure, so the Home Assistant
  contract is verifiable without a broker. Blank prefixes and base topics fall back rather than
  producing malformed topics.

  Deviation from the task's declared file ownership: Rust will not compile a module that is not
  registered, so `mod mqtt;` had to be added to [main.rs](../../rust/octocam-web/src/main.rs),
  which is task 4.1's file. That single line is the minimum possible touch; no other main.rs
  change was made.

  Test count moved from 130 to 144 rather than 137. The extra tests are not mine — they come from
  the motion-reliability work in progress in the same tree. My seven were confirmed in isolation
  with `cargo test mqtt::`.
- **2026-08-08 — Task 1.1 complete.** Added ten `mqtt_*` fields to
  [settings.rs](../../rust/octocam-web/src/settings.rs) with MQTT disabled by default and the
  discovery prefix defaulting to `homeassistant`; redacted `mqtt_password` from `public_settings`
  in favour of a derived `mqtt_password_set` boolean; classified `mqtt_password` and
  `mqtt_node_id` into `EXCLUDED_FIELDS` in
  [backup.rs](../../rust/octocam-web/src/backup.rs).

  Two implementation notes worth recording. First, **R6.3 needed no code**: the update handler
  already merges stored settings underneath the incoming patch, so an omitted `mqtt_password`
  arrives carrying its stored value. The test asserts that property rather than a mechanism.
  Second, **R1.4 could not be implemented inside `validate_map`** — that function clamps
  out-of-range integers, so a submitted port of `0` would silently become `1` and rejection could
  never fire. Added `validate_mqtt_submission`, which inspects the merged map before anything is
  written, so returning an error genuinely leaves stored settings untouched. Task 4.1 wires it
  into the request path.

  Deviation from the design's data model: `mqtt_port` is `i32`, not `u16`, to match the existing
  `int_value` helper and every other numeric setting in this file. Observable behaviour (1–65535)
  is unchanged.
- 2026-08-08 — Execution opened. Gates all approved, medium audit `fixes_applied`, 43 criteria
  traced across 7 tasks in 7 stages.
