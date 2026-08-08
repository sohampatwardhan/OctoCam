# Tasks: MQTT Integration for Home Assistant

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

> [!WARNING]
> Execute dependency stages in order. Every task here is sequential — each stage touches files
> the previous stage created or modified, so nothing is parallel-safe. Stop at both checkpoints
> for human review.

- [x] 1. Settings model and credential containment
  - [x] 1.1 Add MQTT settings fields with validation, redaction, and backup exclusion
    - Add the ten `mqtt_*` fields to `Settings` with the defaults from the design, so an existing settings file loads unchanged and MQTT starts disabled.
    - Generate and persist `mqtt_node_id` once if absent, giving Home Assistant a `unique_id` that survives restarts and device renames.
    - In `validate_map`, reject an out-of-range port and an enabled-with-empty-host submission by retaining the previously stored values.
    - In `validate_map`, preserve the stored password when the incoming map omits `mqtt_password`, and store an empty password when it is explicitly cleared.
    - In `public_settings`, remove `mqtt_password` alongside the existing `admin_password_hash` removal, and insert a derived `mqtt_password_set` boolean.
    - Add the broker connection fields to `PORTABLE_FIELDS` and omit `mqtt_password`, so a settings backup can never carry the broker credential off-device.
    - Add both `mqtt_password` and `mqtt_node_id` to `EXCLUDED_FIELDS`. The existing `field_lists_cover_all_settings` test fails if any field is in neither list, and `mqtt_node_id` must not travel between devices or two cameras would claim the same Home Assistant entity.
    - **Files:** [rust/octocam-web/src/settings.rs](../../rust/octocam-web/src/settings.rs), [rust/octocam-web/src/backup.rs](../../rust/octocam-web/src/backup.rs)
    - **Depends on:** none
    - **Stage:** 1
    - **Documentation:** document on the `mqtt_password` field why it is excluded from `public_settings`, `PORTABLE_FIELDS`, and logs, and on `mqtt_node_id` why identity is generated rather than derived from the device name or MAC. State the contract, not the mechanics.
    - **Verification:** `cargo test`; new unit tests asserting that `public_settings` output contains no `mqtt_password` key but does contain `mqtt_password_set`; that `build_backup` output omits `mqtt_password` while including the non-secret fields; that a port of `0` or `65536` leaves stored settings unchanged; that enabling with an empty host leaves stored settings unchanged; that omitting `mqtt_password` from a PUT map preserves the stored value; that a settings file written before this change still deserializes; and that `field_lists_cover_all_settings` still passes with the new fields classified.
    - **Risk:** low; additive fields with defaults and no migration. Rollback removes the fields, leaving unknown keys in existing settings files, which the loader already tolerates.
    - **Delegation:** sequential subagent
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 6.1, 6.2, 6.3, 6.4_

- [x] 2. Topic and payload construction
  - [x] 2.1 Build the pure topic and discovery-payload functions
    - Create the module with `MqttStatus`, `Topics`, `topics(settings)`, and `discovery_payload(settings)`. No network code in this task, so every behavior here is testable without a broker.
    - Derive the discovery topic as `<discovery_prefix>/binary_sensor/<unique_id>/config`, applying the `homeassistant` default when the prefix is empty, and omitting the optional node-id segment per the design's evidence.
    - Build the state, service-availability, and detection-availability topics from the base topic and node id.
    - Build the discovery payload with `name: null`, `device_class: motion`, the restart-stable `unique_id`, the `device` block carrying the configured device name, and the two-entry `availability` array with `availability_mode: all`.
    - Consult the design's Current Technology Evidence before writing the payload; re-query `context7-mcp` for the Home Assistant discovery schema only if the recorded findings look stale.
    - **Files:** `rust/octocam-web/src/mqtt.rs`
    - **Depends on:** 1.1
    - **Stage:** 2
    - **Documentation:** module-level comment explaining that this module is the only place linking against the MQTT client and that it consumes existing motion state rather than owning it. Document why the availability pair exists — one topic alone cannot distinguish "detection disabled" from "no motion".
    - **Verification:** `cargo test`; unit tests asserting the discovery topic shape including the default prefix, all four topic strings for a known settings fixture, `device_class: motion`, `name: null`, a `unique_id` that is stable when the device name changes, and an `availability` array of exactly two entries with `availability_mode: all`.
    - **Risk:** low; pure functions with no side effects and no external dependency.
    - **Delegation:** sequential subagent
    - _Requirements: 1.3, 2.2, 2.3, 2.4, 2.5, 3.5_

- [x] 3. Publisher task
  - [x] 3.1 Implement the connection lifecycle and publishing loop
    - Add `rumqttc` to the manifest with the rustls-backed TLS feature, keeping the build free of a C toolchain so the existing Docker `aarch64` cross-compile still works.
    - Implement `spawn_mqtt_publisher`, mirroring the shape of `spawn_motion_detector` so the two read alike.
    - Configure `MqttOptions` with client id, keep-alive, credentials when a username is set, and TLS transport when enabled; never fall back to plaintext when TLS is configured.
    - Register the last will on the service availability topic so an ungraceful termination still marks the device offline.
    - On connect: publish retained `online` to service availability, the retained discovery config, and the current motion state read from the shared `AtomicBool` without waiting for a transition.
    - Publish retained `ON`/`OFF` on each motion transition received from the broadcast receiver; on receiver lag, republish current state rather than replaying missed transitions.
    - Publish the detection-availability topic as `online`/`offline` tracking the `motion_enabled` setting.
    - On settings reload: republish the discovery config when identity-affecting fields change, and reconnect when connection fields change.
    - On disable: clear the retained discovery config with an empty retained payload, publish `offline`, and disconnect without opening any further connection.
    - Reconnect with exponential backoff capped at 60 seconds; record failure reasons, including authentication rejection, in `MqttStatus` and keep retrying rather than exiting.
    - Ensure no log statement includes the password, including any `Debug`-formatted dump of the settings struct or of `MqttOptions`.
    - Connect without credentials when no password is stored, rather than sending an empty one.
    - **Files:** `rust/octocam-web/src/mqtt.rs`, [rust/octocam-web/Cargo.toml](../../rust/octocam-web/Cargo.toml)
    - **Depends on:** 2.1
    - **Stage:** 3
    - **Documentation:** document the task's failure contract — it never exits, never propagates broker failure to callers, and treats broker state as best-effort. Explain why backoff is capped and why lag is handled by resync rather than replay.
    - **Verification:** `cargo test`, including unit tests for: the backoff sequence reaching and holding at 60s; the disable path producing an empty retained discovery payload; `MqttOptions` carrying no credentials when no password is stored (R6.5); a TLS handshake failure producing a retry and never a plaintext attempt (R1.6); a device-name change producing a republished discovery payload (R2.7); and `Debug` output for the settings struct and `MqttOptions` containing no password value (R6.6).
      Against the containerized broker (`docker run -d --name octocam-mqtt -p 18830:1883 -v <conf>:/mosquitto/config/mosquitto.conf eclipse-mosquitto:2`, anonymous listener; the image ships `mosquitto_sub`/`mosquitto_pub` so nothing is installed on the host), run `docker exec octocam-mqtt mosquitto_sub -v -t '#'` and assert: retained discovery on connect; **the current motion state published on connect without waiting for a transition (R3.4)**; `ON`/`OFF` on each transition; `online` on connect; `offline` after `kill -9` proving the last will (R4.2); the detection-availability topic flipping to `offline` when `motion_enabled` is turned off and back on when re-enabled (R3.5); and no connection attempts at all while disabled (R4.6).
      Measure publish latency against the 2s bound (R3.6) by timestamping the transition and the received message; take the measurement while the Pi is under its normal streaming load, not idle, since the device already runs at load 2–6.
      Review the documentation surface.
    - **Risk:** medium; a misbehaving loop could spin or leak connections. Bounded by the backoff cap and by the task owning the only client handle. Rollback is removing the spawn call, which leaves retained topics on the broker until manually cleared.
    - **Delegation:** sequential subagent
    - _Requirements: 1.6, 2.1, 2.6, 2.7, 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 4.1, 4.2, 4.4, 4.5, 4.6, 6.5, 6.6_

- [x] 4. Service wiring
  - [x] 4.1 Wire the publisher into the web service and expose its status
    - Add `mqtt_status: Arc<Mutex<MqttStatus>>` and `mqtt_reload_tx: broadcast::Sender<()>` to `AppState`, and register the new module.
    - Spawn the publisher at startup, subscribing it to the existing `motion_tx` so motion detection, `/api/motion/events`, and `/api/status` are untouched.
    - Add `GET /api/mqtt/status` behind `require_admin_login`, returning the current state, last error, and connected-since timestamp.
    - Introduce a shared "settings changed" helper that saves and then signals `mqtt_reload_tx`, and route **every** settings writer through it: `api_settings_update`, `api_restore`, `api_setup_post`, and `api_time_sync`. Restore is the one that matters — broker fields are in `PORTABLE_FIELDS`, so a restored backup changes MQTT configuration and must not be silently ignored until the next restart.
    - Publish `offline` to the service availability topic on clean shutdown, before the process exits.
    - **Files:** [rust/octocam-web/src/main.rs](../../rust/octocam-web/src/main.rs)
    - **Depends on:** 3.1
    - **Stage:** 4
    - **Documentation:** document on the new route why status is admin-only and why it is read-only, and on the reload signal why settings changes are pushed rather than polled.
    - **Verification:** `cargo test`; a test asserting `/api/mqtt/status` returns 403 for a non-admin session and 200 for an admin. Against the containerized broker: change a broker setting via the API and confirm the publisher reconnects without a service restart; **restore a backup carrying different MQTT settings and confirm the publisher picks them up without a restart**; `systemctl stop octocam-web` and confirm `offline` arrives on the availability topic.
    - **Risk:** medium; touches service startup, so a panic here costs the whole web service. Mitigated by the publisher never returning an error to the caller.
    - **Delegation:** sequential subagent
    - _Requirements: 4.3, 5.3, 5.6_

- [ ] 5. Checkpoint — backend publishing verified
  - Confirm against the containerized broker that discovery, motion transitions, birth, and last will all behave as designed, and that disabling MQTT clears the retained config. Do not proceed on unit tests alone; retained messages and last-will only exist as broker behavior and cannot be proven in-process.

- [x] 6. Configuration page
  - [x] 6.1 Build the MQTT settings page and route it
    - Create the page following the `MotionSettings` shape already used in this repo: local form state seeded from settings, its own save action, and its own mutation.
    - Render fields for enabled, host, port, username, password, TLS, client id, base topic, and discovery prefix. Show that a password is set without revealing it, and leave it untouched on save unless the admin edits it.
    - Poll `GET /api/mqtt/status` and present connected, disconnected, or disabled, surfacing the latest failure reason while failing.
    - Report dirty state through the shell's unsaved-changes indicator, matching the other settings pages.
    - Add the `mqtt` slug to `ADMIN_ONLY_SETTINGS_SLUGS` and the page to `ADMIN_ONLY_PAGES`; the total `Record` makes a missing page a compile error, which is what keeps the route guard and sidebar in step.
    - Add the sidebar entry using the already-vendored `MqttIcon`.
    - Extend the settings API types with the new fields and `mqtt_password_set`.
    - **Files:** `frontend/src/routes/MqttSettings.tsx`, [frontend/src/lib/nav.ts](../../frontend/src/lib/nav.ts), [frontend/src/App.tsx](../../frontend/src/App.tsx), [frontend/src/components/Sidebar.tsx](../../frontend/src/components/Sidebar.tsx), [frontend/src/lib/api.ts](../../frontend/src/lib/api.ts)
    - **Depends on:** 4.1
    - **Stage:** 5
    - **Documentation:** document on the page component why the password field is write-only and how an unedited password is preserved, so a later change does not accidentally blank stored credentials.
    - **Verification:** `npm run build` and `npx oxlint`, both clean with no new warnings. In the browser: the page renders for an admin; a stubbed non-admin `/api/me` redirects away from `/settings/mqtt`; editing a field raises the unsaved indicator and saving clears it; the status area reflects a stopped broker; **the page shows that a password is set without rendering its value anywhere in the DOM (R6.2)**; and saving without touching the password leaves the connection working.
    - **Risk:** low and frontend-only; rollback removes the route and slug together, which the total `Record` enforces.
    - **Delegation:** sequential subagent
    - _Requirements: 5.1, 5.2, 5.4, 5.5, 5.7, 6.2_

- [x] 7. Isolation and end-to-end verification
  - [x] 7.1 Prove MQTT cannot affect existing functionality, and verify against Home Assistant
    - With the broker stopped and MQTT enabled, confirm the web UI, streams, API, and motion detection behave exactly as with MQTT disabled.
    - Capture `/api/status.motion_detected` and `/api/motion/events` output with MQTT enabled and disabled, and confirm they are identical.
    - Confirm HomeKit and Matter behavior is unchanged with MQTT enabled.
    - Confirm no API response body contains the broker password, by inspecting `/api/settings` and a downloaded backup.
    - **Files:** [.specs/mqtt-integration/04_execution.md](04_execution.md)
    - **Depends on:** 6.1
    - **Stage:** 6
    - **Documentation:** no public surface; this task records evidence rather than adding code.
    - **Verification:** the checks above, each recorded in the execution ledger with the observed result rather than an assertion that it passed.
    - **Risk:** low; observation only. The Home Assistant check runs against the user's live broker, so it publishes real retained topics — disabling MQTT afterwards clears them if the feature is not kept.
    - **Delegation:** controller
    - _Requirements: 6.1, 7.1, 7.2, 7.3, 7.4, 7.5_

  - [ ]* 7.2 Confirm the entity in a live Home Assistant instance
    - Deferred: needs a Home Assistant instance and its broker, which are not available yet. Everything else in this feature is verifiable against the containerized broker without it.
    - Point OctoCam at the Home Assistant broker and confirm the motion entity auto-appears with no hand-written YAML.
    - Confirm the entity tracks motion, greys out when OctoCam stops, and greys out when motion detection is disabled while OctoCam stays online.
    - Confirm the entity survives an OctoCam restart without duplicating, proving the identifier really is stable.
    - **Files:** [.specs/mqtt-integration/04_execution.md](04_execution.md)
    - **Depends on:** 7.1
    - **Stage:** 7
    - **Documentation:** no public surface; records evidence only.
    - **Verification:** the observations above, recorded in the execution ledger with what was actually seen. This is confirmation in the real consumer, not the primary proof — the containerized broker checks in tasks 3.1 and 4.1 already establish the protocol behavior.
    - **Risk:** low; publishes real retained topics into the user's Home Assistant. Disabling MQTT afterwards clears them.
    - **Delegation:** controller
    - _Requirements: 2.1, 2.3, 3.1, 3.5_

- [x] 8. Checkpoint — ready for delivery
  - Review the broker evidence, confirm the credential never appears in an API response or backup, and obtain final human acceptance.
  - **Affirm or reject open question Q1**: the broker password is stored unencrypted in the settings file, protected only by file ownership and by exclusion from the API, backups, and logs. The design proposed this rather than settling it, so it needs an explicit decision here rather than passing by default.
  - Note whether task 7.2 is being deferred, so delivery is not mistaken for full Home Assistant confirmation.

> [!IMPORTANT]
> Approval gate: approve these tasks before implementation begins.
