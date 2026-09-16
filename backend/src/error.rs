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
            AppError::QuotaExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Runner(_) => StatusCode::BAD_GATEWAY,
            AppError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(json!({ "error": self.to_string() }));
        (status, body).into_response()
    }
}
