mod critic;
mod db;
mod error;
mod routes;
mod scenarios;
mod topology;

use routes::AppState;

const DEFAULT_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_RUNNER: &str = "http://127.0.0.1:8080";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = env_or("SYSPRACTICE_DB", "syspractice.db");
    let runner_url = env_or("LLAMA_SERVER_URL", DEFAULT_RUNNER);
    let addr = env_or("SYSPRACTICE_ADDR", DEFAULT_ADDR);

    // llama-server ignores the model name; Ollama requires a real one (e.g. "gemma4:12b").
    let model = env_or("SYSPRACTICE_MODEL", "local");

    let pool = db::connect(&db_path).await?;
    let app = routes::router(AppState {
        pool,
        runner_url: runner_url.clone(),
        model: model.clone(),
    });

    // Bound to loopback on purpose: this is a local-first app and the critique endpoint
    // proxies to an unauthenticated model runner.
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("syspractice backend on http://{addr}");
    println!("  db     {db_path}");
    println!("  model  {model} @ {runner_url}");

    axum::serve(listener, app).await?;
    Ok(())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
