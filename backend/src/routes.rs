//! HTTP layer. Handlers stay thin — they translate JSON to domain calls and back.

use axum::extract::{Path, State};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

use crate::error::AppError;
use crate::topology::Topology;
use crate::{critic, db, scenarios};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub runner_url: String,
    pub model: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/scenarios", get(list_scenarios))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session).put(update_session))
        .route("/api/critique", post(critique))
        .with_state(state)
}

async fn list_scenarios() -> impl IntoResponse {
    Json(scenarios::SCENARIOS)
}

async fn list_sessions(State(s): State<AppState>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(db::list_sessions(&s.pool).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSession {
    scenario_id: String,
    topology: Topology,
}

async fn create_session(
    State(s): State<AppState>,
    Json(body): Json<CreateSession>,
) -> Result<impl IntoResponse, AppError> {
    let id = db::create_session(&s.pool, &body.scenario_id, &body.topology).await?;
    Ok(Json(json!({ "id": id })))
}

async fn get_session(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(db::get_session(&s.pool, id).await?))
}

async fn update_session(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(topology): Json<Topology>,
) -> Result<impl IntoResponse, AppError> {
    db::update_session(&s.pool, id, &topology).await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CritiqueRequest {
    scenario_id: String,
    topology: Topology,
}

async fn critique(
    State(s): State<AppState>,
    Json(body): Json<CritiqueRequest>,
) -> Result<impl IntoResponse, AppError> {
    let scenario = scenarios::find(&body.scenario_id).ok_or(AppError::NotFound)?;
    let prompt = critic::build_prompt(scenario, &body.topology);
    let stream = critic::stream_critique(&s.runner_url, &s.model, prompt).await?;

    // Keep-alive comments stop proxies from closing the connection while the model
    // is still loading the prompt and has emitted no tokens yet.
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
