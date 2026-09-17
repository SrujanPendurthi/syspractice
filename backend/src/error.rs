use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    /// Carries the URL because "is llama-server actually running?" is the only
    /// question anyone asks when this fires.
    #[error("model runner unreachable at {0} — start llama-server first")]
    Runner(String),

    #[error("not found")]
    NotFound,

    #[error("sign in to continue")]
    Unauthorized,

    /// Deliberately identical for "no such account" and "wrong password". Two distinct
    /// messages turn the login endpoint into an account-existence oracle, which is how
    /// a credential-stuffing list gets filtered down to real addresses.
    #[error("that email and password do not match an account")]
    BadCredentials,

    #[error("an account with that email already exists")]
    EmailTaken,

    #[error("{0}")]
    Invalid(&'static str),

    /// A malformed body, a missing field, or one too large. Axum answers these itself
    /// with plain text by default, which reaches the browser as "Unprocessable Entity"
    /// because `api.ts` finds no JSON `error` field to read.
    #[error("{0}")]
    BadRequest(String),

    /// Nothing is routed for that method on a path that does exist. Axum answers this
    /// itself with an empty body, which reaches `api.ts` as an unexplained failure.
    #[error("that method is not allowed here")]
    MethodNotAllowed,

    /// Rate limiting. 429 rather than 400 — this is "not yet", not "malformed".
    #[error("{0}")]
    Throttled(&'static str),

    /// Inference is a metered cost now, so this is a normal outcome rather than a fault.
    #[error("you have used your {0} critiques for this hour — the limit resets hourly")]
    QuotaExceeded(i64),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::BadCredentials => StatusCode::UNAUTHORIZED,
            AppError::EmailTaken | AppError::Invalid(_) => StatusCode::BAD_REQUEST,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            AppError::QuotaExceeded(_) | AppError::Throttled(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Runner(_) => StatusCode::BAD_GATEWAY,
            AppError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(json!({ "error": self.to_string() }));
        (status, body).into_response()
    }
}

/// Axum's own extractor rejections are plain text. Every one of them has to be
/// translated, not just the body's: a path id that will not parse reached the browser as
/// `Invalid URL: Cannot parse \`abc\` to a \`i64\`` with a text/plain content type, which
/// `api.ts` cannot read and reports as the bare status line. `AppJson` closed this for
/// request bodies; `AppPath` and `AppQuery` close it for the rest.
macro_rules! from_rejection {
    ($($t:ty),+) => {$(
        impl From<$t> for AppError {
            fn from(r: $t) -> Self {
                AppError::BadRequest(r.body_text())
            }
        }
    )+};
}

from_rejection!(JsonRejection, PathRejection, QueryRejection);
