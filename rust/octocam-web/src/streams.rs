use crate::settings::Settings;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const API_PORT: u16 = 9997;
const API_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PathViewers {
    pub browser: u32,
    pub rtsp: u32,
    pub homekit: u32,
    pub matter: u32,
    pub hls: u32,
    pub total: u32,
    /// User-facing cap (excludes the local daemon reserve slots — HomeKit/Matter).
    pub capacity: u32,
    pub clients: Vec<ClientView>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct ClientView {
    pub label: String,
    pub client_type: String,
    pub remote_addr: String,
    pub user_agent: String,
    pub connected_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ViewerReport {
    pub main: PathViewers,
    pub sub: PathViewers,
}

impl ViewerReport {
    /// Main has room for another NON-local viewer. The local daemon buckets
    /// (HomeKit/Matter) and lingering HLS sessions are deliberately excluded from
    /// capacity math — mis-attribution between daemons can never mark a path full.
    #[cfg_attr(not(test), allow(dead_code))] // exercised by unit tests; capacity gating is client-side
    pub fn main_available(&self) -> bool {
        self.main.browser + self.main.rtsp < self.main.capacity
    }
}

/// Query the local mediamtx API and classify every reader. None on any failure.
pub async fn viewer_report(settings: &Settings) -> Option<ViewerReport> {
    let paths = http_get_local("/v3/paths/list").await?;
    let sessions = http_get_local("/v3/rtspsessions/list").await?;
    let (webrtc_sessions, hls_sessions) = tokio::join!(
        http_get_local("/v3/webrtcsessions/list"),
        http_get_local("/v3/hlssessions/list"),
    );
    let webrtc_sessions = webrtc_sessions.unwrap_or_else(|| "{\"items\":[]}".to_string());
    let hls_sessions = hls_sessions.unwrap_or_else(|| "{\"items\":[]}".to_string());
    classify(
        &paths,
        &sessions,
        &webrtc_sessions,
        &hls_sessions,
        &settings.rtsp_path,
        &settings.sub_rtsp_path,
        settings.rtsp_max_clients.max(0) as u32,
        settings.sub_rtsp_max_clients.max(0) as u32,
        settings.homekit_enabled,
        settings.matter_enabled,
    )
}

/// (test seam: the flat argument list mirrors the mediamtx API inputs plus
/// the two daemon flags; a params struct would only obscure the call sites)
#[allow(clippy::too_many_arguments)]
fn classify(
    paths_json: &str,
    sessions_json: &str,
    webrtc_sessions_json: &str,
    hls_sessions_json: &str,
    main_path: &str,
    sub_path: &str,
    main_cap: u32,
    sub_cap: u32,
    homekit_enabled: bool,
    matter_enabled: bool,
) -> Option<ViewerReport> {
    let paths: Value = serde_json::from_str(paths_json).ok()?;
    let sessions: Value = serde_json::from_str(sessions_json).unwrap_or(Value::Null);
    let rtsp_meta = session_meta_map(sessions_json);
    let webrtc_meta = session_meta_map(webrtc_sessions_json);
    let hls_meta = session_meta_map(hls_sessions_json);

    // rtsp session id -> is the reader local (HomeKit's ffmpeg)? Lenient by design:
    // a degraded sessions response must not wipe the whole report — browser/HLS
    // counts don't depend on it, and unmatched rtsp readers fall back to non-local.
    let mut local_session = HashMap::new();
    if let Some(items) = sessions.get("items").and_then(Value::as_array) {
        for item in items {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            let remote = item.get("remoteAddr").and_then(Value::as_str).unwrap_or("");
            local_session.insert(id.to_string(), remote.starts_with("127.0.0.1"));
        }
    }

    let mut report = ViewerReport {
        main: PathViewers {
            capacity: main_cap,
            ..Default::default()
        },
        sub: PathViewers {
            capacity: sub_cap,
            ..Default::default()
        },
    };
    let mut local_main = 0u32;
    let mut local_sub = 0u32;
    for item in paths.get("items")?.as_array()? {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        let (target, local_count) = if name == main_path {
            (&mut report.main, &mut local_main)
        } else if name == sub_path {
            (&mut report.sub, &mut local_sub)
        } else {
            continue;
        };
        let Some(readers) = item.get("readers").and_then(Value::as_array) else {
            continue;
        };
        for reader in readers {
            let kind = reader.get("type").and_then(Value::as_str).unwrap_or("");
            let id = reader.get("id").and_then(Value::as_str).unwrap_or("");
            match kind {
                // Exact casing verified against mediamtx v1.19.2 source/OpenAPI:
                // webRTCSession (capital RTC) and hlsSession — NOT webrtcSession/hlsMuxer.
                "webRTCSession" => {
                    target.browser += 1;
                    target.clients.push(client_view(
                        "Browser preview",
                        "WebRTC",
                        webrtc_meta.get(id),
                    ));
                }
                // HLS sessions linger after the last client leaves; count them in the
                // displayed total but NOT in capacity math, to avoid false "main full".
                "hlsSession" => {
                    target.hls += 1;
                    target
                        .clients
                        .push(client_view("HLS session", "HLS", hls_meta.get(id)));
                }
                "rtspSession" | "rtspsSession" => {
                    if local_session.get(id).copied().unwrap_or(false) {
                        // Local daemon readers; attributed between homekit/matter below.
                        *local_count += 1;
                        target.clients.push(client_view(
                            local_daemon_label(homekit_enabled, matter_enabled),
                            "Local RTSP",
                            rtsp_meta.get(id),
                        ));
                    } else {
                        target.rtsp += 1;
                        target
                            .clients
                            .push(client_view("RTSP client", "RTSP", rtsp_meta.get(id)));
                    }
                }
                _ => {
                    target.rtsp += 1;
                    target
                        .clients
                        .push(client_view("Stream reader", kind, None));
                }
            }
            target.total += 1;
        }
    }
    let (hk, mt) = attribute_local(local_main, homekit_enabled, matter_enabled);
    report.main.homekit = hk;
    report.main.matter = mt;
    let (hk, mt) = attribute_local(local_sub, homekit_enabled, matter_enabled);
    report.sub.homekit = hk;
    report.sub.matter = mt;
    Some(report)
}

#[derive(Clone, Debug, Default)]
struct SessionMeta {
    remote_addr: String,
    user_agent: String,
    connected_at: String,
}

fn session_meta_map(json: &str) -> HashMap<String, SessionMeta> {
    let mut sessions = HashMap::new();
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return sessions;
    };
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return sessions;
    };
    for item in items {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        sessions.insert(
            id.to_string(),
            SessionMeta {
                remote_addr: item
                    .get("remoteAddr")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                user_agent: item
                    .get("userAgent")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                connected_at: item
                    .get("created")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            },
        );
    }
    sessions
}

fn client_view(label: &str, client_type: &str, meta: Option<&SessionMeta>) -> ClientView {
    let meta = meta.cloned().unwrap_or_default();
    ClientView {
        label: label.to_string(),
        client_type: client_type.to_string(),
        remote_addr: fallback(meta.remote_addr, "Not available"),
        user_agent: fallback(meta.user_agent, "Not available"),
        connected_at: fallback(meta.connected_at, "Not available"),
    }
}

fn local_daemon_label(homekit_enabled: bool, matter_enabled: bool) -> &'static str {
    match (homekit_enabled, matter_enabled) {
        (true, true) => "Local daemon (HomeKit/Matter)",
        (true, false) => "HomeKit",
        (false, true) => "Matter",
        (false, false) => "Local RTSP",
    }
}

fn fallback(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

/// mediamtx's session list exposes only remoteAddr, so two loopback daemons are
/// indistinguishable at the protocol level. Each daemon holds at most one
/// persistent reader per path; the transient snapshot ffmpeg also shows as
/// local. Attribution: single-daemon setups get everything; with both enabled,
/// matter is credited one reader once a second local reader exists, and any
/// extras (snapshot capture) ride the homekit bucket.
fn attribute_local(local: u32, homekit_enabled: bool, matter_enabled: bool) -> (u32, u32) {
    if !matter_enabled {
        return (local, 0);
    }
    if !homekit_enabled {
        return (0, local);
    }
    let matter = u32::from(local >= 2);
    (local - matter, matter)
}

/// Minimal HTTP/1.0 GET to the localhost mediamtx API. HTTP/1.0 forces
/// connection-close and forbids chunked bodies, so "read to EOF, split on
/// the blank line" is a complete client. Returns the body.
async fn http_get_local(path: &str) -> Option<String> {
    tokio::time::timeout(API_TIMEOUT, async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", API_PORT))
            .await
            .ok()?;
        let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.ok()?;
        let text = String::from_utf8(raw).ok()?;
        let (head, body) = text.split_once("\r\n\r\n")?;
        if !head.starts_with("HTTP/1.0 200") && !head.starts_with("HTTP/1.1 200") {
            return None;
        }
        Some(body.to_string())
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATHS: &str = r#"{"itemCount":2,"pageCount":1,"items":[
        {"name":"main","readers":[{"type":"webRTCSession","id":"w1"},{"type":"rtspSession","id":"r1"}]},
        {"name":"sub","readers":[{"type":"rtspSession","id":"r2"},{"type":"hlsSession","id":"h1"}]}
    ]}"#;
    const SESSIONS: &str = r#"{"itemCount":2,"pageCount":1,"items":[
        {"id":"r1","remoteAddr":"192.168.2.50:61044"},
        {"id":"r2","remoteAddr":"127.0.0.1:44064"}
    ]}"#;
    const EMPTY_SESSIONS: &str = r#"{"items":[]}"#;

    #[allow(clippy::too_many_arguments)]
    fn classify_test(
        paths: &str,
        sessions: &str,
        main_path: &str,
        sub_path: &str,
        main_cap: u32,
        sub_cap: u32,
        homekit_enabled: bool,
        matter_enabled: bool,
    ) -> Option<ViewerReport> {
        classify(
            paths,
            sessions,
            EMPTY_SESSIONS,
            EMPTY_SESSIONS,
            main_path,
            sub_path,
            main_cap,
            sub_cap,
            homekit_enabled,
            matter_enabled,
        )
    }

    #[test]
    fn classifies_readers_by_type_and_locality() {
        let report = classify_test(PATHS, SESSIONS, "main", "sub", 1, 2, true, false).unwrap();
        assert_eq!(report.main.browser, 1);
        assert_eq!(report.main.rtsp, 1);
        assert_eq!(report.main.total, 2);
        assert_eq!(report.main.capacity, 1);
        assert_eq!(report.main.clients[0].label, "Browser preview");
        assert_eq!(report.main.clients[1].remote_addr, "192.168.2.50:61044");
        assert_eq!(report.sub.homekit, 1);
        assert_eq!(report.sub.hls, 1);
        assert_eq!(report.sub.total, 2);
        assert_eq!(report.sub.capacity, 2);
    }

    #[test]
    fn lingering_hls_does_not_consume_capacity() {
        let paths = r#"{"items":[{"name":"main","readers":[{"type":"hlsSession","id":"h9"}]},{"name":"sub","readers":[]}]}"#;
        let report = classify_test(paths, SESSIONS, "main", "sub", 1, 2, true, false).unwrap();
        assert_eq!(report.main.hls, 1);
        assert!(
            report.main_available(),
            "a lingering HLS session must not mark main full"
        );
    }

    #[test]
    fn main_availability_ignores_homekit() {
        let paths = r#"{"items":[{"name":"main","readers":[{"type":"rtspSession","id":"r2"}]},{"name":"sub","readers":[]}]}"#;
        let report = classify_test(paths, SESSIONS, "main", "sub", 1, 2, true, false).unwrap();
        assert_eq!(report.main.homekit, 1);
        assert!(
            report.main_available(),
            "a HomeKit reader must not consume user capacity"
        );
    }

    #[test]
    fn malformed_paths_yields_none_but_sessions_degrade() {
        assert!(classify_test("not json", SESSIONS, "main", "sub", 1, 2, true, false).is_none());
        assert!(classify_test(PATHS, "{}", "main", "sub", 1, 2, true, false).is_some());
    }

    #[test]
    fn sessions_endpoint_down_still_reports_non_rtsp_counts() {
        let report = classify_test(PATHS, "{}", "main", "sub", 1, 2, true, false).unwrap();
        assert_eq!(report.main.browser, 1);
        assert_eq!(report.sub.hls, 1);
        // Without locality info the sub rtsp reader counts as external, not homekit.
        assert_eq!(report.sub.rtsp, 1);
        assert_eq!(report.sub.homekit, 0);
    }

    #[test]
    fn local_reader_attributed_to_matter_when_only_matter_enabled() {
        let paths = r#"{"items":[{"name":"main","readers":[{"type":"rtspSession","id":"r2"}]},{"name":"sub","readers":[]}]}"#;
        let report = classify_test(paths, SESSIONS, "main", "sub", 1, 2, false, true).unwrap();
        assert_eq!(report.main.matter, 1);
        assert_eq!(report.main.homekit, 0);
        assert!(
            report.main_available(),
            "a Matter reader must not consume user capacity"
        );
    }

    #[test]
    fn two_local_readers_split_between_daemons_when_both_enabled() {
        let paths = r#"{"items":[{"name":"sub","readers":[{"type":"rtspSession","id":"r2"},{"type":"rtspSession","id":"r3"}]},{"name":"main","readers":[]}]}"#;
        let sessions = r#"{"items":[
            {"id":"r2","remoteAddr":"127.0.0.1:44064"},
            {"id":"r3","remoteAddr":"127.0.0.1:44100"}
        ]}"#;
        let report = classify_test(paths, sessions, "main", "sub", 1, 2, true, true).unwrap();
        assert_eq!(report.sub.homekit, 1);
        assert_eq!(report.sub.matter, 1);
    }

    #[test]
    fn adds_webrtc_client_metadata_when_available() {
        let paths = r#"{"items":[{"name":"sub","readers":[{"type":"webRTCSession","id":"w1"}]},{"name":"main","readers":[]}]}"#;
        let webrtc = r#"{"items":[{"id":"w1","remoteAddr":"192.168.2.154:52493","userAgent":"Chrome","created":"2026-07-05T17:55:01Z"}]}"#;
        let report = classify(
            paths,
            SESSIONS,
            webrtc,
            EMPTY_SESSIONS,
            "main",
            "sub",
            1,
            2,
            true,
            true,
        )
        .unwrap();
        assert_eq!(report.sub.browser, 1);
        assert_eq!(report.sub.clients[0].label, "Browser preview");
        assert_eq!(report.sub.clients[0].client_type, "WebRTC");
        assert_eq!(report.sub.clients[0].remote_addr, "192.168.2.154:52493");
        assert_eq!(report.sub.clients[0].user_agent, "Chrome");
    }

    #[test]
    fn attribute_local_rules() {
        assert_eq!(attribute_local(3, true, false), (3, 0));
        assert_eq!(attribute_local(3, false, true), (0, 3));
        assert_eq!(attribute_local(1, true, true), (1, 0)); // ambiguous single: homekit, stable
        assert_eq!(attribute_local(2, true, true), (1, 1));
        assert_eq!(attribute_local(3, true, true), (2, 1)); // transient snapshot ffmpeg rides homekit bucket
        assert_eq!(attribute_local(2, false, false), (2, 0)); // legacy fallback
    }
}
