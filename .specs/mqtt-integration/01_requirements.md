# Requirements: MQTT Integration for Home Assistant

<!-- spec-nav:start -->
**Spec navigation:** [State](00_state.md) · [Requirements](01_requirements.md) · [Design](02_design.md) · [Tasks](03_tasks.md) · [Execution](04_execution.md)
<!-- spec-nav:end -->

## Overview

OctoCam currently has no MQTT capability of any kind. This feature adds one: OctoCam
connects to an MQTT broker, advertises itself to Home Assistant using Home Assistant's
MQTT discovery convention, and publishes its motion state so Home Assistant automations
can react to movement without polling OctoCam's HTTP API.

Scope is deliberately narrow for this first iteration: **one motion sensor entity plus
availability, publish-only.** OctoCam never accepts commands over MQTT.

## Domain terms

| Term | Meaning |
|---|---|
| **Broker** | The MQTT server OctoCam connects to (commonly Mosquitto, often the Home Assistant add-on). |
| **Discovery** | Home Assistant's convention where a device publishes a retained JSON config describing an entity, and Home Assistant creates that entity automatically. |
| **Discovery prefix** | Topic root Home Assistant watches for discovery configs. Home Assistant's default is `homeassistant`. |
| **Availability topic** | A topic carrying `online`/`offline` so Home Assistant can grey out entities when OctoCam is gone. |
| **LWT (Last Will and Testament)** | A message the broker publishes on OctoCam's behalf if the connection drops without a clean disconnect. |
| **Retained message** | A message the broker stores and replays to new subscribers, so Home Assistant sees current state immediately after a restart. |

## Context and constraints

Motion detection is camera-based but **does not use OpenCV**. [`rust/octocam-web/src/motion.rs`](../../rust/octocam-web/src/motion.rs)
pipes the RTSP sub-stream through ffmpeg (downscaled to 80x60 greyscale at 5fps) and applies
pixel-difference thresholding against the user's zone mask. The only motion signal available
is therefore **binary** — motion present or not, plus which zones were active. There is no
object classification, so no person/vehicle/animal distinction can be published.

Motion state already exists in memory as an `AtomicBool` in the web service, is broadcast
internally, and is exposed today at `/api/motion/events` (SSE) and in `/api/status`. This
feature adds a third consumer of that same signal.

Settings persist as JSON via [`rust/octocam-web/src/settings.rs`](../../rust/octocam-web/src/settings.rs).
That module already distinguishes stored settings from the subset returned to clients — it
strips `admin_password_hash` in `public_settings` — which establishes the precedent this
feature's broker password must follow.

Settings pages are admin-gated on both sides: client-side via
[`frontend/src/components/AdminGate.tsx`](../../frontend/src/components/AdminGate.tsx) and
server-side via `require_admin_login`.

## Assumptions

These are assumptions, not established requirements. Flag any that are wrong.

- **A1:** The broker is reachable on the local network; OctoCam does not need to traverse NAT or use a cloud relay.
- **A2:** One OctoCam device publishes to one broker. Multi-broker fan-out is out of scope.
- **A3:** Home Assistant is the consumer, so discovery follows its convention. Other MQTT consumers may subscribe to the same topics, but the schema is chosen for Home Assistant.
- **A4:** Motion zones are not published as separate entities in this iteration; only the aggregate motion state is.

## Out of scope

- Command topics or any Home Assistant → OctoCam control path.
- Camera, snapshot, or stream entities in Home Assistant.
- Diagnostic entities (CPU temperature, uptime, service states).
- Per-zone motion entities.
- Broker discovery/auto-configuration.

---

## Requirement 1: Broker connection configuration

**User Story:** As a camera owner, I want to point OctoCam at my MQTT broker, so that it can
publish to the same broker Home Assistant already uses.

#### Acceptance Criteria

1. **R1.1** THE MQTT_Settings SHALL persist a broker hostname, port, username, password, TLS-enabled flag, client identifier, base topic prefix, and discovery prefix.
2. **R1.2** THE MQTT_Settings SHALL persist an enabled flag that is disabled by default.
3. **R1.3** WHEN no discovery prefix has been configured, THE MQTT_Settings SHALL default it to `homeassistant`.
4. **R1.4** IF a submitted port is outside 1–65535, THEN THE MQTT_Settings SHALL reject the submission and leave the stored configuration unchanged.
5. **R1.5** IF MQTT is submitted as enabled with an empty hostname, THEN THE MQTT_Settings SHALL reject the submission and leave the stored configuration unchanged.
6. **R1.6** WHERE TLS is enabled, THE MQTT_Publisher SHALL connect using TLS and SHALL NOT fall back to an unencrypted connection.

## Requirement 2: Home Assistant discovery

**User Story:** As a Home Assistant user, I want OctoCam's motion sensor to appear
automatically, so that I do not have to hand-write YAML entity definitions.

#### Acceptance Criteria

1. **R2.1** WHEN the MQTT_Publisher establishes a broker connection, THE MQTT_Publisher SHALL publish a retained discovery configuration for a single binary sensor under the configured discovery prefix.
2. **R2.2** THE discovery configuration SHALL declare the binary sensor's device class as motion.
3. **R2.3** THE discovery configuration SHALL declare a unique identifier that remains stable across OctoCam restarts, so Home Assistant updates the existing entity rather than creating duplicates.
4. **R2.4** THE discovery configuration SHALL declare device information carrying OctoCam's configured device name, so the entity is grouped under one device in Home Assistant.
5. **R2.5** THE discovery configuration SHALL reference the motion state topic and the availability topic that this feature publishes.
6. **R2.6** WHEN MQTT is changed from enabled to disabled, THE MQTT_Publisher SHALL clear the retained discovery configuration, so Home Assistant removes the entity rather than leaving it permanently unavailable.
7. **R2.7** IF the configured device name changes, THEN THE MQTT_Publisher SHALL republish the discovery configuration so Home Assistant reflects the new name.

## Requirement 3: Motion state publishing

**User Story:** As a Home Assistant user, I want motion changes to reach Home Assistant
promptly, so that automations trigger when something moves.

#### Acceptance Criteria

1. **R3.1** WHEN OctoCam's motion state changes from clear to detected, THE MQTT_Publisher SHALL publish a detected payload to the motion state topic.
2. **R3.2** WHEN OctoCam's motion state changes from detected to clear, THE MQTT_Publisher SHALL publish a clear payload to the motion state topic.
3. **R3.3** THE MQTT_Publisher SHALL publish motion state as a retained message, so Home Assistant shows current state immediately after a Home Assistant restart.
4. **R3.4** WHEN the MQTT_Publisher establishes or re-establishes a broker connection, THE MQTT_Publisher SHALL publish the current motion state without waiting for the next transition.
5. **R3.5** WHILE motion detection is disabled in OctoCam settings, THE MQTT_Publisher SHALL report the motion entity as unavailable, so Home Assistant does not present a camera that is not watching as merely "clear".
6. **R3.6** WHEN a motion state change occurs while the broker connection is established, THE MQTT_Publisher SHALL publish the corresponding message within 2 seconds.

## Requirement 4: Connection lifecycle and availability

**User Story:** As a Home Assistant user, I want entities to show as unavailable when OctoCam
is offline, so that I can tell a quiet camera from an absent one.

#### Acceptance Criteria

1. **R4.1** WHEN the MQTT_Publisher connects, THE MQTT_Publisher SHALL publish an online payload to the availability topic as a retained message.
2. **R4.2** THE MQTT_Publisher SHALL register a last-will message that publishes an offline payload to the availability topic, so an ungraceful disconnect still marks OctoCam unavailable.
3. **R4.3** WHEN OctoCam shuts down cleanly, THE MQTT_Publisher SHALL publish an offline payload to the availability topic before disconnecting.
4. **R4.4** IF the broker connection fails or drops, THEN THE MQTT_Publisher SHALL retry the connection with a backoff that does not exceed one attempt per 60 seconds at steady state.
5. **R4.5** IF the broker rejects the credentials, THEN THE MQTT_Publisher SHALL record the failure reason for display and SHALL continue retrying rather than exiting.
6. **R4.6** WHILE MQTT is disabled, THE MQTT_Publisher SHALL NOT open any connection to a broker.

## Requirement 5: Configuration page

**User Story:** As an administrator, I want an MQTT settings page, so that I can configure and
check the broker connection from the OctoCam UI.

#### Acceptance Criteria

1. **R5.1** THE MQTT_Settings_Page SHALL be reachable at `/settings/mqtt`.
2. **R5.2** WHILE the signed-in user is not an administrator, THE Settings_Shell SHALL prevent the MQTT settings page from rendering, consistent with every other admin-only settings page.
3. **R5.3** IF a non-administrator requests the MQTT configuration from the API, THEN THE OctoCam_Web SHALL deny the request under its existing authorization rules.
4. **R5.4** THE MQTT_Settings_Page SHALL present the current connection state as one of connected, disconnected, or disabled.
5. **R5.5** WHEN the broker connection is failing, THE MQTT_Settings_Page SHALL display the most recent failure reason.
6. **R5.6** WHEN an administrator saves a changed configuration, THE MQTT_Publisher SHALL apply it without requiring an OctoCam restart.
7. **R5.7** THE MQTT_Settings_Page SHALL report unsaved changes through the same shell indicator used by the other settings pages.

## Requirement 6: Credential handling

**User Story:** As a camera owner, I want my broker password protected, so that configuring
MQTT does not become a way to leak it.

#### Acceptance Criteria

1. **R6.1** THE OctoCam_Web SHALL NOT include the broker password in any API response body.
2. **R6.2** WHEN an administrator loads the MQTT settings page with a password already stored, THE MQTT_Settings_Page SHALL indicate that a password is set without displaying its value.
3. **R6.3** WHEN an administrator saves the configuration without supplying a new password, THE MQTT_Settings SHALL retain the previously stored password.
4. **R6.4** WHEN an administrator explicitly clears the password, THE MQTT_Settings SHALL store an empty password.
5. **R6.5** WHILE no broker password is stored, THE MQTT_Publisher SHALL connect without credentials.
6. **R6.6** THE OctoCam_Web SHALL NOT write the broker password to application logs.

## Requirement 7: Isolation from existing functionality

**User Story:** As a camera owner, I want MQTT problems to stay contained, so that a broker
outage never costs me the camera itself.

#### Acceptance Criteria

1. **R7.1** IF the broker is unreachable, THEN THE OctoCam_Web SHALL continue serving the web UI, streams, and API unaffected.
2. **R7.2** IF the broker is unreachable, THEN THE Motion_Detector SHALL continue detecting motion and updating its existing consumers.
3. **R7.3** THE MQTT_Publisher SHALL NOT change the behavior of the existing `/api/motion/events` stream or the `motion_detected` field in `/api/status`.
4. **R7.4** THE MQTT_Publisher SHALL NOT alter HomeKit or Matter behavior.
5. **R7.5** WHILE MQTT is disabled, THE OctoCam_Web SHALL behave identically to its behavior before this feature existed.

---

## Risk notes

Recorded here so design addresses them explicitly rather than by accident.

- **Credential at rest.** The broker password is stored in OctoCam's settings JSON, which is not
  encrypted. R6 constrains its exposure over the API and in logs, but not at rest. Whether
  at-rest protection is required is an open question below.
- **Retained-message hygiene.** Retained discovery and state messages persist in the broker after
  OctoCam stops. R2.6 covers the disable path; an OctoCam that is decommissioned without being
  disabled first will leave retained topics behind.
- **Topic collisions.** Two OctoCam devices sharing a base topic prefix and client identifier would
  publish over each other. R2.3 requires a stable unique identifier, which should be derived from
  something device-specific.

## Open questions

1. **Q1:** Should the stored broker password be protected at rest, or is restricting it to admin-only API access and keeping it out of logs sufficient for a LAN device?
2. **Q2:** Should the base topic prefix default to something derived from the device name, or to a fixed literal such as `octocam`?
3. **Q3:** When motion detection is disabled, R3.5 marks the entity unavailable. Would you rather it disappear from Home Assistant entirely?
