use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::json;

/// JSON error for the /api surface. Unlike the plain-text `AppError`, this
/// carries a real status code and emits `{"error": "..."}` (plus an optional
/// machine-readable `"code"` for callers that branch on the failure kind,
/// e.g. the /api/restore error taxonomy mirrored from the form route's
/// `?restore=` querystring values).
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    pub code: Option<&'static str>,
}

impl ApiError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self { status, message: msg.into(), code: None }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
    pub fn service_unavailable(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, msg)
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, msg)
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, msg)
    }
    /// Attach a machine-readable code alongside the human-readable message.
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = match self.code {
            Some(code) => json!({ "error": self.message, "code": code }),
            None => json!({ "error": self.message }),
        };
        (self.status, Json(body)).into_response()
    }
}

pub type ApiResult = Result<Response, ApiError>;

pub fn ok_json<T: Serialize>(value: T) -> Response {
    Json(value).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn api_error_carries_status_and_json_shape() {
        let err = ApiError::bad_request("nope");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.message, "nope");
    }

    #[test]
    fn constructors_map_to_expected_statuses() {
        assert_eq!(ApiError::conflict("x").status, StatusCode::CONFLICT);
        assert_eq!(
            ApiError::service_unavailable("x").status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(ApiError::internal("x").status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn with_code_defaults_to_none_and_can_be_attached() {
        assert_eq!(ApiError::bad_request("nope").code, None);
        let err = ApiError::bad_request("too big").with_code("too_large");
        assert_eq!(err.code, Some("too_large"));
    }
}
