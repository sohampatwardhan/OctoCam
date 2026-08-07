use std::borrow::Cow;

use axum::body::Bytes;
use axum::extract::Path as AxumPath;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// The built SPA bundle, baked into the binary at compile time.
/// Path is relative to the crate root (rust/octocam-web).
///
/// NOTE (debug builds): without the `debug-embed` feature, rust-embed reads the
/// folder from disk at runtime, resolved relative to the process CWD — NOT the
/// manifest dir. Always run `cargo run`/`cargo test` for this crate from
/// `rust/octocam-web/`, or assets 404 in debug. Release builds embed at compile
/// time and are unaffected.
#[derive(RustEmbed)]
#[folder = "../../frontend/dist"]
struct SpaAssets;

/// Cache-Control for a bundle path. Vite content-hashes everything under
/// `assets/`, so those are immutable; index.html must never be cached so
/// clients always pick up a new deploy.
pub fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}

pub struct SpaResponse {
    pub status: StatusCode,
    pub content_type: String,
    pub cache_control: &'static str,
    pub body: Bytes,
}

/// Turn an embedded file's data into a `Bytes` body WITHOUT copying when the
/// bytes are already `'static` (the release/embedded case). `into_owned()` would
/// heap-clone the whole asset on every request — wasteful on the Pi Zero 2 W.
fn body_from(data: Cow<'static, [u8]>) -> Bytes {
    match data {
        Cow::Borrowed(b) => Bytes::from_static(b),
        Cow::Owned(v) => Bytes::from(v),
    }
}

/// Resolve a request path (relative to the /app mount, no leading slash) to an
/// embedded asset, falling back to index.html for client-side routes.
pub fn resolve_spa(path: &str) -> SpaResponse {
    let lookup = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = SpaAssets::get(lookup) {
        return SpaResponse {
            status: StatusCode::OK,
            content_type: file.metadata.mimetype().to_string(),
            cache_control: cache_control_for(lookup),
            body: body_from(file.data),
        };
    }

    // Not a real file: hand back index.html so the SPA router handles it.
    match SpaAssets::get("index.html") {
        Some(file) => SpaResponse {
            status: StatusCode::OK,
            content_type: "text/html; charset=utf-8".to_string(),
            cache_control: cache_control_for("index.html"),
            body: body_from(file.data),
        },
        None => SpaResponse {
            status: StatusCode::NOT_FOUND,
            content_type: "text/plain; charset=utf-8".to_string(),
            cache_control: "no-cache",
            body: Bytes::from_static(b"SPA bundle missing"),
        },
    }
}

fn into_response(r: SpaResponse) -> Response {
    let content_type = HeaderValue::from_str(&r.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    (
        r.status,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, HeaderValue::from_static(r.cache_control)),
        ],
        r.body,
    )
        .into_response()
}

/// Handler for the bare `/app` mount point.
pub async fn spa_index() -> Response {
    into_response(resolve_spa(""))
}

/// Handler for `/app/{*path}` — assets and client routes alike.
pub async fn spa_asset(AxumPath(path): AxumPath<String>) -> Response {
    into_response(resolve_spa(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_policy_matches_asset_class() {
        assert_eq!(
            cache_control_for("assets/index-a1b2c3.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control_for("index.html"), "no-cache");
        assert_eq!(cache_control_for("favicon.ico"), "public, max-age=3600");
    }

    #[test]
    fn index_is_served_for_root() {
        let r = resolve_spa("");
        assert_eq!(r.status, axum::http::StatusCode::OK);
        assert!(r.content_type.starts_with("text/html"));
        assert!(!r.body.is_empty());
    }

    #[test]
    fn unknown_client_route_falls_back_to_index() {
        // A client-side route like /app/settings is not a real file; SPA must
        // return index.html (200) so the router can take over in the browser.
        let r = resolve_spa("settings");
        assert_eq!(r.status, axum::http::StatusCode::OK);
        assert!(r.content_type.starts_with("text/html"));
    }
}
