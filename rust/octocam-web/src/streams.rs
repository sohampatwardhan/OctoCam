use crate::settings::Settings;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const API_PORT: u16 = 9997;
const API_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PathViewers {
    pub browser: u32,
    pub rtsp: u32,
    pub homekit: u32,
    pub hls: u32,
    pub total: u32,
    /// User-facing cap (excludes the HomeKit reserve slot).
    pub capacity: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ViewerReport {
    pub main: PathViewers,
    pub sub: PathViewers,
}

impl ViewerReport {
    /// Main has room for another NON-local viewer. HomeKit's reserve and lingering
    /// HLS sessions are deliberately excluded from capacity math.
    #[allow(dead_code)] // TODO(task 5): consumed by /api/status wiring
    pub fn main_available(&self) -> bool {
        self.main.browser + self.main.rtsp < self.main.capacity
    }
}

/// Query the local mediamtx API and classify every reader. None on any failure.
#[allow(dead_code)] // TODO(task 5): consumed by /api/status wiring
pub async fn viewer_report(settings: &Settings) -> Option<ViewerReport> {
    let paths = http_get_local("/v3/paths/list").await?;
    let sessions = http_get_local("/v3/rtspsessions/list").await?;
    classify(
        &paths,
        &sessions,
        &settings.rtsp_path,
        &settings.sub_rtsp_path,
        settings.rtsp_max_clients.max(0) as u32,
        settings.sub_rtsp_max_clients.max(0) as u32,
    )
}

fn classify(
    paths_json: &str,
    sessions_json: &str,
    main_path: &str,
    sub_path: &str,
    main_cap: u32,
    sub_cap: u32,
) -> Option<ViewerReport> {
    let paths: Value = serde_json::from_str(paths_json).ok()?;
    let sessions: Value = serde_json::from_str(sessions_json).ok()?;

    // rtsp session id -> is the reader local (HomeKit's ffmpeg)?
    let mut local_session = std::collections::HashMap::new();
    for item in sessions.get("items")?.as_array()? {
        let id = item.get("id")?.as_str()?.to_string();
        let remote = item.get("remoteAddr").and_then(Value::as_str).unwrap_or("");
        local_session.insert(id, remote.starts_with("127.0.0.1"));
    }

    let mut report = ViewerReport {
        main: PathViewers { capacity: main_cap, ..Default::default() },
        sub: PathViewers { capacity: sub_cap, ..Default::default() },
    };
    for item in paths.get("items")?.as_array()? {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        let target = if name == main_path {
            &mut report.main
        } else if name == sub_path {
            &mut report.sub
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
                "webRTCSession" => target.browser += 1,
                // HLS sessions linger after the last client leaves; count them in the
                // displayed total but NOT in capacity math, to avoid false "main full".
                "hlsSession" => target.hls += 1,
                "rtspSession" | "rtspsSession" => {
                    if local_session.get(id).copied().unwrap_or(false) {
                        target.homekit += 1;
                    } else {
                        target.rtsp += 1;
                    }
                }
                _ => target.rtsp += 1,
            }
            target.total += 1;
        }
    }
    Some(report)
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

    #[test]
    fn classifies_readers_by_type_and_locality() {
        let report = classify(PATHS, SESSIONS, "main", "sub", 1, 2).unwrap();
        assert_eq!(report.main, PathViewers { browser: 1, rtsp: 1, homekit: 0, hls: 0, total: 2, capacity: 1 });
        assert_eq!(report.sub, PathViewers { browser: 0, rtsp: 0, homekit: 1, hls: 1, total: 2, capacity: 2 });
    }

    #[test]
    fn lingering_hls_does_not_consume_capacity() {
        let paths = r#"{"items":[{"name":"main","readers":[{"type":"hlsSession","id":"h9"}]},{"name":"sub","readers":[]}]}"#;
        let report = classify(paths, SESSIONS, "main", "sub", 1, 2).unwrap();
        assert_eq!(report.main.hls, 1);
        assert!(report.main_available(), "a lingering HLS session must not mark main full");
    }

    #[test]
    fn main_availability_ignores_homekit() {
        let paths = r#"{"items":[{"name":"main","readers":[{"type":"rtspSession","id":"r2"}]},{"name":"sub","readers":[]}]}"#;
        let report = classify(paths, SESSIONS, "main", "sub", 1, 2).unwrap();
        assert_eq!(report.main.homekit, 1);
        assert!(report.main_available(), "a HomeKit reader must not consume user capacity");
    }

    #[test]
    fn malformed_json_yields_none() {
        assert!(classify("not json", SESSIONS, "main", "sub", 1, 2).is_none());
        assert!(classify(PATHS, "{}", "main", "sub", 1, 2).is_none());
    }
}
