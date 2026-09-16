//! Persistence. Pure data access — no HTTP types cross this boundary.
//!
//! ponytail: runtime-checked queries (`query_as`) rather than the compile-time `query!` macros,
//! so a build never needs a live DATABASE_URL or a checked-in `.sqlx` cache. Switch if the
//! query set grows enough that typos stop being caught by the tests.

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};

use crate::error::AppError;
use crate::topology::Topology;

#[derive(Debug, FromRow)]
struct SessionRow {
    id: i64,
    scenario_id: String,
    topology: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    pub scenario_id: String,
    pub topology: Topology,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<SessionRow> for Session {
    type Error = serde_json::Error;

    fn try_from(r: SessionRow) -> Result<Self, Self::Error> {
        Ok(Session {
            id: r.id,
            scenario_id: r.scenario_id,
            topology: serde_json::from_str(&r.topology)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

/// Row shape for the session list — the full topology is not sent until a session is opened.
#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: i64,
    pub scenario_id: String,
    pub node_count: i64,
    pub updated_at: String,
}

pub async fn connect(path: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        // WAL keeps reads from blocking behind the write that saves a session.
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    // Each pooled connection to ":memory:" would get its own private database, so tests
    // must run against a single connection.
    let max = if path == ":memory:" { 1 } else { 4 };

    let pool = SqlitePoolOptions::new().max_connections(max).connect_with(opts).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

pub async fn create_session(
    pool: &SqlitePool,
    scenario_id: &str,
    topology: &Topology,
) -> Result<i64, AppError> {
    let json = serde_json::to_string(topology).expect("Topology always serializes");
    let row: (i64,) = sqlx::query_as("INSERT INTO sessions (scenario_id, topology) VALUES (?, ?) RETURNING id")
        .bind(scenario_id)
        .bind(json)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn update_session(
    pool: &SqlitePool,
    id: i64,
    topology: &Topology,
) -> Result<(), AppError> {
    let json = serde_json::to_string(topology).expect("Topology always serializes");
    let res = sqlx::query("UPDATE sessions SET topology = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(json)
        .bind(id)
        .execute(pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn get_session(pool: &SqlitePool, id: i64) -> Result<Session, AppError> {
    let row: Option<SessionRow> = sqlx::query_as("SELECT * FROM sessions WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    let row = row.ok_or(AppError::NotFound)?;
    // A row whose topology JSON no longer parses is a schema-drift bug, not a missing row.
    Session::try_from(row).map_err(|e| AppError::Db(sqlx::Error::Decode(Box::new(e))))
}

pub async fn list_sessions(pool: &SqlitePool) -> Result<Vec<SessionSummary>, AppError> {
    let rows = sqlx::query_as::<_, SessionSummary>(
        "SELECT id, scenario_id, json_array_length(topology, '$.nodes') AS node_count, updated_at
         FROM sessions ORDER BY updated_at DESC LIMIT 100",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{Node, NodeKind};

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

        let id = create_session(&pool, "url-shortener", &topo(2)).await.unwrap();
        let got = get_session(&pool, id).await.unwrap();
        assert_eq!(got.topology.nodes.len(), 2);
        assert_eq!(got.scenario_id, "url-shortener");

        update_session(&pool, id, &topo(5)).await.unwrap();
        assert_eq!(get_session(&pool, id).await.unwrap().topology.nodes.len(), 5);

        let list = list_sessions(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].node_count, 5, "node_count must track the saved topology");
    }

    #[tokio::test]
    async fn missing_rows_are_not_found() {
        let pool = connect(":memory:").await.unwrap();
        assert!(matches!(get_session(&pool, 999).await, Err(AppError::NotFound)));
        assert!(matches!(update_session(&pool, 999, &topo(1)).await, Err(AppError::NotFound)));
    }
}
