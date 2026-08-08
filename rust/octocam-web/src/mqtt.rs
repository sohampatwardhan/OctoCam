//! MQTT publishing for Home Assistant.
//!
//! This module is the only place in the crate that knows MQTT exists. It
//! *consumes* motion state rather than owning it: the detector in
//! [`crate::motion`] remains the single source of truth, and everything here is
//! a best-effort mirror of that state onto a broker. A broker outage must never
//! be visible to the camera, the streams, or the web UI.
//!
//! Publishing is one-way. Home Assistant can observe this camera but cannot
//! control it, so there are no command topics and no inbound path into device
//! settings.

use crate::motion::{MotionHealth, MotionUpdate};
use crate::settings::{self, Settings};
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS, Transport};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

/// Fallback discovery prefix when none is configured. Home Assistant watches
/// `homeassistant` unless its MQTT integration has been told otherwise.
const DEFAULT_DISCOVERY_PREFIX: &str = "homeassistant";

/// Fallback topic root, used when the configured base topic is blank.
const DEFAULT_BASE_TOPIC: &str = "octocam";

/// What the publisher is currently doing, surfaced to the settings page.
///
/// `last_error` is retained across a reconnect attempt on purpose: an operator
/// looking at a camera stuck in a retry loop needs the reason it keeps failing,
/// not an empty field between attempts.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct MqttStatus {
    pub state: MqttConnectionState,
    pub last_error: Option<String>,
    pub connected_since: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MqttConnectionState {
    #[default]
    Disabled,
    Connecting,
    Connected,
}

/// Every topic this feature publishes to, derived once from settings.
///
/// There are two availability topics rather than one because Home Assistant
/// cannot otherwise distinguish "the camera is offline" from "the camera is
/// online but not watching for motion". Combined with `availability_mode: all`,
/// either being `offline` marks the entity unavailable — which is what stops a
/// camera with detection switched off from reporting a reassuring "clear".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Topics {
    pub discovery: String,
    pub motion_state: String,
    pub service_availability: String,
    pub detection_availability: String,
    pub unique_id: String,
    pub device_id: String,
}

fn non_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    }
}

/// Builds every topic string for this device.
///
/// Kept pure and separate from the connection loop so topic construction —
/// which is what Home Assistant integration actually depends on — is testable
/// without a broker.
pub fn topics(settings: &Settings) -> Topics {
    let prefix = non_empty(&settings.mqtt_discovery_prefix, DEFAULT_DISCOVERY_PREFIX);
    let base = non_empty(&settings.mqtt_base_topic, DEFAULT_BASE_TOPIC);
    let node = non_empty(&settings.mqtt_node_id, "unidentified");

    let device_id = format!("octocam_{node}");
    let unique_id = format!("{device_id}_motion");

    Topics {
        // Home Assistant's documented shape is
        // `<prefix>/<component>/[<node_id>/]<object_id>/config`. The optional
        // node segment is omitted and `<object_id>` is the unique id, which is
        // the convention its own documentation recommends.
        discovery: format!("{prefix}/binary_sensor/{unique_id}/config"),
        motion_state: format!("{base}/{node}/motion/state"),
        service_availability: format!("{base}/{node}/availability"),
        detection_availability: format!("{base}/{node}/motion/availability"),
        unique_id,
        device_id,
    }
}

/// Builds the retained discovery document that makes Home Assistant create the
/// motion entity by itself.
///
/// `name` is deliberately `null`: that tells Home Assistant to name the entity
/// after the device, so a rename in OctoCam propagates without the entity
/// ending up called "Front Door Front Door Motion".
pub fn discovery_payload(settings: &Settings) -> Value {
    let topics = topics(settings);
    let device_name = if settings.device_name.trim().is_empty() {
        "OctoCam"
    } else {
        settings.device_name.trim()
    };

    json!({
        "name": Value::Null,
        "device_class": "motion",
        "unique_id": topics.unique_id,
        "state_topic": topics.motion_state,
        "payload_on": "ON",
        "payload_off": "OFF",
        "availability_mode": "all",
        "availability": [
            { "topic": topics.service_availability },
            { "topic": topics.detection_availability },
        ],
        "device": {
            "identifiers": [topics.device_id],
            "name": device_name,
            "manufacturer": "OctoCam",
            "model": "Raspberry Pi Camera",
        },
    })
}


/// Payloads. Home Assistant's binary sensor defaults, stated explicitly so a
/// future change to the discovery document cannot silently desync them.
const PAYLOAD_ON: &str = "ON";
const PAYLOAD_OFF: &str = "OFF";
const PAYLOAD_ONLINE: &str = "online";
const PAYLOAD_OFFLINE: &str = "offline";

/// Reconnect backoff bounds. The ceiling exists so a broker that is simply gone
/// costs one connection attempt a minute rather than a spin loop; the floor is
/// short because a transient drop usually clears immediately.
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_CEILING: Duration = Duration::from_secs(60);

/// Doubles the retry delay up to the ceiling.
///
/// Pure so the schedule is testable without waiting out a real backoff.
fn next_backoff(current: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled > BACKOFF_CEILING {
        BACKOFF_CEILING
    } else {
        doubled
    }
}

/// Builds the client options for a broker connection.
///
/// Credentials are attached only when a username is actually configured, so an
/// anonymous broker is not sent an empty username it would have to reject. TLS,
/// once requested, is never downgraded: there is no plaintext retry path here,
/// because silently falling back would turn a misconfiguration into an
/// unnoticed plaintext credential transmission.
fn build_options(settings: &Settings, topics: &Topics) -> MqttOptions {
    let client_id = if settings.mqtt_client_id.trim().is_empty() {
        topics.device_id.clone()
    } else {
        settings.mqtt_client_id.trim().to_string()
    };

    let mut options = MqttOptions::new(
        client_id,
        settings.mqtt_host.trim().to_string(),
        settings.mqtt_port.clamp(1, 65535) as u16,
    );
    options.set_keep_alive(Duration::from_secs(30));

    if !settings.mqtt_username.trim().is_empty() {
        options.set_credentials(
            settings.mqtt_username.trim().to_string(),
            settings.mqtt_password.expose().to_string(),
        );
    }

    if settings.mqtt_tls {
        options.set_transport(Transport::tls_with_default_config());
    }

    // Registered before connecting so the broker owns the promise: if this
    // process dies without a clean disconnect, the camera still goes offline.
    options.set_last_will(LastWill::new(
        topics.service_availability.clone(),
        PAYLOAD_OFFLINE,
        QoS::AtLeastOnce,
        true,
    ));

    options
}

/// Runs the MQTT publisher for the lifetime of the process.
///
/// The task never exits and never surfaces an error to its caller. A broker
/// outage is not an OctoCam outage: the camera, streams, motion detection and
/// web UI must all behave identically whether or not this succeeds, so every
/// failure here is recorded in `status` and retried rather than propagated.
///
/// Motion state is *read*, never owned — [`crate::motion`] remains the source
/// of truth and this only mirrors it onto a broker.
pub fn spawn_mqtt_publisher(
    config_path: PathBuf,
    motion_detected: Arc<AtomicBool>,
    motion_health: Arc<MotionHealth>,
    mut motion_rx: broadcast::Receiver<MotionUpdate>,
    mut reload_rx: broadcast::Receiver<()>,
    status: Arc<Mutex<MqttStatus>>,
) {
    tokio::spawn(async move {
        let mut backoff = BACKOFF_START;
        // Remembers what was last advertised so disabling MQTT can withdraw the
        // retained discovery document it published earlier.
        let mut announced: Option<Topics> = None;

        loop {
            let settings = settings::load_settings(&config_path);

            if !settings.mqtt_enabled {
                if let Some(previous) = announced.take() {
                    withdraw(&settings, &previous).await;
                }
                set_status(&status, MqttConnectionState::Disabled, None);
                // Nothing to poll while disabled, so wait for a settings change
                // rather than reconnect-looping against a broker we must not
                // contact at all.
                let _ = reload_rx.recv().await;
                backoff = BACKOFF_START;
                continue;
            }

            let topics = topics(&settings);
            set_status(&status, MqttConnectionState::Connecting, None);

            let (client, mut eventloop) = AsyncClient::new(build_options(&settings, &topics), 32);
            let mut connected = false;

            loop {
                tokio::select! {
                    event = eventloop.poll() => match event {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            connected = true;
                            backoff = BACKOFF_START;
                            announced = Some(topics.clone());
                            on_connect(&client, &settings, &topics, &motion_detected, &motion_health).await;
                            set_status(&status, MqttConnectionState::Connected, None);
                        }
                        Ok(_) => {}
                        Err(error) => {
                            // Covers refused credentials as well as an absent
                            // broker; both are retried, never fatal.
                            set_status(
                                &status,
                                MqttConnectionState::Connecting,
                                Some(error.to_string()),
                            );
                            break;
                        }
                    },
                    motion = motion_rx.recv() => match motion {
                        Ok(update) if connected => {
                            publish(
                                &client,
                                &topics.motion_state,
                                payload_for(update.motion_detected),
                                true,
                            )
                            .await;
                            // Availability rides the same update, so a detector
                            // that goes blind is reflected without waiting for
                            // the next reconnect.
                            publish(
                                &client,
                                &topics.detection_availability,
                                availability_for(update.motion_available),
                                true,
                            )
                            .await;
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // Missed transitions are not worth replaying — only
                            // the current state matters to Home Assistant.
                            if connected {
                                let now = motion_detected.load(Ordering::Relaxed);
                                publish(&client, &topics.motion_state, payload_for(now), true).await;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    _ = reload_rx.recv() => {
                        // Settings changed: drop this connection and rebuild
                        // from the new configuration on the next pass.
                        let _ = client.disconnect().await;
                        break;
                    }
                }
            }

            if !settings.mqtt_enabled {
                continue;
            }
            tokio::time::sleep(backoff).await;
            backoff = next_backoff(backoff);
        }
    });
}

fn availability_for(available: bool) -> &'static str {
    if available {
        PAYLOAD_ONLINE
    } else {
        PAYLOAD_OFFLINE
    }
}

fn payload_for(detected: bool) -> &'static str {
    if detected {
        PAYLOAD_ON
    } else {
        PAYLOAD_OFF
    }
}

async fn publish(client: &AsyncClient, topic: &str, payload: &str, retain: bool) {
    if let Err(error) = client
        .publish(topic, QoS::AtLeastOnce, retain, payload.as_bytes().to_vec())
        .await
    {
        tracing::warn!("mqtt publish to {topic} failed: {error}");
    }
}

/// Everything that must be true the moment a connection is established.
///
/// Publishing current motion state here rather than waiting for the next
/// transition is what stops Home Assistant showing a stale or empty sensor
/// after either side restarts.
async fn on_connect(
    client: &AsyncClient,
    settings: &Settings,
    topics: &Topics,
    motion_detected: &Arc<AtomicBool>,
    motion_health: &Arc<MotionHealth>,
) {
    publish(client, &topics.service_availability, PAYLOAD_ONLINE, true).await;

    let discovery = discovery_payload(settings).to_string();
    if let Err(error) = client
        .publish(&topics.discovery, QoS::AtLeastOnce, true, discovery.into_bytes())
        .await
    {
        tracing::warn!("mqtt discovery publish failed: {error}");
    }

    // Detector *liveness*, not merely the settings flag. A detector that is
    // enabled but wedged reports unavailable, so Home Assistant never shows a
    // confident "clear" from a camera that cannot currently see anything.
    publish(
        client,
        &topics.detection_availability,
        availability_for(motion_health.snapshot().available),
        true,
    )
    .await;

    let detected = motion_detected.load(Ordering::Relaxed);
    publish(client, &topics.motion_state, payload_for(detected), true).await;
}

/// Withdraws this device from Home Assistant when MQTT is switched off.
///
/// An empty retained payload on the discovery topic is how Home Assistant is
/// told to forget an entity. Without it, disabling MQTT would leave a
/// permanently "unavailable" ghost entity behind forever.
async fn withdraw(settings: &Settings, topics: &Topics) {
    let (client, mut eventloop) = AsyncClient::new(build_options(settings, topics), 8);
    let _ = client
        .publish(&topics.discovery, QoS::AtLeastOnce, true, Vec::new())
        .await;
    let _ = client
        .publish(
            &topics.service_availability,
            QoS::AtLeastOnce,
            true,
            PAYLOAD_OFFLINE.as_bytes().to_vec(),
        )
        .await;
    // Pump briefly so the packets actually leave before the client drops.
    for _ in 0..8 {
        if tokio::time::timeout(Duration::from_millis(250), eventloop.poll())
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = client.disconnect().await;
}

fn set_status(
    status: &Arc<Mutex<MqttStatus>>,
    state: MqttConnectionState,
    error: Option<String>,
) {
    if let Ok(mut guard) = status.lock() {
        guard.state = state;
        if error.is_some() {
            guard.last_error = error;
        }
        guard.connected_since = match state {
            MqttConnectionState::Connected => guard.connected_since.or_else(now_epoch),
            _ => None,
        };
    }
}

fn now_epoch() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured() -> Settings {
        let mut settings = Settings::default();
        settings.mqtt_node_id = "a1b2c3d4".to_string();
        settings.device_name = "Front Door".to_string();
        settings
    }

    #[test]
    fn backoff_doubles_then_holds_at_the_ceiling() {
        let mut delay = BACKOFF_START;
        let mut seen = vec![delay];
        for _ in 0..12 {
            delay = next_backoff(delay);
            seen.push(delay);
        }
        assert_eq!(seen[0], Duration::from_secs(1));
        assert_eq!(seen[1], Duration::from_secs(2));
        assert_eq!(seen[2], Duration::from_secs(4));
        assert!(
            seen.iter().all(|d| *d <= BACKOFF_CEILING),
            "retry delay must never exceed one attempt per 60s (R4.4)"
        );
        assert_eq!(
            *seen.last().expect("non-empty"),
            BACKOFF_CEILING,
            "backoff must settle at the ceiling rather than growing without bound"
        );
    }

    #[test]
    fn no_credentials_are_sent_when_no_username_is_configured() {
        let mut settings = configured();
        settings.mqtt_host = "broker.local".to_string();
        let options = build_options(&settings, &topics(&settings));
        assert!(
            options.credentials().is_none(),
            "an anonymous broker must not be sent an empty username (R6.5)"
        );

        settings.mqtt_username = "octocam".to_string();
        settings.mqtt_password = crate::settings::Secret::from("s3cret");
        let options = build_options(&settings, &topics(&settings));
        assert!(options.credentials().is_some());
    }

    #[test]
    fn enabling_tls_selects_a_tls_transport_with_no_plaintext_variant() {
        let mut settings = configured();
        settings.mqtt_host = "broker.local".to_string();

        let plain = build_options(&settings, &topics(&settings));
        assert!(matches!(plain.transport(), Transport::Tcp), "default is plaintext");

        settings.mqtt_tls = true;
        let secured = build_options(&settings, &topics(&settings));
        assert!(
            !matches!(secured.transport(), Transport::Tcp),
            "TLS must never resolve to a plaintext transport (R1.6)"
        );
    }

    #[test]
    fn the_client_id_falls_back_to_device_identity_but_respects_an_override() {
        let mut settings = configured();
        settings.mqtt_host = "broker.local".to_string();
        let options = build_options(&settings, &topics(&settings));
        assert_eq!(options.client_id(), "octocam_a1b2c3d4");

        settings.mqtt_client_id = "custom-id".to_string();
        let options = build_options(&settings, &topics(&settings));
        assert_eq!(options.client_id(), "custom-id");
    }

    #[test]
    fn discovery_topic_follows_home_assistant_shape_with_the_default_prefix() {
        let settings = configured();
        assert_eq!(settings.mqtt_discovery_prefix, "homeassistant", "R1.3");
        assert_eq!(
            topics(&settings).discovery,
            "homeassistant/binary_sensor/octocam_a1b2c3d4_motion/config"
        );
    }

    #[test]
    fn a_blank_prefix_or_base_topic_falls_back_rather_than_producing_a_broken_topic() {
        let mut settings = configured();
        settings.mqtt_discovery_prefix = "   ".to_string();
        settings.mqtt_base_topic = String::new();
        let topics = topics(&settings);
        assert!(topics.discovery.starts_with("homeassistant/"), "R1.3");
        assert!(topics.motion_state.starts_with("octocam/"));
    }

    #[test]
    fn every_topic_is_derived_from_the_base_topic_and_node_id() {
        let topics = topics(&configured());
        assert_eq!(topics.motion_state, "octocam/a1b2c3d4/motion/state");
        assert_eq!(topics.service_availability, "octocam/a1b2c3d4/availability");
        assert_eq!(
            topics.detection_availability,
            "octocam/a1b2c3d4/motion/availability"
        );
    }

    #[test]
    fn discovery_payload_declares_a_motion_binary_sensor(){
        let payload = discovery_payload(&configured());
        assert_eq!(payload["device_class"], "motion", "R2.2");
        assert!(payload["name"].is_null(), "entity name derives from the device");
        assert_eq!(payload["state_topic"], "octocam/a1b2c3d4/motion/state", "R2.5");
    }

    #[test]
    fn discovery_payload_carries_the_device_name_and_groups_under_one_device() {
        let payload = discovery_payload(&configured());
        assert_eq!(payload["device"]["name"], "Front Door", "R2.4");
        assert_eq!(payload["device"]["identifiers"][0], "octocam_a1b2c3d4", "R2.4");
    }

    #[test]
    fn unique_id_survives_a_device_rename() {
        let settings = configured();
        let before = topics(&settings).unique_id;

        let mut renamed = settings.clone();
        renamed.device_name = "Back Garden".to_string();
        assert_eq!(topics(&renamed).unique_id, before, "R2.3");

        // The payload must follow the rename even though identity does not.
        assert_eq!(discovery_payload(&renamed)["device"]["name"], "Back Garden");
    }

    #[test]
    fn both_availability_topics_are_declared_so_detection_off_is_distinguishable() {
        let payload = discovery_payload(&configured());
        let availability = payload["availability"].as_array().expect("array");

        // `all` is what makes either topic going offline mark the entity
        // unavailable; with `any` a disabled detector would still look "clear".
        assert_eq!(payload["availability_mode"], "all", "R3.5");
        assert_eq!(availability.len(), 2, "R2.5, R3.5");
        assert_eq!(availability[0]["topic"], "octocam/a1b2c3d4/availability");
        assert_eq!(
            availability[1]["topic"],
            "octocam/a1b2c3d4/motion/availability"
        );
    }
    // ---- Broker integration tests -------------------------------------------
    // Ignored by default: they need the local Mosquitto container on
    // 127.0.0.1:18830. Run with:
    //   cargo test mqtt::tests::broker -- --ignored --test-threads=1
    // These cover the criteria that cannot exist in-process: retained discovery
    // and current state on connect, the birth message, and a live transition
    // within the 2s bound. The last will (R4.2) needs an ungraceful kill and is
    // verified by the checkpoint script rather than here.

    use crate::motion::{MotionHealth, MotionUpdate};
    use rumqttc::{AsyncClient, Event, MqttOptions, Packet};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;
    use tokio::sync::broadcast;

    const BROKER_HOST: &str = "127.0.0.1";
    const BROKER_PORT: u16 = 18830;

    fn broker_config(dir: &std::path::Path, node: &str) -> PathBuf {
        let mut cfg = Settings::default();
        cfg.mqtt_enabled = true;
        cfg.mqtt_host = BROKER_HOST.to_string();
        cfg.mqtt_port = BROKER_PORT as i32;
        cfg.mqtt_node_id = node.to_string();
        cfg.device_name = "Test Cam".to_string();
        cfg.motion_enabled = true;
        let path = dir.join("settings.json");
        settings::save_settings(&path, &cfg).expect("write settings");
        path
    }

    async fn drain(id: &str, filter: &str, window: Duration) -> Vec<(String, Vec<u8>)> {
        let mut opts = MqttOptions::new(id, BROKER_HOST, BROKER_PORT);
        opts.set_keep_alive(Duration::from_secs(5));
        let (client, mut eventloop) = AsyncClient::new(opts, 32);
        client.subscribe(filter, QoS::AtLeastOnce).await.unwrap();
        let mut out = Vec::new();
        let deadline = tokio::time::Instant::now() + window;
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(300), eventloop.poll()).await {
                Ok(Ok(Event::Incoming(Packet::Publish(p)))) => out.push((p.topic, p.payload.to_vec())),
                Ok(Ok(_)) => {}
                Ok(Err(_)) => break,
                Err(_) => {}
            }
        }
        out
    }

    #[tokio::test]
    #[ignore]
    async fn broker_publishes_retained_discovery_and_state_on_connect() {
        let tmp = tempfile::tempdir().unwrap();
        let node = "conn01";
        let config = broker_config(tmp.path(), node);
        let (motion_tx, _) = broadcast::channel(16);
        let (_reload_tx, reload_rx) = broadcast::channel(4);
        spawn_mqtt_publisher(
            config,
            Arc::new(AtomicBool::new(false)),
            Arc::new(MotionHealth::default()),
            motion_tx.subscribe(),
            reload_rx,
            Arc::new(Mutex::new(MqttStatus::default())),
        );
        tokio::time::sleep(Duration::from_secs(2)).await;

        let disc = drain("assert-conn", "homeassistant/#", Duration::from_secs(2)).await;
        let found = disc.iter().find(|(t, _)| t.contains(node)).expect("retained discovery on connect (R2.1)");
        let json: Value = serde_json::from_slice(&found.1).unwrap();
        assert_eq!(json["device_class"], "motion", "R2.2");

        let state = drain("assert-state", &format!("octocam/{node}/#"), Duration::from_secs(2)).await;
        assert!(
            state.iter().any(|(t, p)| t.ends_with("/motion/state") && p == b"OFF"),
            "current state on connect without a transition (R3.4)"
        );
        assert!(
            state.iter().any(|(t, p)| t.ends_with("/availability") && p == b"online"),
            "service availability online on connect (R4.1)"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn broker_publishes_transitions_within_the_latency_bound() {
        let tmp = tempfile::tempdir().unwrap();
        let node = "trans1";
        let config = broker_config(tmp.path(), node);
        let (motion_tx, _) = broadcast::channel(16);
        let (_reload_tx, reload_rx) = broadcast::channel(4);
        spawn_mqtt_publisher(
            config,
            Arc::new(AtomicBool::new(false)),
            Arc::new(MotionHealth::default()),
            motion_tx.subscribe(),
            reload_rx,
            Arc::new(Mutex::new(MqttStatus::default())),
        );
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Subscribe, then send a transition and stop as soon as ON arrives, so
        // the measured elapsed is genuine publish latency, not the drain window.
        let mut opts = MqttOptions::new("assert-trans", BROKER_HOST, BROKER_PORT);
        opts.set_keep_alive(Duration::from_secs(5));
        let (sub, mut eventloop) = AsyncClient::new(opts, 32);
        sub.subscribe(format!("octocam/{node}/motion/state"), QoS::AtLeastOnce)
            .await
            .unwrap();
        // Drain the retained OFF that connect published, so we time the ON only.
        let settle = tokio::time::Instant::now() + Duration::from_secs(1);
        while tokio::time::Instant::now() < settle {
            let _ = tokio::time::timeout(Duration::from_millis(200), eventloop.poll()).await;
        }

        let sent = tokio::time::Instant::now();
        motion_tx
            .send(MotionUpdate { motion_detected: true, motion_available: true })
            .unwrap();

        let mut latency = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Ok(Event::Incoming(Packet::Publish(p)))) =
                tokio::time::timeout(Duration::from_millis(200), eventloop.poll()).await
            {
                if p.payload.as_ref() == b"ON" {
                    latency = Some(sent.elapsed());
                    break;
                }
            }
        }
        let latency = latency.expect("detected transition publishes ON (R3.1)");
        assert!(latency < Duration::from_secs(2), "within the 2s bound (R3.6): {latency:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn broker_fires_the_last_will_on_an_ungraceful_drop() {
        // Watch the availability topic first.
        let node = "will01";
        let mut sub_opts = MqttOptions::new("assert-will", BROKER_HOST, BROKER_PORT);
        sub_opts.set_keep_alive(Duration::from_secs(5));
        let (sub, mut sub_loop) = AsyncClient::new(sub_opts, 32);
        sub.subscribe(format!("octocam/{node}/availability"), QoS::AtLeastOnce)
            .await
            .unwrap();

        // Connect a client built exactly as the publisher builds it — same will
        // registration — then drop it WITHOUT disconnect(), so the broker sees
        // an ungraceful close and must publish the will (R4.2).
        let mut settings = Settings::default();
        settings.mqtt_node_id = node.to_string();
        settings.mqtt_host = BROKER_HOST.to_string();
        settings.mqtt_port = BROKER_PORT as i32;
        let topics = topics(&settings);
        {
            let (client, mut eventloop) = AsyncClient::new(build_options(&settings, &topics), 8);
            client
                .publish(&topics.service_availability, QoS::AtLeastOnce, true, b"online".to_vec())
                .await
                .unwrap();
            // Pump until connected and the online publish is acknowledged.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
            while tokio::time::Instant::now() < deadline {
                let _ = tokio::time::timeout(Duration::from_millis(200), eventloop.poll()).await;
            }
            // client + eventloop dropped here with no disconnect() → RST → will.
        }

        let mut saw_offline = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Ok(Event::Incoming(Packet::Publish(p)))) =
                tokio::time::timeout(Duration::from_millis(300), sub_loop.poll()).await
            {
                if p.payload.as_ref() == b"offline" {
                    saw_offline = true;
                    break;
                }
            }
        }
        assert!(saw_offline, "broker must publish the last will offline on ungraceful drop (R4.2)");
    }
}
