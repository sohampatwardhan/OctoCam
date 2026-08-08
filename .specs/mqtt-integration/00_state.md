# Spec State: MQTT Integration for Home Assistant

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

| Gate | Status | Evidence |
|---|---|---|
| Requirements | approved | Re-approved after audit fixes 2026-08-08 (R6.4 split into R6.4/R6.5/R6.6) |
| Design | approved | Re-approved after audit fixes 2026-08-08 |
| Tasks | approved | Re-approved after audit fixes 2026-08-08 |
| Audit | fixes_applied | All 11 medium-audit findings applied and artifacts re-approved 2026-08-08 |
| Execution | complete | All required tasks done and verified against the broker container; task 7.2 (live Home Assistant) deferred, no instance yet |

## Summary

Add MQTT publishing to OctoCam so Home Assistant automatically discovers the camera's motion
sensor and receives motion state changes. No MQTT capability exists in the repository today;
this is a from-scratch feature spanning the Rust web service, the settings store, and a new
admin-only `/settings/mqtt` page.

## Approved decisions

Confirmed by the user before requirements were drafted:

- **Entity scope:** one motion binary sensor plus an availability topic. No camera entity, no
  diagnostic entities, no per-zone entities.
- **Broker authentication:** username and password, with TLS available but optional, matching a
  typical local Mosquitto or Home Assistant add-on setup.
- **Direction:** publish only. OctoCam accepts no commands over MQTT, so no command topics and
  no new remote-control surface.

## Context corrections

- Motion detection is camera-based but does **not** use OpenCV, contrary to an initial
  description. [`rust/octocam-web/src/motion.rs`](../../rust/octocam-web/src/motion.rs) uses
  ffmpeg downscaling plus pixel-difference thresholding in Rust. The available signal is binary
  with zone masking, so no object classification can be published to Home Assistant.

## Open questions — resolved at the design gate

1. **Password at rest — accepted.** Stored plaintext in the root-owned settings file, with every
   off-device path closed instead: redacted from the API, omitted from backups, never logged.
   This was flagged as a security-posture judgement rather than a mechanical choice, and the user
   accepted it by approving the design.
2. **Base topic prefix.** Resolved as the fixed literal `octocam` plus a stable node id, so
   renaming the camera does not orphan retained topics under an old prefix.
3. **Disabled detection.** Resolved as unavailable rather than removed, via a second availability
   topic with `availability_mode: all`, so Home Assistant keeps automations and history.

## Verification environment

A containerized broker stands in for a real one, since none is available yet:

- Container `octocam-mqtt` (`eclipse-mosquitto:2`), `--restart unless-stopped`, listening on
  `127.0.0.1:18830`. Config and persistence live in `~/.octocam-dev-mqtt/`.
- Anonymous and unencrypted deliberately — it is bound to the developer Mac and holds no real
  credentials. It must not be reused for anything reachable.
- The image ships `mosquitto_sub`/`mosquitto_pub`, so broker checks run via `docker exec` with
  nothing installed on the host. Verified: a retained publish is replayed to a fresh subscriber,
  which is the mechanism R2.1 and R3.3 depend on.
- Task 7.2 (live Home Assistant confirmation) stays deferred until the user has an instance.

## Change control

- 2026-08-08 — Requirements drafted from a three-question scoping exchange covering entity scope,
  broker authentication, and publish direction.
- 2026-08-08 — Design drafted against current rumqttc 0.25.1 and Home Assistant discovery
  documentation, resolving all three open questions.
- 2026-08-08 — All 11 audit findings applied. Requirements: R6.4 split into three criteria, so
  the criterion count is now 43. Design: corrected the rumqttc rationale (the build already
  compiles C via rusqlite's bundled feature, so "avoids a C toolchain" was false), extended the
  reload signal to all four settings writers, classified `mqtt_password` and `mqtt_node_id` into
  `EXCLUDED_FIELDS`, and made the `Debug`-dump leak an explicit security concern. Tasks: closed
  six verification gaps in 3.1, added the R6.2 browser check to 6.1, switched broker verification
  to the container, split the deferred Home Assistant check into optional task 7.2, and added the
  Q1 decision to the delivery checkpoint. All artifacts re-approved by the user.
- 2026-08-08 — Medium audit completed with traceability and technical/factual reviewers. Coverage
  chain, task graph, credential containment, motion-signal assumptions, and frontend claims all
  verified clean. Findings open against the design's crate rationale, the settings-reload path,
  eight verification gaps, and the unavailable broker tooling. No P0.
- 2026-08-08 — User stated no MQTT broker is available yet, so the on-device Home Assistant
  verification in task 7.1 must be deferrable without blocking the rest of the feature.
- 2026-08-08 — Tasks approved by the user, then a medium audit was selected before execution
  because the feature stores a broker credential and opens a new network egress path.
- 2026-08-08 — Tasks drafted: 6 leaf tasks across 6 dependency stages, all sequential because
  each stage modifies files an earlier stage created. Validation passes with all 42 acceptance
  criteria traced. Not yet approved.
- 2026-08-08 — Design approved by the user without changes. Approval carries the Q1 security
  posture: the broker password is stored unencrypted on disk, protected by file ownership and by
  excluding it from the API, backups, and logs.
- 2026-08-08 — Requirements approved by the user without changes. The three open questions above
  were not answered at the gate, so design carries them: it will propose a resolution for each and
  surface them again at the design gate. Q1 (password at rest) is a security-posture decision and
  is called out explicitly rather than silently defaulted.


## Delivery (2026-08-08)

- **Q1 resolved — plaintext at rest, approved by the user.** The broker password is stored
  unencrypted in the root-owned settings file, protected by exclusion from the API, backups, and
  logs (a `Secret` newtype that will not print itself). This matches how `admin_password_hash`
  already lives on the device.
- All required tasks complete. Task 7.2 (confirming the entity in a live Home Assistant) remains
  deferred until the user has a broker/HA instance; the protocol behaviour it would confirm is
  already verified against the local Mosquitto container.
- History was rebuilt into four clean commits after a directory-level `git add` had mixed the
  user's motion work into the MQTT commits. The rebuilt tree was verified byte-identical to the
  pre-rewrite state.
