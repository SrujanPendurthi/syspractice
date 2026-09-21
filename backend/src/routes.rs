//! HTTP layer. Handlers stay thin — they translate JSON to domain calls and back.
//!
//! Ownership is not enforced here. Every user-owned query in `db.rs` filters on `user_id`
//! in its own SQL, so a handler cannot forget the check by forgetting a line.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use axum_macros::{FromRequest, FromRequestParts};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

use crate::auth::{self, AuthUser, MaybeUser, Throttle};
use crate::critic::Stage;
use crate::error::AppError;
use crate::scenarios::{Level, Scenario, Track};
use crate::topology::Topology;
use crate::{concepts, critic, db, scenarios};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub runner: critic::Runner,
    /// Failed sign-ins, keyed by folded email.
    pub login_throttle: Arc<Throttle>,
    /// Registrations, keyed by client address. Registration has to tell you an email is
    /// taken to be usable at all, which makes it an account-existence oracle; rate
    /// limiting is the mitigation available without sending confirmation email.
    pub register_throttle: Arc<Throttle>,
    /// Posts and comments, keyed by user id. Moderation is a soft delete after the fact;
    /// these are what keep the queue small enough to moderate by hand.
    pub post_throttle: Arc<Throttle>,
    pub comment_throttle: Arc<Throttle>,
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

/// The same treatment for the path and the query string. A path id that will not parse as
/// an `i64` is a 400 either way — what these change is that it comes back as JSON the
/// frontend can read, rather than as `text/plain` that `api.ts` reports as the bare
/// status line with no reason attached.
#[derive(FromRequestParts)]
#[from_request(via(Path), rejection(AppError))]
pub struct AppPath<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(Query), rejection(AppError))]
pub struct AppQuery<T>(pub T);

pub fn router(state: AppState) -> Router {
    // Nested so the API gets its own fallback. Without it an unknown /api path falls
    // through to the SPA and answers 200 with index.html, which `api.ts` then tries to
    // parse as JSON — a renamed route fails as an opaque SyntaxError instead of a 404.
    let api = Router::new()
        .route("/scenarios", get(list_scenarios))
        .route("/tracks", get(list_tracks))
        .route("/progress", get(list_progress))
        .route("/attempts", get(list_attempts).post(create_attempt))
        .route("/attempts/{id}", get(get_attempt).put(update_attempt))
        .route("/critique", post(critique))
        .route("/concepts", get(list_concepts))
        .route("/concepts/{id}", get(get_concept))
        .route("/concepts/{id}/read", post(mark_concept_read))
        .route("/reads", get(list_reads))
        .route("/cards", get(list_cards))
        .route("/cards/grade", post(grade_card))
        .route("/posts", get(list_posts).post(create_post))
        .route("/posts/{id}", get(get_post).delete(delete_post))
        .route("/posts/{id}/comments", post(create_comment))
        .route("/posts/{id}/vote", post(vote_post))
        .route("/comments/{id}", delete(delete_comment))
        .route("/matches", get(list_matches).post(create_match))
        .route("/matches/{id}", get(get_match))
        .route("/matches/{id}/join", post(join_match))
        .route("/matches/{id}/cancel", post(cancel_match))
        .route("/matches/{id}/submit", post(submit_match))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .fallback(|| async { AppError::NotFound })
        // Axum answers a wrong method with an empty body, which reaches `api.ts` as a
        // failure with nothing in it to show the user.
        .method_not_allowed_fallback(|| async { AppError::MethodNotAllowed });

    Router::new().nest("/api", api).with_state(state)
}

async fn list_scenarios() -> impl IntoResponse {
    Json(scenarios::SCENARIOS)
}

/// The tracks, with their display names.
///
/// Served rather than mirrored. `Track` itself has to exist on both sides because it is on
/// every scenario, but its label is only ever rendered — putting it here means one fewer
/// string pair to keep in step across the language boundary, which is the whole complaint
/// that the hand-mirroring note in CLAUDE.md is about.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackCard {
    id: Track,
    label: &'static str,
}

async fn list_tracks() -> impl IntoResponse {
    let cards: Vec<TrackCard> = scenarios::Track::ALL
        .iter()
        .map(|t| TrackCard { id: *t, label: t.label() })
        .collect();
    Json(cards)
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
/// Shared by every track, which is what lets this stay a plain serde default.
fn first_stage() -> Stage {
    Stage::Requirements
}

/// Refuses a stage that belongs to a different interview than the problem does.
///
/// Without it a client posts `stage: "evaluationAndMonitoring"` against a classic scenario
/// and the problem shows as solved without a single stage having been walked — `solved` is
/// "reached a terminal stage", and nothing else was checking that the stage was one this
/// problem could reach.
fn check_stage(scenario: &Scenario, stage: Stage) -> Result<(), AppError> {
    if scenario.track.position(stage).is_none() {
        return Err(AppError::Invalid("that stage is not part of this problem's interview"));
    }
    Ok(())
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
    let scenario = scenarios::find(&body.scenario_id).ok_or(AppError::NotFound)?;
    check_stage(scenario, body.stage)?;

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
    AppPath(id): AppPath<i64>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(db::get_attempt(&s.pool, user.id, id).await?))
}

async fn update_attempt(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(id): AppPath<i64>,
    AppJson(body): AppJson<UpdateAttempt>,
) -> Result<impl IntoResponse, AppError> {
    check_notes(&body.notes)?;
    // The stage has to be checked against the problem this attempt is already against,
    // not one the caller names — otherwise the check is the caller's to pass or skip.
    let scenario_id = db::attempt_scenario(&s.pool, user.id, id).await?;
    let scenario = scenarios::find(&scenario_id).ok_or(AppError::NotFound)?;
    check_stage(scenario, body.stage)?;

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
    check_stage(scenario, body.stage)?;

    // Claimed up front and in one statement, so concurrent requests cannot each read the
    // same under-limit count and all go through. Released below if the runner never ran.
    let reservation = db::reserve_critique(&s.pool, user.id, s.critique_quota)
        .await?
        .ok_or(AppError::QuotaExceeded(s.critique_quota))?;

    let system = critic::system_prompt(scenario.track, body.level, body.stage);
    let prompt = critic::build_prompt(scenario, &body.topology, &body.notes, body.level);

    let stream = match critic::stream_critique(&s.runner, system, prompt).await {
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

// --- concepts ----------------------------------------------------------------

/// The list carries summaries, not bodies. Twelve full articles is a large payload for a
/// page that shows one line of each.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConceptCard {
    id: &'static str,
    title: &'static str,
    summary: &'static str,
    scenarios: &'static [&'static str],
}

async fn list_concepts() -> impl IntoResponse {
    let cards: Vec<ConceptCard> = concepts::CONCEPTS
        .iter()
        .map(|c| ConceptCard {
            id: c.id,
            title: c.title,
            summary: c.summary,
            scenarios: c.scenarios,
        })
        .collect();
    Json(cards)
}

async fn get_concept(AppPath(id): AppPath<String>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(concepts::find(&id).ok_or(AppError::NotFound)?))
}

#[derive(Deserialize)]
struct MarkRead {
    read: bool,
}

async fn mark_concept_read(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(id): AppPath<String>,
    AppJson(body): AppJson<MarkRead>,
) -> Result<impl IntoResponse, AppError> {
    // Validated against the static list so the table cannot fill with ids that no longer
    // exist — the same reason `create_attempt` checks `scenario_id`.
    concepts::find(&id).ok_or(AppError::NotFound)?;
    db::set_concept_read(&s.pool, user.id, &id, body.read).await?;
    Ok(Json(json!({ "read": body.read })))
}

/// Empty for anonymous visitors rather than a 401, for the same reason as `/api/progress`:
/// the learn page is browsable without an account.
async fn list_reads(
    State(s): State<AppState>,
    MaybeUser(user): MaybeUser,
) -> Result<impl IntoResponse, AppError> {
    let reads = match user {
        Some(u) => db::list_concept_reads(&s.pool, u.id).await?,
        None => Vec::new(),
    };
    Ok(Json(reads))
}

// --- flashcards --------------------------------------------------------------

/// How many cards one sitting serves: everything due first, then new cards up to this.
/// Without a cap a new account's first session is the whole deck, which is a reading
/// list rather than a review.
const CARD_SESSION: usize = 20;

/// Anonymous visitors get the whole deck in syllabus order — there is no schedule to
/// serve, so a session cap would only hide cards from them. Signed in, it is the due cards
/// soonest first, then unseen ones. A stored id with no card behind it (a probe that has
/// since been removed) simply never matches.
async fn list_cards(
    State(s): State<AppState>,
    MaybeUser(user): MaybeUser,
) -> Result<impl IntoResponse, AppError> {
    let Some(u) = user else {
        return Ok(Json(concepts::cards().collect::<Vec<_>>()));
    };
    let schedule = db::card_schedule(&s.pool, u.id).await?;
    let due = schedule
        .iter()
        .filter(|(_, due)| *due)
        .filter_map(|(id, _)| concepts::cards().find(|c| &c.id == id));
    let new = concepts::cards().filter(|c| !schedule.iter().any(|(id, _)| id == &c.id));
    Ok(Json(due.chain(new).take(CARD_SESSION).collect::<Vec<_>>()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Grade {
    card_id: String,
    correct: bool,
}

async fn grade_card(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppJson(body): AppJson<Grade>,
) -> Result<impl IntoResponse, AppError> {
    // Checked against the static deck so the table cannot fill with ids that are not
    // cards, the same reason `mark_concept_read` checks its id.
    if !concepts::cards().any(|c| c.id == body.card_id) {
        return Err(AppError::NotFound);
    }
    let (card_box, due_at) = db::grade_card(&s.pool, user.id, &body.card_id, body.correct).await?;
    Ok(Json(json!({ "box": card_box, "dueAt": due_at })))
}

// --- discussion ---------------------------------------------------------------

/// Roughly four pages of prose for a post and one for a comment. These are storage and
/// readability limits rather than billing ones — nothing here reaches the model — but an
/// unbounded text column is still a spam vector, and the ceiling has to exist somewhere.
const MAX_TITLE: usize = 200;
const MAX_POST: usize = 10_000;
const MAX_COMMENT: usize = 4_000;

#[derive(Deserialize)]
struct PostFilter {
    /// `?scenario=url-shortener` narrows the index to threads tagged with one problem.
    scenario: Option<String>,
}

/// Readable signed out, like the problem list and the concepts. `myVote` comes back 0 for
/// an anonymous reader rather than the endpoint refusing to answer.
async fn list_posts(
    State(s): State<AppState>,
    MaybeUser(user): MaybeUser,
    AppQuery(filter): AppQuery<PostFilter>,
) -> Result<impl IntoResponse, AppError> {
    let posts =
        db::list_posts(&s.pool, user.map(|u| u.id), filter.scenario.as_deref()).await?;
    Ok(Json(posts))
}

async fn get_post(
    State(s): State<AppState>,
    MaybeUser(user): MaybeUser,
    AppPath(id): AppPath<i64>,
) -> Result<impl IntoResponse, AppError> {
    let post = db::get_post(&s.pool, user.map(|u| u.id), id).await?;
    let comments = db::list_comments(&s.pool, id).await?;
    Ok(Json(json!({ "post": post, "comments": comments })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePost {
    title: String,
    body: String,
    /// Optional: a post does not have to hang off a problem.
    scenario_id: Option<String>,
}

/// Trims, then rejects anything empty or over the cap. Counted in characters rather than
/// bytes so a limit stated to the user as "200 characters" is the limit they get.
fn check_text(text: &str, max: usize, complaint: &'static str) -> Result<String, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err(AppError::Invalid(complaint));
    }
    Ok(trimmed.to_string())
}

async fn create_post(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppJson(body): AppJson<CreatePost>,
) -> Result<impl IntoResponse, AppError> {
    let key = user.id.to_string();
    s.post_throttle.check(&key)?;

    let title = check_text(&body.title, MAX_TITLE, "give the post a title of 1 to 200 characters")?;
    let text = check_text(&body.body, MAX_POST, "a post has to say something, and stay under 10,000 characters")?;

    // Validated against the static list for the same reason `create_attempt` validates it:
    // otherwise a client can tag threads onto problems that do not exist, and they are
    // then unreachable from the only page that filters by scenario.
    if let Some(id) = &body.scenario_id {
        scenarios::find(id).ok_or(AppError::NotFound)?;
    }

    let id = db::create_post(&s.pool, user.id, body.scenario_id.as_deref(), &title, &text).await?;
    s.post_throttle.record(&key);
    Ok(Json(json!({ "id": id })))
}

async fn delete_post(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(id): AppPath<i64>,
) -> Result<impl IntoResponse, AppError> {
    db::delete_post(&s.pool, &user, id).await?;
    Ok(Json(json!({ "deleted": id })))
}

#[derive(Deserialize)]
struct CreateComment {
    body: String,
}

async fn create_comment(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(post_id): AppPath<i64>,
    AppJson(body): AppJson<CreateComment>,
) -> Result<impl IntoResponse, AppError> {
    let key = user.id.to_string();
    s.comment_throttle.check(&key)?;

    let text = check_text(&body.body, MAX_COMMENT, "a comment has to say something, and stay under 4,000 characters")?;
    let id = db::create_comment(&s.pool, user.id, post_id, &text).await?;
    s.comment_throttle.record(&key);
    Ok(Json(json!({ "id": id })))
}

async fn delete_comment(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(id): AppPath<i64>,
) -> Result<impl IntoResponse, AppError> {
    db::delete_comment(&s.pool, &user, id).await?;
    Ok(Json(json!({ "deleted": id })))
}

#[derive(Deserialize)]
struct Vote {
    /// -1, 0 or 1. Zero withdraws a vote rather than needing a second endpoint.
    value: i64,
}

async fn vote_post(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(post_id): AppPath<i64>,
    AppJson(body): AppJson<Vote>,
) -> Result<impl IntoResponse, AppError> {
    if !(-1..=1).contains(&body.value) {
        return Err(AppError::Invalid("a vote is -1, 0 or 1"));
    }
    let score = db::set_post_vote(&s.pool, user.id, post_id, body.value).await?;
    Ok(Json(json!({ "score": score, "myVote": body.value })))
}

// --- head-to-head ---------------------------------------------------------------

/// Bounds on the clock. Under five minutes nobody finishes a diagram; over an hour it is
/// not a race, and the match sits holding a lobby slot.
const MIN_MINUTES: i64 = 5;
const MAX_MINUTES: i64 = 60;

/// The lobby: open challenges anyone can take, plus your own matches when signed in.
/// Readable signed out, because it is the page that has to make the feature look worth
/// making an account for.
async fn list_matches(
    State(s): State<AppState>,
    MaybeUser(user): MaybeUser,
) -> Result<impl IntoResponse, AppError> {
    let open = db::list_open_matches(&s.pool).await?;
    let mine = match user {
        Some(u) => db::list_my_matches(&s.pool, u.id).await?,
        None => Vec::new(),
    };
    Ok(Json(json!({ "open": open, "mine": mine })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateMatch {
    scenario_id: String,
    #[serde(default = "default_level")]
    level: Level,
    minutes: i64,
}

async fn create_match(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppJson(body): AppJson<CreateMatch>,
) -> Result<impl IntoResponse, AppError> {
    scenarios::find(&body.scenario_id).ok_or(AppError::NotFound)?;
    if !(MIN_MINUTES..=MAX_MINUTES).contains(&body.minutes) {
        return Err(AppError::Invalid("a match runs for between 5 and 60 minutes"));
    }

    let id = db::create_match(&s.pool, user.id, &body.scenario_id, body.level, body.minutes)
        .await?
        .ok_or(AppError::Invalid(
            "you already have three challenges waiting — cancel one or wait for a taker",
        ))?;
    Ok(Json(json!({ "id": id })))
}

async fn join_match(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(id): AppPath<i64>,
) -> Result<impl IntoResponse, AppError> {
    db::join_match(&s.pool, user.id, id).await?;
    Ok(Json(db::get_match(&s.pool, id).await?))
}

async fn cancel_match(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(id): AppPath<i64>,
) -> Result<impl IntoResponse, AppError> {
    db::cancel_match(&s.pool, user.id, id).await?;
    Ok(Json(json!({ "cancelled": id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitMatch {
    topology: Topology,
    #[serde(default)]
    notes: String,
}

/// Locks your board in. The attempt is an ordinary attempt — it counts toward progress and
/// shows up in the sidebar like any other, which is the point: a race is still practice.
async fn submit_match(
    State(s): State<AppState>,
    AuthUser(user): AuthUser,
    AppPath(id): AppPath<i64>,
    AppJson(body): AppJson<SubmitMatch>,
) -> Result<impl IntoResponse, AppError> {
    check_notes(&body.notes)?;
    let m = db::get_match(&s.pool, id).await?;
    if m.host_id != user.id && m.guest_id != Some(user.id) {
        // Not yours to submit to, and saying so would confirm the match exists.
        return Err(AppError::NotFound);
    }

    // Cheap pre-check, purely so a double-clicked Submit does not leave a duplicate
    // attempt behind. The UPDATE below is still the thing that decides — this cannot be
    // relied on, because the deadline can pass between here and there.
    if if m.host_id == user.id { m.host_submitted } else { m.guest_submitted } {
        return Err(AppError::Invalid(
            "you have already submitted — a board is locked in once it is sent",
        ));
    }

    // The attempt is written first: a saved board that failed to attach to a match is a
    // recoverable annoyance, whereas a match pointing at an attempt that was never stored
    // is a match that cannot be graded.
    // A submitted board has finished its interview, so it is stamped with whatever
    // finishing means for *its* track. Hard-coding `Bottlenecks` here stamped an ML board
    // with a stage its own interview never walks, leaving it permanently unfinished.
    let scenario = scenarios::find(&m.scenario_id).ok_or(AppError::NotFound)?;
    let attempt = db::create_attempt(
        &s.pool,
        user.id,
        &m.scenario_id,
        &body.topology,
        &body.notes,
        scenario.track.terminal(),
    )
    .await?;

    if !db::submit_match_attempt(&s.pool, user.id, id, attempt).await? {
        // The UPDATE is the gate; this read only picks which refusal to report, so a
        // deadline passing between the two cannot let anything through.
        let now = db::get_match(&s.pool, id).await?;
        let mine = if now.host_id == user.id { now.host_submitted } else { now.guest_submitted };
        return Err(AppError::Invalid(if mine {
            "you have already submitted — a board is locked in once it is sent"
        } else if now.cancelled {
            "that match was cancelled"
        } else if now.started_at.is_none() {
            "nobody has taken this challenge yet"
        } else {
            "the clock ran out"
        }));
    }

    Ok(Json(grade_if_ready(&s, id, &user).await?))
}

async fn get_match(
    State(s): State<AppState>,
    MaybeUser(user): MaybeUser,
    AppPath(id): AppPath<i64>,
) -> Result<impl IntoResponse, AppError> {
    match user {
        // Grading happens on read, by a participant, rather than on a timer. A background
        // job would be a second thing to run and supervise for an event that only matters
        // when somebody is looking at it.
        Some(u) => Ok(Json(grade_if_ready(&s, id, &u).await?)),
        None => Ok(Json(db::get_match(&s.pool, id).await?)),
    }
}

/// Grades the match if it is ready and nobody else has claimed it, then returns it.
///
/// Every early return hands back the match unchanged: a match that is not ready, is being
/// graded by the other player right now, or is already graded is not an error.
async fn grade_if_ready(
    s: &AppState,
    id: i64,
    viewer: &db::User,
) -> Result<db::MatchView, AppError> {
    let m = db::get_match(&s.pool, id).await?;
    let participant = m.host_id == viewer.id || m.guest_id == Some(viewer.id);
    if m.verdict.is_some() || !participant {
        return Ok(m);
    }
    if !db::claim_grading(&s.pool, id).await? {
        return Ok(m);
    }

    let (host, guest) = db::match_designs(&s.pool, id).await?;

    // A walkover needs no model: there is nothing to compare, and charging inference for
    // a verdict that is already decided by the clock would be charging for arithmetic.
    // Written in the third person: one verdict is stored and both players read it, so
    // "your opponent did not submit" is read by the opponent as if they had won.
    const WALKOVER: &str = "Only one design was submitted before the clock ran out.";
    let decided = match (&host, &guest) {
        (None, None) => Some(("Neither design was submitted before the clock ran out.", "draw")),
        (Some(_), None) => Some((WALKOVER, "host")),
        (None, Some(_)) => Some((WALKOVER, "guest")),
        (Some(_), Some(_)) => None,
    };
    if let Some((verdict, winner)) = decided {
        db::finish_grading(&s.pool, id, verdict, winner).await?;
        return db::get_match(&s.pool, id).await;
    }

    let scenario = scenarios::find(&m.scenario_id).ok_or(AppError::NotFound)?;
    let level = db::parse_level(&m.level)?;
    let (host, guest) = (host.unwrap(), guest.unwrap());

    // Which player is shown as "Design A" is decided by match id parity, so position bias
    // does not systematically favour whoever created the challenge.
    let host_is_a = critic::host_is_design_a(id);
    let (a, b) = if host_is_a { (&host, &guest) } else { (&guest, &host) };

    // ponytail: one unit of quota, charged to whoever's read triggered the grading —
    // usually the player who submitted last. It is one inference shared by two people;
    // splitting it fairly needs a second kind of ledger, which is not worth a table yet.
    let reservation = match db::reserve_critique(&s.pool, viewer.id, s.critique_quota).await? {
        Some(r) => r,
        None => {
            // Give the claim back, or the match is stamped graded with no verdict in it
            // and neither player can ever get one.
            db::release_grading(&s.pool, id).await?;
            return Err(AppError::QuotaExceeded(s.critique_quota));
        }
    };

    let prompt = critic::build_verdict_prompt(
        scenario,
        level,
        (&a.0, &a.1),
        (&b.0, &b.1),
    );

    let text = match critic::complete(&s.runner, critic::judge_prompt(level), prompt).await {
        Ok(text) => text,
        Err(e) => {
            db::release_critique(&s.pool, reservation).await?;
            db::release_grading(&s.pool, id).await?;
            return Err(e);
        }
    };

    let winner = match critic::parse_verdict(&text) {
        critic::Winner::Draw => "draw",
        critic::Winner::A => if host_is_a { "host" } else { "guest" },
        critic::Winner::B => if host_is_a { "guest" } else { "host" },
    };

    db::finish_grading(&s.pool, id, &text, winner).await?;
    db::get_match(&s.pool, id).await
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
