//! Persistence. Pure data access — no HTTP types cross this boundary.
//!
//! ponytail: runtime-checked queries (`query_as`) rather than the compile-time `query!` macros,
//! so a build never needs a live DATABASE_URL or a checked-in `.sqlx` cache. Switch if the
//! query set grows enough that typos stop being caught by the tests.
//!
//! Everything owned by a user is filtered on `user_id` **inside the SQL**, never by the
//! caller afterwards. An ownership check a handler can forget to write is an ownership
//! check that will eventually not be written.

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};

use crate::critic::Stage;
use crate::scenarios::{self, Level};
use crate::error::AppError;
use crate::topology::Topology;

#[derive(Debug, FromRow)]
struct AttemptRow {
    id: i64,
    scenario_id: String,
    topology: String,
    notes: String,
    stage: String,
    created_at: String,
    updated_at: String,
}

/// One practice attempt at a scenario. Not to be confused with an auth session — that is
/// what `auth_sessions` holds, and the reason this type is no longer called `Session`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub id: i64,
    pub scenario_id: String,
    pub topology: Topology,
    pub notes: String,
    pub stage: Stage,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<AttemptRow> for Attempt {
    type Error = serde_json::Error;

    fn try_from(r: AttemptRow) -> Result<Self, Self::Error> {
        Ok(Attempt {
            id: r.id,
            scenario_id: r.scenario_id,
            topology: serde_json::from_str(&r.topology)?,
            notes: r.notes,
            // The column holds the serde tag, so it round-trips through the same JSON
            // string the wire format uses.
            stage: serde_json::from_str(&format!("\"{}\"", r.stage))?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

/// Row shape for the attempt list — the full topology is not sent until one is opened.
#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AttemptSummary {
    pub id: i64,
    pub scenario_id: String,
    pub node_count: i64,
    pub updated_at: String,
}

/// Per-scenario progress. Derived from `attempts` rather than tracked separately — the
/// stage an attempt reached is already the whole answer.
#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioProgress {
    pub scenario_id: String,
    /// True once any attempt at this scenario reached the final stage. That means the
    /// interview was walked end to end, not that the design was any good.
    pub solved: bool,
    pub attempts: i64,
    pub last_seen: String,
}

/// A registered account, as the rest of the app sees it. Deliberately has no
/// `password_hash` field so it cannot be serialized into a response by accident.
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    /// Moderator. Set by hand in SQL — see migration `0006`. It rides along on the user
    /// rather than being looked up per delete, because every delete needs it.
    pub is_admin: bool,
}

/// Each problem's own finish line, as `{scenario_id: stage_tag}` for binding.
///
/// Per scenario rather than a set of every track's terminal stage, because `solved` has to
/// mean "walked *this* interview to the end". A flat set would let an ML terminal stage
/// mark a classic problem solved — `check_stage` in `routes.rs` refuses to write one, but
/// a definition of completion that depends on a handler having been careful is a
/// definition that stops holding the first time one is not.
///
/// Derived from the scenario list, so a new track or a new problem is completable with no
/// SQL to edit and nothing here that can disagree with `Track::stages`.
fn finish_lines() -> String {
    let map: std::collections::HashMap<&str, String> = scenarios::SCENARIOS
        .iter()
        .map(|s| (s.id, stage_tag(s.track.terminal())))
        .collect();
    serde_json::to_string(&map).expect("a map of strings always serializes")
}

pub async fn list_progress(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<ScenarioProgress>, AppError> {
    // The comparison is 0 or 1 in SQLite, so MAX over the group is "any attempt got
    // there". Matching on `json_each`'s key rather than building a `$."<id>"` path keeps
    // the id out of SQL syntax entirely: a path built by concatenation fails outright on
    // an id ending in a backslash, which would 500 this user's progress on every page
    // load with no way to recover from inside the app. A row whose scenario no longer
    // exists matches nothing and reads as unsolved, which is the right answer.
    let rows = sqlx::query_as::<_, ScenarioProgress>(
        "SELECT scenario_id,
                MAX(EXISTS (SELECT 1 FROM json_each(?2) j
                            WHERE j.key = attempts.scenario_id
                              AND j.value = attempts.stage)) AS solved,
                COUNT(*)        AS attempts,
                MAX(updated_at) AS last_seen
         FROM attempts
         WHERE user_id = ?1
         GROUP BY scenario_id",
    )
    .bind(user_id)
    .bind(finish_lines())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn connect(path: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        // WAL keeps reads from blocking behind the write that saves an attempt.
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        // Off by default in SQLite, and the accounts schema leans on ON DELETE CASCADE to
        // take a user's auth sessions with them.
        .foreign_keys(true);

    // Each pooled connection to ":memory:" would get its own private database, so tests
    // must run against a single connection.
    let max = if path == ":memory:" { 1 } else { 4 };

    let pool = SqlitePoolOptions::new().max_connections(max).connect_with(opts).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

/// The serde tag for a stage, for storing in the `stage` column. Trimming the quotes off
/// the JSON keeps one source of truth for the spelling rather than a second match arm that
/// can drift from the `Serialize` derive.
fn stage_tag(stage: Stage) -> String {
    serde_json::to_string(&stage)
        .expect("Stage always serializes")
        .trim_matches('"')
        .to_string()
}

pub async fn create_attempt(
    pool: &SqlitePool,
    user_id: i64,
    scenario_id: &str,
    topology: &Topology,
    notes: &str,
    stage: Stage,
) -> Result<i64, AppError> {
    let json = serde_json::to_string(topology).expect("Topology always serializes");
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO attempts (user_id, scenario_id, topology, notes, stage)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(scenario_id)
    .bind(json)
    .bind(notes)
    .bind(stage_tag(stage))
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn update_attempt(
    pool: &SqlitePool,
    user_id: i64,
    id: i64,
    topology: &Topology,
    notes: &str,
    stage: Stage,
) -> Result<(), AppError> {
    let json = serde_json::to_string(topology).expect("Topology always serializes");
    let res = sqlx::query(
        "UPDATE attempts SET topology = ?, notes = ?, stage = ?, updated_at = datetime('now')
         WHERE id = ? AND user_id = ?",
    )
    .bind(json)
    .bind(notes)
    .bind(stage_tag(stage))
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Somebody else's attempt reads as `NotFound` rather than `Forbidden` on purpose —
/// whether that id exists is not the asker's business.
pub async fn get_attempt(pool: &SqlitePool, user_id: i64, id: i64) -> Result<Attempt, AppError> {
    let row: Option<AttemptRow> =
        sqlx::query_as("SELECT * FROM attempts WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    let row = row.ok_or(AppError::NotFound)?;
    // A row whose topology JSON no longer parses is a schema-drift bug, not a missing row.
    Attempt::try_from(row).map_err(|e| AppError::Db(sqlx::Error::Decode(Box::new(e))))
}

/// Which problem an attempt is against, scoped to its owner like everything else here.
///
/// `update_attempt` needs it to know which interview the incoming stage has to belong to,
/// and the attempt only carries the id. One indexed lookup on a path that already writes.
pub async fn attempt_scenario(
    pool: &SqlitePool,
    user_id: i64,
    id: i64,
) -> Result<String, AppError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT scenario_id FROM attempts WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.ok_or(AppError::NotFound)?.0)
}

pub async fn list_attempts(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<AttemptSummary>, AppError> {
    let rows = sqlx::query_as::<_, AttemptSummary>(
        "SELECT id, scenario_id, json_array_length(topology, '$.nodes') AS node_count, updated_at
         FROM attempts WHERE user_id = ? ORDER BY updated_at DESC LIMIT 100",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// --- accounts ---------------------------------------------------------------

/// `email_folded` is the uniqueness key; `email` keeps whatever the user typed.
/// Returns `AppError::EmailTaken` rather than a raw constraint violation so the caller
/// does not have to pattern-match on sqlite error codes.
pub async fn create_user(
    pool: &SqlitePool,
    email: &str,
    display_name: &str,
    password_hash: &str,
) -> Result<User, AppError> {
    let res: Result<(i64,), sqlx::Error> = sqlx::query_as(
        "INSERT INTO users (email, email_folded, display_name, password_hash)
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(email)
    .bind(fold_email(email))
    .bind(display_name)
    .bind(password_hash)
    .fetch_one(pool)
    .await;

    match res {
        Ok(row) => Ok(User {
            id: row.0,
            email: email.to_string(),
            display_name: display_name.to_string(),
            is_admin: false,
        }),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(AppError::EmailTaken),
        Err(e) => Err(AppError::Db(e)),
    }
}

/// The hash comes back with the user because login has to verify it. Nothing else calls
/// this, which is why it is the only place `password_hash` leaves the database.
pub async fn find_user_for_login(
    pool: &SqlitePool,
    email: &str,
) -> Result<Option<(User, String)>, AppError> {
    let row: Option<(i64, String, String, bool, String)> = sqlx::query_as(
        "SELECT id, email, display_name, is_admin, password_hash FROM users WHERE email_folded = ?",
    )
    .bind(fold_email(email))
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, email, display_name, is_admin, hash)| {
        (User { id, email, display_name, is_admin }, hash)
    }))
}

/// Lowercase and trim. Not full Unicode case folding, but it is the same transformation
/// on signup and on login, which is the property that actually matters.
pub fn fold_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub async fn store_auth_session(
    pool: &SqlitePool,
    token_hash: &[u8],
    user_id: i64,
    days: i64,
) -> Result<(), AppError> {
    // Expired rows are harmless — `user_for_token` filters them — but nothing was ever
    // deleting them. Sweeping this user's on the way in keeps the table bounded by live
    // sessions without needing a background job.
    sqlx::query("DELETE FROM auth_sessions WHERE user_id = ? AND expires_at <= datetime('now')")
        .bind(user_id)
        .execute(pool)
        .await?;

    sqlx::query(
        "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
         VALUES (?, ?, datetime('now', ?))",
    )
    .bind(token_hash)
    .bind(user_id)
    // No leading '+': SQLite takes the sign from the number, and "+-1 days" is not a
    // modifier it understands — it evaluates the whole expression to NULL.
    .bind(format!("{days} days"))
    .execute(pool)
    .await?;
    Ok(())
}

/// The expiry check is in the SQL, so an expired row can never be mistaken for a live one
/// by a caller that forgot to compare timestamps.
pub async fn user_for_token(
    pool: &SqlitePool,
    token_hash: &[u8],
) -> Result<Option<User>, AppError> {
    let row: Option<User> = sqlx::query_as(
        "SELECT u.id, u.email, u.display_name, u.is_admin
         FROM auth_sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ? AND s.expires_at > datetime('now')",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_auth_session(pool: &SqlitePool, token_hash: &[u8]) -> Result<(), AppError> {
    sqlx::query("DELETE FROM auth_sessions WHERE token_hash = ?")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Claims one unit of quota, or `None` when the user is already at the limit.
///
/// The count and the insert are one statement on purpose. Read-then-write let concurrent
/// requests all read the same under-limit count and all proceed — measured at 5 through on
/// a limit of 2 with twelve in flight, which is a 2.5x overrun on a billing control.
/// SQLite serializes writers, so evaluating the count inside the INSERT is enough; no
/// explicit transaction is needed.
pub async fn reserve_critique(
    pool: &SqlitePool,
    user_id: i64,
    quota: i64,
) -> Result<Option<i64>, AppError> {
    let row: Option<(i64,)> = sqlx::query_as(
        "INSERT INTO critique_log (user_id)
         SELECT ?
         WHERE (SELECT COUNT(*) FROM critique_log
                WHERE user_id = ? AND created_at > datetime('now', '-1 hour')) < ?
         RETURNING id",
    )
    .bind(user_id)
    .bind(user_id)
    .bind(quota)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

/// Gives a reservation back when the critique never happened. The runner refusing the
/// request costs no GPU time, so charging for it would be charging for our own outage.
pub async fn release_critique(pool: &SqlitePool, id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM critique_log WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}

pub async fn critiques_in_last_hour(pool: &SqlitePool, user_id: i64) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM critique_log
         WHERE user_id = ? AND created_at > datetime('now', '-1 hour')",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

// --- concepts ----------------------------------------------------------------

/// The concept ids this user has marked read. Just the ids: the text is static and comes
/// from `concepts.rs`, so there is nothing to join to.
pub async fn list_concept_reads(pool: &SqlitePool, user_id: i64) -> Result<Vec<String>, AppError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT concept_id FROM concept_reads WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Marking something read twice is not an error, so the insert is idempotent rather than
/// something every caller has to check first.
pub async fn set_concept_read(
    pool: &SqlitePool,
    user_id: i64,
    concept_id: &str,
    read: bool,
) -> Result<(), AppError> {
    if read {
        sqlx::query(
            "INSERT INTO concept_reads (user_id, concept_id) VALUES (?, ?)
             ON CONFLICT (user_id, concept_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(concept_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("DELETE FROM concept_reads WHERE user_id = ? AND concept_id = ?")
            .bind(user_id)
            .bind(concept_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

// --- flashcards --------------------------------------------------------------

/// The highest Leitner box. Box `n` waits `2^(n-1)` days: 1, 2, 4, 8, 16.
pub const TOP_BOX: i64 = 5;

/// Every card this user has graded, soonest due first, with whether it is due now. The
/// comparison is done in SQL so both sides are SQLite's own UTC text and nothing has to
/// parse a zone-less timestamp.
pub async fn card_schedule(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<(String, bool)>, AppError> {
    Ok(sqlx::query_as(
        "SELECT card_id, due_at <= datetime('now') FROM card_reviews
         WHERE user_id = ? ORDER BY due_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Got it moves a card up one box, missed it sends it back to box 1; either way it is due
/// again after its new box's interval. One upsert, so a double-clicked grade cannot read a
/// box and then write a stale one. A new card that is got right starts in box 2 — it has
/// been answered once, which is what box 1 is waiting to find out.
///
/// The interval is `N || ' days'` with no leading `+`: SQLite takes the sign from the
/// number, and `+N days` is the trap `store_auth_session` hit.
pub async fn grade_card(
    pool: &SqlitePool,
    user_id: i64,
    card_id: &str,
    correct: bool,
) -> Result<(i64, String), AppError> {
    Ok(sqlx::query_as(
        "INSERT INTO card_reviews (user_id, card_id, box, due_at)
         VALUES (?1, ?2, ?3, datetime('now', (1 << (?3 - 1)) || ' days'))
         ON CONFLICT (user_id, card_id) DO UPDATE SET
             box = CASE WHEN ?4 THEN MIN(box + 1, ?5) ELSE 1 END,
             due_at = datetime('now', (1 << (CASE WHEN ?4 THEN MIN(box + 1, ?5) ELSE 1 END - 1))
                                      || ' days')
         RETURNING box, due_at",
    )
    .bind(user_id)
    .bind(card_id)
    .bind(if correct { 2_i64 } else { 1 })
    .bind(correct)
    .bind(TOP_BOX)
    .fetch_one(pool)
    .await?)
}

// --- discussion ---------------------------------------------------------------

/// A post as the index shows it. No `body`: a hundred posts of ten thousand characters is
/// a megabyte for a page that renders titles, the same reason `/api/concepts` sends
/// summaries only.
#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PostSummary {
    pub id: i64,
    pub scenario_id: Option<String>,
    pub title: String,
    pub author: String,
    pub author_id: i64,
    pub score: i64,
    /// This viewer's vote: -1, 0 or 1. Always 0 when signed out.
    pub my_vote: i64,
    pub comments: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: i64,
    pub scenario_id: Option<String>,
    pub title: String,
    pub body: String,
    pub author: String,
    pub author_id: i64,
    pub score: i64,
    pub my_vote: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: i64,
    pub body: String,
    pub author: String,
    pub author_id: i64,
    pub created_at: String,
}

/// Score and comment count are summed on read rather than kept in a counter column, for
/// the same reason progress is derived: a counter needs a second write on every vote and
/// is wrong forever the one time that write is lost.
///
/// ponytail: correlated subqueries and newest-first. Both are fine while a page of posts
/// is a page of posts; if the index gets slow, that is the moment for a ranking column,
/// not before.
const POST_COLUMNS: &str = "
    p.id, p.scenario_id, p.title, p.created_at,
    u.display_name AS author,
    u.id           AS author_id,
    COALESCE((SELECT SUM(value) FROM post_votes WHERE post_id = p.id), 0)        AS score,
    COALESCE((SELECT value FROM post_votes WHERE post_id = p.id AND user_id = ?1), 0) AS my_vote";

/// `viewer` is `None` for a signed-out reader — the discussion is public to read.
/// `scenario` filters to threads tagged with one problem.
pub async fn list_posts(
    pool: &SqlitePool,
    viewer: Option<i64>,
    scenario: Option<&str>,
) -> Result<Vec<PostSummary>, AppError> {
    let sql = format!(
        "SELECT {POST_COLUMNS},
                (SELECT COUNT(*) FROM comments
                 WHERE post_id = p.id AND deleted_at IS NULL) AS comments
         FROM posts p JOIN users u ON u.id = p.user_id
         WHERE p.deleted_at IS NULL AND (?2 IS NULL OR p.scenario_id = ?2)
         ORDER BY p.created_at DESC LIMIT 100"
    );
    let rows = sqlx::query_as::<_, PostSummary>(&sql)
        .bind(viewer)
        .bind(scenario)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get_post(pool: &SqlitePool, viewer: Option<i64>, id: i64) -> Result<Post, AppError> {
    let sql = format!(
        "SELECT {POST_COLUMNS}, p.body
         FROM posts p JOIN users u ON u.id = p.user_id
         WHERE p.id = ?2 AND p.deleted_at IS NULL"
    );
    sqlx::query_as::<_, Post>(&sql)
        .bind(viewer)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn create_post(
    pool: &SqlitePool,
    user_id: i64,
    scenario_id: Option<&str>,
    title: &str,
    body: &str,
) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO posts (user_id, scenario_id, title, body) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(scenario_id)
    .bind(title)
    .bind(body)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// The author or a moderator, decided in the SQL like every other ownership check. A post
/// belonging to somebody else reads as `NotFound`, not `Forbidden`.
pub async fn delete_post(
    pool: &SqlitePool,
    user: &User,
    id: i64,
) -> Result<(), AppError> {
    let res = sqlx::query(
        "UPDATE posts SET deleted_at = datetime('now')
         WHERE id = ? AND deleted_at IS NULL AND (user_id = ? OR ? = 1)",
    )
    .bind(id)
    .bind(user.id)
    .bind(user.is_admin)
    .execute(pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn list_comments(pool: &SqlitePool, post_id: i64) -> Result<Vec<Comment>, AppError> {
    let rows = sqlx::query_as::<_, Comment>(
        "SELECT c.id, c.body, c.created_at, u.display_name AS author, u.id AS author_id
         FROM comments c JOIN users u ON u.id = c.user_id
         WHERE c.post_id = ? AND c.deleted_at IS NULL
         ORDER BY c.created_at LIMIT 500",
    )
    .bind(post_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The insert is conditional on the post being alive, so a comment cannot be attached to
/// something already removed — the foreign key only proves the row exists, not that it is
/// still visible.
pub async fn create_comment(
    pool: &SqlitePool,
    user_id: i64,
    post_id: i64,
    body: &str,
) -> Result<i64, AppError> {
    let row: Option<(i64,)> = sqlx::query_as(
        "INSERT INTO comments (post_id, user_id, body)
         SELECT ?, ?, ?
         WHERE EXISTS (SELECT 1 FROM posts WHERE id = ? AND deleted_at IS NULL)
         RETURNING id",
    )
    .bind(post_id)
    .bind(user_id)
    .bind(body)
    .bind(post_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.0).ok_or(AppError::NotFound)
}

pub async fn delete_comment(pool: &SqlitePool, user: &User, id: i64) -> Result<(), AppError> {
    let res = sqlx::query(
        "UPDATE comments SET deleted_at = datetime('now')
         WHERE id = ? AND deleted_at IS NULL AND (user_id = ? OR ? = 1)",
    )
    .bind(id)
    .bind(user.id)
    .bind(user.is_admin)
    .execute(pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// `value` is -1, 0 or 1; 0 withdraws the vote. Returns the post's new score.
///
/// The upsert is what keeps one user to one ballot — without the primary key conflict
/// clause, voting twice would insert twice and the sum would count both.
pub async fn set_post_vote(
    pool: &SqlitePool,
    user_id: i64,
    post_id: i64,
    value: i64,
) -> Result<i64, AppError> {
    let affected = if value == 0 {
        sqlx::query("DELETE FROM post_votes WHERE post_id = ? AND user_id = ?")
            .bind(post_id)
            .bind(user_id)
            .execute(pool)
            .await?;
        // A withdrawn vote that was never cast is not an error, so existence is checked
        // separately rather than inferred from rows_affected.
        post_is_live(pool, post_id).await?
    } else {
        // The WHERE on the SELECT is also what lets SQLite tell the upsert clause apart
        // from a join constraint; an unconditional INSERT ... SELECT ... ON CONFLICT is
        // a parse error.
        sqlx::query(
            "INSERT INTO post_votes (post_id, user_id, value)
             SELECT ?, ?, ?
             WHERE EXISTS (SELECT 1 FROM posts WHERE id = ? AND deleted_at IS NULL)
             ON CONFLICT (post_id, user_id) DO UPDATE SET value = excluded.value",
        )
        .bind(post_id)
        .bind(user_id)
        .bind(value)
        .bind(post_id)
        .execute(pool)
        .await?
        .rows_affected()
            > 0
    };

    if !affected {
        return Err(AppError::NotFound);
    }

    let row: (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(value), 0) FROM post_votes WHERE post_id = ?")
            .bind(post_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

async fn post_is_live(pool: &SqlitePool, post_id: i64) -> Result<bool, AppError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM posts WHERE id = ? AND deleted_at IS NULL")
        .bind(post_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

// --- head-to-head ---------------------------------------------------------------

/// A match as either player is allowed to see it.
///
/// There is deliberately no `host_attempt_id` or `guest_attempt_id` here. Those are the
/// two boards, and handing an opponent the id of yours mid-race is handing them your
/// design — the whole exercise is that neither of you can see the other until the clock
/// stops. Only whether each side has *submitted* crosses the wire.
#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MatchView {
    pub id: i64,
    pub scenario_id: String,
    pub level: String,
    pub minutes: i64,
    pub host_id: i64,
    pub host: String,
    pub guest_id: Option<i64>,
    pub guest: Option<String>,
    pub started_at: Option<String>,
    /// Computed, never stored: a second copy of `started_at + minutes` is a fact that can
    /// contradict the first. The client counts down against this rather than being pushed
    /// ticks — a deadline is a timestamp, and a timestamp does not need a live connection.
    pub deadline: Option<String>,
    pub host_submitted: bool,
    pub guest_submitted: bool,
    pub cancelled: bool,
    pub verdict: Option<String>,
    /// "host", "guest" or "draw". Stored as the side rather than as A/B, because which
    /// player was shown to the judge as A is a detail of how bias was handled.
    pub winner: Option<String>,
    pub created_at: String,
}

const MATCH_COLUMNS: &str = "
    m.id, m.scenario_id, m.level, m.minutes,
    m.host_id, h.display_name AS host,
    m.guest_id, g.display_name AS guest,
    m.started_at,
    -- No leading plus: SQLite takes the sign from the number, and a plus in front of a
    -- negative one is not a modifier it understands — the whole expression evaluates to
    -- NULL, which would leave the match unfinishable rather than expired. Same trap as
    -- the session expiry in store_auth_session.
    CASE WHEN m.started_at IS NULL THEN NULL
         ELSE datetime(m.started_at, m.minutes || ' minutes') END AS deadline,
    m.host_attempt_id  IS NOT NULL AS host_submitted,
    m.guest_attempt_id IS NOT NULL AS guest_submitted,
    m.cancelled_at     IS NOT NULL AS cancelled,
    m.verdict, m.winner, m.created_at
  FROM matches m
  JOIN users h ON h.id = m.host_id
  LEFT JOIN users g ON g.id = m.guest_id";

/// How many challenges one person may leave sitting in the lobby unanswered.
///
/// This is the rate limit for matches, and it is a cap on outstanding rows rather than a
/// count per hour because that is the thing being protected: the lobby is a shared page,
/// and one account posting fifty challenges makes it useless for everyone whether it did
/// so in a minute or over a day. It also costs nothing to stay within — finish or cancel
/// one and you have another.
pub const OPEN_MATCH_LIMIT: i64 = 3;

/// `None` when the host is already at `OPEN_MATCH_LIMIT`. Counted inside the INSERT for
/// the same reason `reserve_critique` is: read-then-write lets concurrent requests all
/// see the same under-limit count.
pub async fn create_match(
    pool: &SqlitePool,
    host_id: i64,
    scenario_id: &str,
    level: Level,
    minutes: i64,
) -> Result<Option<i64>, AppError> {
    let row: Option<(i64,)> = sqlx::query_as(
        "INSERT INTO matches (host_id, scenario_id, level, minutes)
         SELECT ?, ?, ?, ?
         WHERE (SELECT COUNT(*) FROM matches
                WHERE host_id = ? AND guest_id IS NULL AND cancelled_at IS NULL) < ?
         RETURNING id",
    )
    .bind(host_id)
    .bind(scenario_id)
    .bind(level_tag(level))
    .bind(minutes)
    .bind(host_id)
    .bind(OPEN_MATCH_LIMIT)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

pub async fn get_match(pool: &SqlitePool, id: i64) -> Result<MatchView, AppError> {
    sqlx::query_as::<_, MatchView>(&format!("SELECT {MATCH_COLUMNS} WHERE m.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

/// Challenges nobody has taken yet. Anyone may read the lobby, signed in or not — it is
/// the page that has to make the feature look worth signing up for.
pub async fn list_open_matches(pool: &SqlitePool) -> Result<Vec<MatchView>, AppError> {
    let rows = sqlx::query_as::<_, MatchView>(&format!(
        "SELECT {MATCH_COLUMNS}
         WHERE m.guest_id IS NULL AND m.cancelled_at IS NULL
         ORDER BY m.created_at DESC LIMIT 50"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_my_matches(pool: &SqlitePool, user_id: i64) -> Result<Vec<MatchView>, AppError> {
    let rows = sqlx::query_as::<_, MatchView>(&format!(
        "SELECT {MATCH_COLUMNS}
         WHERE m.host_id = ?1 OR m.guest_id = ?1
         ORDER BY m.created_at DESC LIMIT 50"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Takes an open challenge and starts the clock.
///
/// One statement, so two people pressing Join at the same moment cannot both become the
/// guest — the second sees `guest_id` already set and matches nothing. `host_id <> ?`
/// stops someone racing themselves, which would otherwise be a free win.
pub async fn join_match(pool: &SqlitePool, user_id: i64, id: i64) -> Result<(), AppError> {
    let res = sqlx::query(
        "UPDATE matches SET guest_id = ?, started_at = datetime('now')
         WHERE id = ? AND guest_id IS NULL AND cancelled_at IS NULL AND host_id <> ?",
    )
    .bind(user_id)
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::Invalid("that challenge is no longer open"));
    }
    Ok(())
}

/// Only the host, and only while nobody has joined. Once the clock is running the other
/// player has spent time on it, and withdrawing would be a way to deny them a result.
pub async fn cancel_match(pool: &SqlitePool, user_id: i64, id: i64) -> Result<(), AppError> {
    let res = sqlx::query(
        "UPDATE matches SET cancelled_at = datetime('now')
         WHERE id = ? AND host_id = ? AND guest_id IS NULL AND cancelled_at IS NULL",
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Locks one player's board in. Submitting is final: the `IS NULL` guard is what makes it
/// one shot, and without it you could keep editing after seeing the clock run down on
/// your opponent.
///
/// Every condition is in this one statement rather than checked first, so the deadline
/// cannot pass between the check and the write. The caller may still read the row
/// afterwards to decide *which* refusal to report — that read is for the error message
/// only, and this UPDATE stays the thing that actually decides.
pub async fn submit_match_attempt(
    pool: &SqlitePool,
    user_id: i64,
    match_id: i64,
    attempt_id: i64,
) -> Result<bool, AppError> {
    let res = sqlx::query(
        "UPDATE matches
         SET host_attempt_id  = CASE WHEN host_id  = ?1 THEN ?2 ELSE host_attempt_id  END,
             guest_attempt_id = CASE WHEN guest_id = ?1 THEN ?2 ELSE guest_attempt_id END
         WHERE id = ?3
           AND cancelled_at IS NULL
           AND started_at IS NOT NULL
           AND datetime('now') <= datetime(started_at, minutes || ' minutes')
           AND (host_id = ?1 OR guest_id = ?1)
           AND (CASE WHEN host_id = ?1 THEN host_attempt_id ELSE guest_attempt_id END) IS NULL",
    )
    .bind(user_id)
    .bind(attempt_id)
    .bind(match_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// The two boards, for the judge. Read straight from `attempts`, so a match cannot show
/// a design its player did not save.
pub async fn match_designs(
    pool: &SqlitePool,
    match_id: i64,
) -> Result<(Option<(Topology, String)>, Option<(Topology, String)>), AppError> {
    let row: Option<(Option<String>, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT ha.topology, ha.notes, ga.topology, ga.notes
             FROM matches m
             LEFT JOIN attempts ha ON ha.id = m.host_attempt_id
             LEFT JOIN attempts ga ON ga.id = m.guest_attempt_id
             WHERE m.id = ?",
        )
        .bind(match_id)
        .fetch_optional(pool)
        .await?;

    let (ht, hn, gt, gn) = row.ok_or(AppError::NotFound)?;
    let parse = |t: Option<String>, n: Option<String>| -> Result<Option<(Topology, String)>, AppError> {
        match t {
            None => Ok(None),
            Some(t) => {
                let topology = serde_json::from_str(&t)
                    .map_err(|e| AppError::Db(sqlx::Error::Decode(Box::new(e))))?;
                Ok(Some((topology, n.unwrap_or_default())))
            }
        }
    };
    Ok((parse(ht, hn)?, parse(gt, gn)?))
}

/// Claims the right to grade this match, or returns false because it is not gradable yet
/// or somebody else already has it.
///
/// `graded_at` is stamped *before* the model runs, not after. Both players open the
/// result page at roughly the same moment — the moment the clock stops — so without this
/// the same match is graded twice, billed twice, and can produce two different answers to
/// the same question.
pub async fn claim_grading(pool: &SqlitePool, match_id: i64) -> Result<bool, AppError> {
    let res = sqlx::query(
        "UPDATE matches SET graded_at = datetime('now')
         WHERE id = ?
           AND graded_at IS NULL
           AND cancelled_at IS NULL
           AND started_at IS NOT NULL
           AND ((host_attempt_id IS NOT NULL AND guest_attempt_id IS NOT NULL)
                OR datetime('now') > datetime(started_at, minutes || ' minutes'))",
    )
    .bind(match_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn finish_grading(
    pool: &SqlitePool,
    match_id: i64,
    verdict: &str,
    winner: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE matches SET verdict = ?, winner = ? WHERE id = ?")
        .bind(verdict)
        .bind(winner)
        .bind(match_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Puts a claimed grading back when the model never answered. Without this a single
/// runner outage leaves the match permanently stamped as graded with no verdict in it —
/// unreadable, and unretryable.
pub async fn release_grading(pool: &SqlitePool, match_id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE matches SET graded_at = NULL WHERE id = ? AND verdict IS NULL")
        .bind(match_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The serde tag for a level, matching `stage_tag`'s reasoning: one source of truth for
/// the spelling rather than a match arm that can drift from the derive.
fn level_tag(level: Level) -> String {
    serde_json::to_string(&level)
        .expect("Level always serializes")
        .trim_matches('"')
        .to_string()
}

/// The inverse. A stored tag that no longer parses is schema drift, not a missing row,
/// so it fails loudly rather than quietly grading everyone as a senior.
pub fn parse_level(tag: &str) -> Result<Level, AppError> {
    serde_json::from_str(&format!("\"{tag}\""))
        .map_err(|e| AppError::Db(sqlx::Error::Decode(Box::new(e))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::Track;
    use crate::topology::{Node, NodeKind};

    /// Tests predate accounts; this keeps them readable by giving each one an owner.
    async fn user(pool: &SqlitePool, email: &str) -> i64 {
        create_user(pool, email, "Test", "not-a-real-hash").await.unwrap().id
    }

    #[tokio::test]
    async fn progress_tracks_the_furthest_stage_reached() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        create_attempt(&pool, u, "url-shortener", &topo(1), "", Stage::HighLevel).await.unwrap();
        create_attempt(&pool, u, "chat-app", &topo(1), "", Stage::Bottlenecks).await.unwrap();

        let progress = list_progress(&pool, u).await.unwrap();
        let get = |id: &str| {
            progress.iter().find(|p| p.scenario_id == id).expect("scenario missing from progress")
        };

        assert!(!get("url-shortener").solved, "mid-interview is attempted, not solved");
        assert!(get("chat-app").solved, "reaching the final stage is what counts as solved");
        assert_eq!(get("chat-app").attempts, 1);

        // A second attempt that stops early must not un-solve the scenario.
        create_attempt(&pool, u, "chat-app", &topo(1), "", Stage::Requirements).await.unwrap();
        let progress = list_progress(&pool, u).await.unwrap();
        let chat = progress.iter().find(|p| p.scenario_id == "chat-app").unwrap();
        assert!(chat.solved, "a later shallow attempt must not clear an earlier completion");
        assert_eq!(chat.attempts, 2);
    }

    #[tokio::test]
    async fn untouched_scenarios_are_simply_absent() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;
        assert!(list_progress(&pool, u).await.unwrap().is_empty());
    }

    #[test]
    fn stage_tags_match_the_wire_format() {
        assert_eq!(stage_tag(Stage::HighLevel), "highLevel");
        assert_eq!(stage_tag(Stage::Requirements), "requirements");
    }

    fn topo(n: usize) -> Topology {
        Topology {
            nodes: (0..n)
                .map(|i| Node {
                    id: format!("n{i}"),
                    kind: NodeKind::Service,
                    label: format!("svc{i}"),
                    x: 0.0,
                    y: 0.0,
                })
                .collect(),
            edges: vec![],
        }
    }

    #[tokio::test]
    async fn saves_loads_and_lists() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        let id = create_attempt(&pool, u, "url-shortener", &topo(2), "10k QPS", Stage::Requirements)
            .await
            .unwrap();
        let got = get_attempt(&pool, u, id).await.unwrap();
        assert_eq!(got.topology.nodes.len(), 2);
        assert_eq!(got.scenario_id, "url-shortener");
        assert_eq!(got.notes, "10k QPS");
        assert_eq!(got.stage, Stage::Requirements);

        update_attempt(&pool, u, id, &topo(5), "18TB year one", Stage::DeepDive).await.unwrap();
        let got = get_attempt(&pool, u, id).await.unwrap();
        assert_eq!(got.topology.nodes.len(), 5);
        assert_eq!(got.notes, "18TB year one", "notes must survive an update");
        assert_eq!(got.stage, Stage::DeepDive, "the interview must resume where it left off");

        let list = list_attempts(&pool, u).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].node_count, 5, "node_count must track the saved topology");
    }

    #[tokio::test]
    async fn missing_rows_are_not_found() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;
        assert!(matches!(get_attempt(&pool, u, 999).await, Err(AppError::NotFound)));
        let missing = update_attempt(&pool, u, 999, &topo(1), "", Stage::Requirements).await;
        assert!(matches!(missing, Err(AppError::NotFound)));
    }

    #[tokio::test]
    async fn one_user_cannot_touch_anothers_attempt() {
        let pool = connect(":memory:").await.unwrap();
        let alice = user(&pool, "alice@example.com").await;
        let bob = user(&pool, "bob@example.com").await;

        let id =
            create_attempt(&pool, alice, "url-shortener", &topo(3), "alice's", Stage::DeepDive)
                .await
                .unwrap();

        // The dangerous failure is not an error — it is a 200 carrying Alice's diagram.
        match get_attempt(&pool, bob, id).await {
            Err(AppError::NotFound) => {}
            Ok(a) => panic!("bob read alice's attempt: {} nodes", a.topology.nodes.len()),
            Err(e) => panic!("expected NotFound, got {e}"),
        }
        assert!(matches!(
            update_attempt(&pool, bob, id, &topo(1), "overwritten", Stage::Requirements).await,
            Err(AppError::NotFound)
        ));

        // ...and Alice's row is untouched by the attempt to overwrite it.
        let still = get_attempt(&pool, alice, id).await.unwrap();
        assert_eq!(still.notes, "alice's");

        assert!(list_attempts(&pool, bob).await.unwrap().is_empty());
        assert!(list_progress(&pool, bob).await.unwrap().is_empty(), "bob sees alice's checkmarks");
    }

    #[tokio::test]
    async fn an_email_is_taken_regardless_of_case() {
        let pool = connect(":memory:").await.unwrap();
        create_user(&pool, "Alice@Example.com", "Alice", "h").await.unwrap();

        assert!(matches!(
            create_user(&pool, "alice@example.com", "Impostor", "h").await,
            Err(AppError::EmailTaken)
        ));

        // Login has to find the account through the same folding, or a user who typed
        // their address with different capitalisation can never sign in again.
        let found = find_user_for_login(&pool, " ALICE@EXAMPLE.COM ").await.unwrap();
        let (u, hash) = found.expect("folded lookup missed the account");
        assert_eq!(u.email, "Alice@Example.com", "the typed form is what we show back");
        assert_eq!(hash, "h");
    }

    #[tokio::test]
    async fn an_expired_token_is_not_a_login() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        store_auth_session(&pool, b"live", u, 30).await.unwrap();
        assert!(user_for_token(&pool, b"live").await.unwrap().is_some());

        store_auth_session(&pool, b"stale", u, -1).await.unwrap();
        assert!(user_for_token(&pool, b"stale").await.unwrap().is_none(), "expired token accepted");

        delete_auth_session(&pool, b"live").await.unwrap();
        assert!(user_for_token(&pool, b"live").await.unwrap().is_none(), "logout left it usable");
    }

    #[tokio::test]
    async fn a_reservation_is_refused_at_the_limit_and_refunded_on_failure() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        let first = reserve_critique(&pool, u, 2).await.unwrap().expect("first is within quota");
        assert!(reserve_critique(&pool, u, 2).await.unwrap().is_some());
        assert!(reserve_critique(&pool, u, 2).await.unwrap().is_none(), "third exceeded the quota");

        // Giving one back has to free a slot, or a runner outage permanently eats quota.
        release_critique(&pool, first).await.unwrap();
        assert_eq!(critiques_in_last_hour(&pool, u).await.unwrap(), 1);
        assert!(reserve_critique(&pool, u, 2).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn expired_sessions_are_swept_when_a_new_one_is_issued() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        store_auth_session(&pool, b"stale", u, -1).await.unwrap();
        store_auth_session(&pool, b"live", u, 30).await.unwrap();

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM auth_sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "the expired row should have been swept, not accumulated");
        assert!(user_for_token(&pool, b"live").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn marking_a_concept_read_is_idempotent_and_reversible() {
        let pool = connect(":memory:").await.unwrap();
        let alice = user(&pool, "alice@example.com").await;
        let bob = user(&pool, "bob@example.com").await;

        set_concept_read(&pool, alice, "caching", true).await.unwrap();
        set_concept_read(&pool, alice, "caching", true).await.unwrap();
        assert_eq!(list_concept_reads(&pool, alice).await.unwrap(), vec!["caching"]);
        assert!(list_concept_reads(&pool, bob).await.unwrap().is_empty(), "reads are per user");

        set_concept_read(&pool, alice, "caching", false).await.unwrap();
        assert!(list_concept_reads(&pool, alice).await.unwrap().is_empty());
    }

    /// Days from now until `due_at`, as SQLite itself measures it.
    async fn days_until(pool: &SqlitePool, due_at: &str) -> f64 {
        let (d,): (f64,) = sqlx::query_as("SELECT julianday(?) - julianday('now')")
            .bind(due_at)
            .fetch_one(pool)
            .await
            .unwrap();
        d
    }

    #[tokio::test]
    async fn grading_walks_the_leitner_boxes_and_a_miss_starts_over() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        // New and right lands in box 2; each further hit doubles the wait, capped at 16 days.
        for (want_box, want_days) in [(2, 2.0), (3, 4.0), (4, 8.0), (5, 16.0), (5, 16.0)] {
            let (b, due) = grade_card(&pool, u, "caching:0", true).await.unwrap();
            assert_eq!(b, want_box);
            assert!((days_until(&pool, &due).await - want_days).abs() < 0.01, "box {b}: {due}");
        }

        let (b, due) = grade_card(&pool, u, "caching:0", false).await.unwrap();
        assert_eq!(b, 1, "a miss goes back to box 1 from anywhere");
        assert!((days_until(&pool, &due).await - 1.0).abs() < 0.01);

        let (b, _) = grade_card(&pool, u, "caching:1", false).await.unwrap();
        assert_eq!(b, 1, "a new card that is missed starts in box 1");

        // Nothing is due yet — every interval is at least a day — until time passes.
        assert!(card_schedule(&pool, u).await.unwrap().iter().all(|(_, due)| !due));
        sqlx::query("UPDATE card_reviews SET due_at = datetime('now', '-1 minutes') WHERE card_id = ?")
            .bind("caching:1")
            .execute(&pool)
            .await
            .unwrap();
        let schedule = card_schedule(&pool, u).await.unwrap();
        assert_eq!(schedule[0], ("caching:1".to_string(), true), "overdue sorts first");
        assert_eq!(schedule.len(), 2);
    }

    #[tokio::test]
    async fn one_users_schedule_is_invisible_to_another() {
        let pool = connect(":memory:").await.unwrap();
        let alice = user(&pool, "alice@example.com").await;
        let bob = user(&pool, "bob@example.com").await;

        grade_card(&pool, alice, "caching:0", true).await.unwrap();
        grade_card(&pool, alice, "caching:0", true).await.unwrap();
        assert!(card_schedule(&pool, bob).await.unwrap().is_empty());

        // Bob grading the same card starts his own row rather than moving Alice's.
        let (b, _) = grade_card(&pool, bob, "caching:0", false).await.unwrap();
        assert_eq!(b, 1);
        let (b, _) = grade_card(&pool, alice, "caching:0", true).await.unwrap();
        assert_eq!(b, 4, "alice's box was untouched by bob's miss");
    }

    #[tokio::test]
    async fn critiques_are_counted_per_user() {
        let pool = connect(":memory:").await.unwrap();
        let alice = user(&pool, "alice@example.com").await;
        let bob = user(&pool, "bob@example.com").await;

        for _ in 0..3 {
            reserve_critique(&pool, alice, 100).await.unwrap().expect("within quota");
        }
        assert_eq!(critiques_in_last_hour(&pool, alice).await.unwrap(), 3);
        assert_eq!(critiques_in_last_hour(&pool, bob).await.unwrap(), 0, "quota is per user");
    }

    #[tokio::test]
    async fn only_the_author_or_a_moderator_can_remove_a_post() {
        let pool = connect(":memory:").await.unwrap();
        let author = find_user_for_login(&pool, "a@example.com").await.unwrap();
        assert!(author.is_none(), "fixture assumption: the db starts empty");

        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();
        let mod_ = User { is_admin: true, ..create_user(&pool, "m@example.com", "M", "x").await.unwrap() };

        let post = create_post(&pool, a.id, None, "Title", "Body").await.unwrap();

        // Somebody else's post is NotFound rather than Forbidden — whether it exists is
        // not the asker's business, and a 403 answers that question.
        assert!(delete_post(&pool, &b, post).await.is_err());
        assert!(get_post(&pool, None, post).await.is_ok(), "B's failed delete removed it anyway");

        delete_post(&pool, &mod_, post).await.unwrap();
        assert!(get_post(&pool, None, post).await.is_err(), "a removed post is still readable");
        assert!(list_posts(&pool, None, None).await.unwrap().is_empty());

        // Deleting twice is not a way to discover that something existed.
        assert!(delete_post(&pool, &mod_, post).await.is_err());
    }

    #[tokio::test]
    async fn a_second_vote_replaces_the_first_rather_than_adding_to_it() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();
        let post = create_post(&pool, a.id, None, "Title", "Body").await.unwrap();

        assert_eq!(set_post_vote(&pool, a.id, post, 1).await.unwrap(), 1);
        // Without the primary key upsert this would insert a second ballot and read as 2.
        assert_eq!(set_post_vote(&pool, a.id, post, 1).await.unwrap(), 1);
        assert_eq!(set_post_vote(&pool, a.id, post, -1).await.unwrap(), -1);
        assert_eq!(set_post_vote(&pool, b.id, post, 1).await.unwrap(), 0);
        assert_eq!(set_post_vote(&pool, a.id, post, 0).await.unwrap(), 1);

        let listed = list_posts(&pool, Some(b.id), None).await.unwrap();
        assert_eq!(listed[0].score, 1);
        assert_eq!(listed[0].my_vote, 1, "the viewer's own vote did not come back");
        assert_eq!(
            list_posts(&pool, None, None).await.unwrap()[0].my_vote,
            0,
            "a signed-out reader must not inherit somebody's vote",
        );
    }

    #[tokio::test]
    async fn nothing_attaches_to_a_removed_post() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let post = create_post(&pool, a.id, Some("url-shortener"), "Title", "Body").await.unwrap();

        create_comment(&pool, a.id, post, "first").await.unwrap();
        let orphan = create_comment(&pool, a.id, post, "second").await.unwrap();
        delete_comment(&pool, &a, orphan).await.unwrap();
        assert_eq!(list_comments(&pool, post).await.unwrap().len(), 1);
        assert_eq!(list_posts(&pool, None, None).await.unwrap()[0].comments, 1);

        // The foreign key proves the row exists; it says nothing about whether the post is
        // still visible, which is the check that actually matters here.
        delete_post(&pool, &a, post).await.unwrap();
        assert!(create_comment(&pool, a.id, post, "too late").await.is_err());
        assert!(set_post_vote(&pool, a.id, post, 1).await.is_err());
        assert!(create_comment(&pool, a.id, 9999, "no such post").await.is_err());
    }

    #[tokio::test]
    async fn the_scenario_filter_only_shows_that_problems_threads() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        create_post(&pool, a.id, Some("url-shortener"), "Tagged", "Body").await.unwrap();
        create_post(&pool, a.id, None, "Untagged", "Body").await.unwrap();

        assert_eq!(list_posts(&pool, None, None).await.unwrap().len(), 2, "no filter means everything");
        let tagged = list_posts(&pool, None, Some("url-shortener")).await.unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].title, "Tagged");
        assert!(list_posts(&pool, None, Some("chat-app")).await.unwrap().is_empty());
    }


    /// Helper: a match with the clock already run out. `minutes` is bounded to 5..=60 by
    /// the handler, so a negative value is only reachable from here — and it is the
    /// cheapest way to get an expired match without sleeping through one.
    async fn expired_match(pool: &SqlitePool, host: i64, guest: i64) -> i64 {
        let id = create_match(pool, host, "url-shortener", Level::Senior, -1).await.unwrap().unwrap();
        join_match(pool, guest, id).await.unwrap();
        id
    }

    #[tokio::test]
    async fn a_challenge_is_taken_once_and_not_by_its_own_host() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();
        let c = create_user(&pool, "c@example.com", "C", "x").await.unwrap();

        let id = create_match(&pool, a.id, "url-shortener", Level::Senior, 15).await.unwrap().unwrap();
        let open = get_match(&pool, id).await.unwrap();
        assert!(open.started_at.is_none(), "the clock must not start before somebody joins");
        assert!(open.deadline.is_none());
        assert_eq!(list_open_matches(&pool).await.unwrap().len(), 1);

        // Racing yourself would be a free win, and the lobby would show a match with one
        // player in both seats.
        assert!(join_match(&pool, a.id, id).await.is_err(), "the host joined their own match");

        join_match(&pool, b.id, id).await.unwrap();
        assert!(join_match(&pool, c.id, id).await.is_err(), "a taken challenge was taken twice");

        let started = get_match(&pool, id).await.unwrap();
        assert_eq!(started.guest_id, Some(b.id));
        assert!(started.deadline.is_some(), "the deadline is computed once the clock starts");
        assert!(
            list_open_matches(&pool).await.unwrap().is_empty(),
            "a match in progress is still sitting in the lobby",
        );
        assert_eq!(list_my_matches(&pool, c.id).await.unwrap().len(), 0);
        assert_eq!(list_my_matches(&pool, b.id).await.unwrap().len(), 1);

        // Once somebody is racing you, withdrawing would be a way to deny them a result.
        assert!(cancel_match(&pool, a.id, id).await.is_err());
    }

    #[tokio::test]
    async fn a_board_is_submitted_once_and_not_after_the_clock() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();
        let c = create_user(&pool, "c@example.com", "C", "x").await.unwrap();

        let id = create_match(&pool, a.id, "url-shortener", Level::Senior, 15).await.unwrap().unwrap();
        join_match(&pool, b.id, id).await.unwrap();

        let attempt = create_attempt(&pool, a.id, "url-shortener", &topo(2), "", Stage::Bottlenecks)
            .await
            .unwrap();
        assert!(submit_match_attempt(&pool, a.id, id, attempt).await.unwrap());
        // Otherwise you could keep editing while watching your opponent's clock run down.
        assert!(!submit_match_attempt(&pool, a.id, id, attempt).await.unwrap(), "submitted twice");
        assert!(!submit_match_attempt(&pool, c.id, id, attempt).await.unwrap(), "a stranger submitted");

        let m = get_match(&pool, id).await.unwrap();
        assert!(m.host_submitted && !m.guest_submitted);

        let late = expired_match(&pool, a.id, b.id).await;
        assert!(
            !submit_match_attempt(&pool, a.id, late, attempt).await.unwrap(),
            "a board landed after the deadline",
        );
    }

    #[tokio::test]
    async fn a_match_is_graded_once_even_when_both_players_look_at_once() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();

        let id = create_match(&pool, a.id, "url-shortener", Level::Senior, 15).await.unwrap().unwrap();
        join_match(&pool, b.id, id).await.unwrap();
        assert!(!claim_grading(&pool, id).await.unwrap(), "graded before anybody submitted");

        for u in [a.id, b.id] {
            let at = create_attempt(&pool, u, "url-shortener", &topo(2), "", Stage::Bottlenecks)
                .await
                .unwrap();
            submit_match_attempt(&pool, u, id, at).await.unwrap();
        }

        // Both players open the result at the same moment — the moment the clock stops.
        // Without the claim this is two model calls, two bills, and two answers to one
        // question.
        assert!(claim_grading(&pool, id).await.unwrap());
        assert!(!claim_grading(&pool, id).await.unwrap(), "the same match was claimed twice");

        // A runner outage must not leave the match stamped graded with no verdict in it.
        release_grading(&pool, id).await.unwrap();
        assert!(claim_grading(&pool, id).await.unwrap(), "a released claim could not be retried");

        finish_grading(&pool, id, "Design A covered partitioning.", "host").await.unwrap();
        release_grading(&pool, id).await.unwrap();
        assert!(!claim_grading(&pool, id).await.unwrap(), "a finished verdict was released");
    }

    #[tokio::test]
    async fn an_expired_match_is_gradable_and_a_negative_clock_is_not_null() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();
        let id = expired_match(&pool, a.id, b.id).await;

        // A leading '+' on the modifier would make this NULL, and a NULL deadline makes
        // every comparison false — the match would be unfinishable rather than expired,
        // and nobody would ever get a verdict.
        let m = get_match(&pool, id).await.unwrap();
        assert!(m.deadline.is_some(), "the deadline evaluated to NULL");
        assert!(claim_grading(&pool, id).await.unwrap(), "a match past its deadline never grades");
    }

    #[tokio::test]
    async fn the_lobby_cannot_be_filled_by_one_account() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();

        let mut ids = Vec::new();
        for _ in 0..OPEN_MATCH_LIMIT {
            ids.push(
                create_match(&pool, a.id, "url-shortener", Level::Senior, 15)
                    .await
                    .unwrap()
                    .expect("under the limit"),
            );
        }
        assert!(
            create_match(&pool, a.id, "url-shortener", Level::Senior, 15).await.unwrap().is_none(),
            "one account filled the lobby past the limit",
        );
        assert!(
            create_match(&pool, b.id, "url-shortener", Level::Senior, 15).await.unwrap().is_some(),
            "one account's limit blocked another's",
        );

        // The cap is on challenges *waiting*, so clearing one frees the slot.
        cancel_match(&pool, a.id, ids[0]).await.unwrap();
        assert!(create_match(&pool, a.id, "url-shortener", Level::Senior, 15).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn the_match_view_never_carries_either_board() {
        let pool = connect(":memory:").await.unwrap();
        let a = create_user(&pool, "a@example.com", "A", "x").await.unwrap();
        let b = create_user(&pool, "b@example.com", "B", "x").await.unwrap();
        let id = create_match(&pool, a.id, "url-shortener", Level::Senior, 15).await.unwrap().unwrap();
        join_match(&pool, b.id, id).await.unwrap();

        let attempt = create_attempt(&pool, a.id, "url-shortener", &topo(3), "my secret plan", Stage::Bottlenecks)
            .await
            .unwrap();
        submit_match_attempt(&pool, a.id, id, attempt).await.unwrap();

        // The whole exercise is that neither player can see the other's design until the
        // clock stops. An attempt id in this payload is enough to go and read one.
        let json = serde_json::to_string(&get_match(&pool, id).await.unwrap()).unwrap();
        assert!(!json.contains("ttemptId"), "an attempt id reached the wire: {json}");
        assert!(!json.contains("secret plan"), "a board's notes reached the wire: {json}");
        assert!(json.contains("\"hostSubmitted\":true"), "the status itself must still show");
    }


    #[tokio::test]
    async fn an_ml_problem_is_solved_at_its_own_last_stage() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        // The other half of `each_track_is_solved_at_its_own_finish_line`, which could
        // only assert the negative until there were ML scenarios to finish.
        create_attempt(&pool, u, "spam-classifier", &topo(1), "", Stage::EvaluationAndMonitoring)
            .await
            .unwrap();
        create_attempt(&pool, u, "llm-serving", &topo(1), "", Stage::Bottlenecks).await.unwrap();

        let progress = list_progress(&pool, u).await.unwrap();
        let solved = |id: &str| progress.iter().find(|p| p.scenario_id == id).unwrap().solved;

        assert!(solved("spam-classifier"), "an ML problem is not solved by its own last stage");
        assert!(
            !solved("llm-serving"),
            "a classic terminal stage solved an ML problem",
        );
    }

    #[test]
    fn every_stage_tag_is_pinned() {
        // There is no test runner on the frontend, so `STAGES_BY_TRACK` in types.ts is
        // mirrored by hand against nothing. This is the authoritative list to copy from,
        // and a Rust-side rename fails here rather than silently in the browser.
        let expect = [
            (Stage::Requirements, "requirements"),
            (Stage::Estimation, "estimation"),
            (Stage::HighLevel, "highLevel"),
            (Stage::DeepDive, "deepDive"),
            (Stage::Bottlenecks, "bottlenecks"),
            (Stage::DataAndFeatures, "dataAndFeatures"),
            (Stage::Modeling, "modeling"),
            (Stage::Serving, "serving"),
            (Stage::EvaluationAndMonitoring, "evaluationAndMonitoring"),
        ];
        for (stage, tag) in expect {
            assert_eq!(stage_tag(stage), tag, "{stage:?} changed its wire format");
        }

        // Every stage any track walks must appear above, or the list is not authoritative.
        let known: Vec<&str> = expect.iter().map(|(_, t)| *t).collect();
        for track in Track::ALL {
            for stage in track.stages() {
                assert!(known.contains(&stage_tag(*stage).as_str()), "{stage:?} is unpinned");
            }
        }
    }

    #[tokio::test]
    async fn each_track_is_solved_at_its_own_finish_line() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;

        // Both of these are classic problems, so only `bottlenecks` finishes either. The
        // ML half of this — that `evaluationAndMonitoring` finishes an ML problem — is
        // asserted in `an_ml_problem_is_solved_at_its_own_last_stage` once ML scenarios
        // exist; here the point is that the other track's terminal stage must not count.
        create_attempt(&pool, u, "url-shortener", &topo(1), "", Stage::Bottlenecks).await.unwrap();
        create_attempt(&pool, u, "chat-app", &topo(1), "", Stage::EvaluationAndMonitoring)
            .await
            .unwrap();

        let progress = list_progress(&pool, u).await.unwrap();
        let solved = |id: &str| progress.iter().find(|p| p.scenario_id == id).unwrap().solved;

        assert!(solved("url-shortener"), "a classic attempt at bottlenecks is not solved");
        // The point of the per-scenario lookup: an ML terminal stage is only reachable on
        // a classic problem by a client that went around `check_stage`, and it must not be
        // rewarded with a tick. Completion cannot depend on a handler having been careful.
        assert!(
            !solved("chat-app"),
            "another track's terminal stage solved a classic problem",
        );
    }

    #[tokio::test]
    async fn a_scenario_that_no_longer_exists_does_not_break_progress() {
        let pool = connect(":memory:").await.unwrap();
        let u = user(&pool, "a@example.com").await;
        create_attempt(&pool, u, "url-shortener", &topo(1), "", Stage::Bottlenecks).await.unwrap();

        // Rows predating the `scenarios::find` check in `create_attempt` can hold any id,
        // including one that would be invalid inside a JSON path. Progress has to keep
        // answering for everything else rather than 500 the whole page.
        sqlx::query("INSERT INTO attempts (user_id, scenario_id, topology, notes, stage) VALUES (?, ?, '{}', '', 'bottlenecks')")
            .bind(u)
            .bind(r#"ghost\"#)
            .execute(&pool)
            .await
            .unwrap();

        let progress = list_progress(&pool, u).await.expect("a stray row must not fail the query");
        let find = |id: &str| progress.iter().find(|p| p.scenario_id == id);
        assert!(find("url-shortener").unwrap().solved, "the real scenario stopped being solved");
        assert!(!find(r#"ghost\"#).unwrap().solved, "an unknown scenario read as solved");
    }

}
