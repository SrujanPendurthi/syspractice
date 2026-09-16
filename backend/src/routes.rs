//! HTTP layer. Handlers stay thin — they translate JSON to domain calls and back.
//!
//! Ownership is not enforced here. Every user-owned query in `db.rs` filters on `user_id`
//! in its own SQL, so a handler cannot forget the check by forgetting a line.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::sse::{KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_macros::FromRequest;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

use crate::auth::{self, AuthUser, MaybeUser, Throttle};
use crate::critic::Stage;
use crate::error::AppError;
use crate::scenarios::Level;
use crate::topology::Topology;
use crate::{critic, db, scenarios};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub runner_url: String,
    pub model: String,
    /// Failed sign-ins, keyed by folded email.
    pub login_throttle: Arc<Throttle>,
    /// Registrations, keyed by client address. Registration has to tell you an email is
    /// taken to be usable at all, which makes it an account-existence oracle; rate
    /// limiting is the mitigation available without sending confirmation email.
    pub register_throttle: Arc<Throttle>,
    /// Whether `X-Forwarded-For` is trustworthy here. See `auth::client_key`.
    pub trust_proxy: bool,
    /// Off for local development over plain http, on everywhere else.
    pub secure_cookies: bool,
    /// Critiques allowed per user per hour. Inference is billed by the second now, and
    /// the panel re-runs itself whenever the canvas goes quiet.
    pub critique_quota: i64,
}

/// `Json`, but its rejections come back in the same `{"error": ...}` shape as everything
/// else. Axum's own answer to a missing field is plain text with a 422, which `api.ts`
/// cannot read, so the user is shown "Unprocessable Entity".
#[derive(FromRequest)]
#[from_request(via(Json), rejection(AppError))]
pub struct AppJson<T>(pub T);

pub fn router(state: AppState) -> Router {
    // Nested so the API gets its own fallback. Without it an unknown /api path falls
    // through to the SPA and answers 200 with index.html, which `api.ts` then tries to
    // parse as JSON — a renamed route fails as an opaque SyntaxError instead of a 404.
    let api = Router::new()
        .route("/scenarios", get(list_scenarios))
        .route("/progress", get(list_progress))
        .route("/attempts", get(list_attempts).post(create_attempt))
        .route("/attempts/{id}", get(get_attempt).put(update_attempt))
        .route("/critique", post(critique))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .fallback(|| async { AppError::NotFound });

    Router::new().nest("/api", api).with_state(state)
}

async fn list_scenarios() -> impl IntoResponse {
    Json(scenarios::SCENARIOS)
}

/// Anonymous visitors get an empty list rather than a 401. The problem list is the
/// landing page, and it should render for someone who has never signed in.
async fn list_progress(
    State(s): State<AppState>,
    MaybeUser(user): MaybeUser,
) -> Result<impl IntoResponse, AppError> {
    let progress = match user {
        Some(u) => db::list_progress(&s.pool, u.id).await?,
        None => Vec::new(),
    };
    Ok(Json(progress))
}

async fn list_attempts(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(db::list_attempts(&s.pool, user.id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAttempt {
    scenario_id: String,
    topology: Topology,
    #[serde(default)]
    notes: String,
    #[serde(default = "first_stage")]
    stage: Stage,
}

/// An attempt that arrives without a stage is one that has not started the interview.
fn first_stage() -> Stage {
    Stage::Requirements
}

/// Roughly two thousand tokens of prose, which is far more than requirements and
/// back-of-the-envelope maths need.
///
/// This is a billing control as much as a storage one: `notes` is pasted verbatim into
/// the critique prompt, so without a ceiling one request can spend megabytes of inference
/// against a quota that counts requests. It is also what keeps the prompt inside the
/// model's context — overflow truncates from the front and silently drops the system
/// prompt, which is the failure CLAUDE.md describes.
const MAX_NOTES: usize = 8_000;

fn check_notes(notes: &str) -> Result<(), AppError> {
    if notes.len() > MAX_NOTES {
        return Err(AppError::Invalid("those notes are too long — keep them under 8,000 characters"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAttempt {
    topology: Topology,
    #[serde(default)]
    notes: String,
    #[serde(default = "first_stage")]
    stage: Stage,
}

async fn create_attempt(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppJson(body): AppJson<CreateAttempt>,
) -> Result<impl IntoResponse, AppError> {
    check_notes(&body.notes)?;
    // Without this a client can write attempts against scenario ids that do not exist,
    // which show up in its own progress as solved problems that are not problems.
    scenarios::find(&body.scenario_id).ok_or(AppError::NotFound)?;

    let id = db::create_attempt(
        &s.pool,
        user.id,
        &body.scenario_id,
        &body.topology,
        &body.notes,
        body.stage,
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

async fn get_attempt(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(db::get_attempt(&s.pool, user.id, id).await?))
}

async fn update_attempt(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
    AppJson(body): AppJson<UpdateAttempt>,
) -> Result<impl IntoResponse, AppError> {
    check_notes(&body.notes)?;
    db::update_attempt(&s.pool, user.id, id, &body.topology, &body.notes, body.stage).await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CritiqueRequest {
    scenario_id: String,
    topology: Topology,
    #[serde(default)]
    notes: String,
    #[serde(default = "first_stage")]
    stage: Stage,
    #[serde(default = "default_level")]
    level: Level,
}

fn default_level() -> Level {
    Level::Senior
}

async fn critique(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppJson(body): AppJson<CritiqueRequest>,
) -> Result<impl IntoResponse, AppError> {
    let scenario = scenarios::find(&body.scenario_id).ok_or(AppError::NotFound)?;
    check_notes(&body.notes)?;

    // Claimed up front and in one statement, so concurrent requests cannot each read the
    // same under-limit count and all go through. Released below if the runner never ran.
    let reservation = db::reserve_critique(&s.pool, user.id, s.critique_quota)
        .await?
        .ok_or(AppError::QuotaExceeded(s.critique_quota))?;

    let system = critic::system_prompt(body.level, body.stage);
    let prompt = critic::build_prompt(scenario, &body.topology, &body.notes, body.level);

    let stream = match critic::stream_critique(&s.runner_url, &s.model, system, prompt).await {
        Ok(stream) => stream,
        Err(e) => {
            // No GPU time was spent, so charging for it would be charging for our own
            // outage. Once the stream starts the charge stands, because closing the tab
            // does not give the seconds back.
            db::release_critique(&s.pool, reservation).await?;
            return Err(e);
        }
    };

    // Keep-alive comments stop proxies from closing the connection while the model
    // is still loading the prompt and has emitted no tokens yet.
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// --- accounts ----------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Register {
    email: String,
    display_name: String,
    password: String,
}

#[derive(Deserialize)]
struct Login {
    email: String,
    password: String,
}

async fn register(
    State(s): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    AppJson(body): AppJson<Register>,
) -> Result<impl IntoResponse, AppError> {
    // Counted before the work, and counted on success too: the limit is on how many times
    // one address may ask whether an email is taken, not on how many times it may fail.
    let who = auth::client_key(&headers, peer, s.trust_proxy);
    s.register_throttle.check(&who)?;
    s.register_throttle.record(&who);

    auth::check_email(&body.email)?;
    auth::check_password_rules(&body.password)?;

    let name = body.display_name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(AppError::Invalid("pick a display name of 1 to 40 characters"));
    }

    let hash = auth::hash_password(body.password).await?;
    let user = db::create_user(&s.pool, body.email.trim(), name, &hash).await?;

    let token = auth::issue_session(&s, user.id).await?;
    Ok((auth::set_cookie(jar, token, s.secure_cookies), Json(user)))
}

async fn login(
    State(s): State<AppState>,
    jar: CookieJar,
    AppJson(body): AppJson<Login>,
) -> Result<impl IntoResponse, AppError> {
    let key = db::fold_email(&body.email);
    s.login_throttle.check(&key)?;

    let found = db::find_user_for_login(&s.pool, &body.email).await?;

    // No such account still costs an argon2 verify, so the two failures are not
    // distinguishable by how long they take.
    let Some((user, hash)) = found else {
        auth::dummy_verify(body.password).await;
        s.login_throttle.record(&key);
        return Err(AppError::BadCredentials);
    };

    if !auth::verify_password(body.password, hash).await? {
        s.login_throttle.record(&key);
        return Err(AppError::BadCredentials);
    }

    s.login_throttle.clear(&key);
    let token = auth::issue_session(&s, user.id).await?;
    Ok((auth::set_cookie(jar, token, s.secure_cookies), Json(user)))
}

async fn logout(State(s): State<AppState>, jar: CookieJar) -> Result<impl IntoResponse, AppError> {
    // Delete the row as well as the cookie: a token that survives on a copied cookie jar
    // is the difference between signing out and hiding the evidence.
    if let Some(token) = jar.get(auth::COOKIE).map(|c| c.value().to_string()) {
        db::delete_auth_session(&s.pool, &auth::token_hash(&token)).await?;
    }
    Ok((auth::clear_cookie(jar, s.secure_cookies), Json(json!({ "ok": true }))))
}

async fn me(AuthUser(user): AuthUser) -> impl IntoResponse {
    Json(user)
}
