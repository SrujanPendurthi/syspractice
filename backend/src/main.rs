mod auth;
mod concepts;
mod critic;
mod db;
mod error;
mod routes;
mod scenarios;
mod topology;

use std::net::SocketAddr;
use std::sync::Arc;

use tower_http::services::{ServeDir, ServeFile};

use routes::AppState;

const DEFAULT_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_RUNNER: &str = "http://127.0.0.1:8080";
const DEFAULT_QUOTA: i64 = 60;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = env_or("SYSPRACTICE_DB", "syspractice.db");
    let runner_url = env_or("LLAMA_SERVER_URL", DEFAULT_RUNNER);
    let addr = env_or("SYSPRACTICE_ADDR", DEFAULT_ADDR);

    // llama-server ignores the model name; Ollama requires a real one (e.g. "gemma4:12b").
    let model = env_or("SYSPRACTICE_MODEL", "local");

    // Safe by default. `./dev` turns it off because Safari will not store a Secure cookie
    // from http://localhost, which would leave every dev sign-in silently failing.
    let secure_cookies = env_or("SYSPRACTICE_SECURE_COOKIES", "1") != "0";

    // Off by default: `X-Forwarded-For` is caller-supplied, so believing it without a
    // proxy in front lets one client vary a header and evade every rate limit.
    let trust_proxy = env_or("SYSPRACTICE_TRUST_PROXY", "0") == "1";

    let critique_quota =
        env_or("SYSPRACTICE_CRITIQUE_QUOTA", &DEFAULT_QUOTA.to_string()).parse().unwrap_or(DEFAULT_QUOTA);

    let pool = db::connect(&db_path).await?;
    let app = routes::router(AppState {
        pool,
        runner_url: runner_url.clone(),
        model: model.clone(),
        login_throttle: Arc::new(auth::Throttle::new(
            auth::LOGIN_MAX,
            auth::LOGIN_WINDOW,
            "too many sign-in attempts — try again in 15 minutes",
        )),
        register_throttle: Arc::new(auth::Throttle::new(
            auth::REGISTER_MAX,
            auth::REGISTER_WINDOW,
            "too many accounts created from here — try again later",
        )),
        post_throttle: Arc::new(auth::Throttle::new(
            auth::POST_MAX,
            auth::POST_WINDOW,
            "you have posted enough for one hour — give the thread a chance to answer",
        )),
        comment_throttle: Arc::new(auth::Throttle::new(
            auth::COMMENT_MAX,
            auth::COMMENT_WINDOW,
            "you have commented enough for one hour",
        )),
        trust_proxy,
        secure_cookies,
        critique_quota,
    });

    // Serving the built frontend from here is what keeps this a single origin: no CORS
    // layer, and the SPA fallback that `useRoute` needs for paths like /s/url-shortener.
    // In development Vite serves the app instead and proxies /api back to this process,
    // so a missing dist/ is normal rather than an error.
    let dist = env_or("SYSPRACTICE_DIST", "../frontend/dist");
    let app = app.fallback_service(
        ServeDir::new(&dist).fallback(ServeFile::new(format!("{dist}/index.html"))),
    );

    // Loopback by default. The critique endpoint proxies to a model runner that has no
    // authentication of its own, so when this binds a public address — which hosting
    // requires — the runner must stay on loopback behind it.
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("syspractice backend on http://{addr}");
    println!("  db     {db_path}");
    println!("  model  {model} @ {runner_url}");
    println!("  quota  {critique_quota} critiques/user/hour");
    if !secure_cookies {
        println!("  !!     cookies are not marked Secure (SYSPRACTICE_SECURE_COOKIES=0)");
    }

    // `into_make_service_with_connect_info` is what makes the peer address available to
    // the registration rate limit; a plain `app` would panic in that extractor.
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
