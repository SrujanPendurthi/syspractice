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
}

pub async fn list_progress(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<ScenarioProgress>, AppError> {
    // `stage = 'bottlenecks'` is 0 or 1 in SQLite, so MAX over the group is "any attempt
    // got there". The literal has to match the serde tag that `stage_tag` writes.
    let rows = sqlx::query_as::<_, ScenarioProgress>(
        "SELECT scenario_id,
                MAX(stage = 'bottlenecks') AS solved,
                COUNT(*)                   AS attempts,
                MAX(updated_at)            AS last_seen
         FROM attempts
         WHERE user_id = ?
         GROUP BY scenario_id",
    )
    .bind(user_id)
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
    let row: Option<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, email, display_name, password_hash FROM users WHERE email_folded = ?",
    )
    .bind(fold_email(email))
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, email, display_name, hash)| {
        (User { id, email, display_name }, hash)
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
        "SELECT u.id, u.email, u.display_name
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
