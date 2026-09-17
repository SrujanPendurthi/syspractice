//! Passwords, session tokens, and the extractor that turns a cookie into a user.
//!
//! The SQL all lives in `db.rs`; this module is the crypto and the HTTP edge of it.
//!
//! Two properties are worth stating because they are easy to quietly lose in a refactor:
//! the plaintext session token exists only in the response that creates it (the database
//! holds SHA-256 of it), and password hashing runs on a blocking thread because argon2 is
//! deliberately slow and would otherwise stall the async runtime under any concurrency.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;

use argon2::password_hash::rand_core::OsRng as SaltRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::db::{self, User};
use crate::error::AppError;
use crate::routes::AppState;

pub const COOKIE: &str = "syspractice_session";
const SESSION_DAYS: i64 = 30;

/// Long enough that a passphrase is the natural answer, with no composition rules —
/// those push people towards `Password1!` and a sticky note.
const MIN_PASSWORD: usize = 10;
/// argon2 hashes whatever it is given, so an unbounded password is a free CPU burn.
const MAX_PASSWORD: usize = 256;

// --- passwords ---------------------------------------------------------------

/// argon2id at the crate defaults, salted per user. Runs on a blocking thread: the whole
/// point of the algorithm is to take tens of milliseconds of CPU, which is exactly what
/// must never happen on an async worker.
pub async fn hash_password(password: String) -> Result<String, AppError> {
    blocking(move || {
        let salt = SaltString::generate(&mut SaltRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|_| AppError::Invalid("could not hash that password"))
    })
    .await
}

pub async fn verify_password(password: String, hash: String) -> Result<bool, AppError> {
    blocking(move || {
        let parsed = match PasswordHash::new(&hash) {
            Ok(p) => p,
            // A row we cannot parse is a broken hash, not a correct password.
            Err(_) => return Ok(false),
        };
        Ok(Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    })
    .await
}

/// Burns the same argon2 time as a real check, for the path where the email matched no
/// account. Without it the two outcomes are distinguishable by a stopwatch, which gives
/// back exactly the account-existence oracle that `BadCredentials` exists to deny.
pub async fn dummy_verify(password: String) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = match DUMMY.get() {
        Some(h) => h.clone(),
        None => {
            let h = hash_password("a password nobody has".into()).await.unwrap_or_default();
            DUMMY.get_or_init(|| h).clone()
        }
    };
    let _ = verify_password(password, hash).await;
}

async fn blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| AppError::Invalid("password hashing failed"))?
}

pub fn check_password_rules(password: &str) -> Result<(), AppError> {
    if password.len() < MIN_PASSWORD {
        return Err(AppError::Invalid("password must be at least 10 characters"));
    }
    if password.len() > MAX_PASSWORD {
        return Err(AppError::Invalid("password must be under 256 characters"));
    }
    Ok(())
}

/// Deliberately permissive. The only address that matters is one the user can actually
/// receive at, and no pattern here can tell you that — a stricter rule just rejects valid
/// addresses that happen to look unusual.
pub fn check_email(email: &str) -> Result<(), AppError> {
    let e = email.trim();
    let bad = e.len() < 3 || e.len() > 254 || !e.contains('@') || e.starts_with('@')
        || e.ends_with('@')
        || e.contains(char::is_whitespace);
    if bad {
        return Err(AppError::Invalid("that does not look like an email address"));
    }
    Ok(())
}

// --- session tokens ----------------------------------------------------------

/// Returns the plaintext token for the cookie. Only its hash reaches the database, so a
/// leaked backup cannot be replayed as a pile of live logins.
pub async fn issue_session(state: &AppState, user_id: i64) -> Result<String, AppError> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);

    db::store_auth_session(&state.pool, &token_hash(&token), user_id, SESSION_DAYS).await?;
    Ok(token)
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn set_cookie(jar: CookieJar, token: String, secure: bool) -> CookieJar {
    jar.add(
        Cookie::build((COOKIE, token))
            .http_only(true)
            .secure(secure)
            // Lax, not Strict: following a link into a problem from the discussion tab
            // should not land the user on a page that says they are signed out. Every
            // mutating endpoint is JSON-only, which no cross-site form can produce.
            .same_site(SameSite::Lax)
            .path("/")
            .max_age(time::Duration::days(SESSION_DAYS))
            .build(),
    )
}

pub fn clear_cookie(jar: CookieJar, secure: bool) -> CookieJar {
    // Removal has to match the original path and flags or the browser keeps the old one.
    jar.remove(Cookie::build((COOKIE, "")).path("/").secure(secure).build())
}

// --- extractors --------------------------------------------------------------

/// A signed-in user. Rejects with 401, so putting this in a handler's arguments is the
/// whole of "this endpoint requires an account".
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        MaybeUser::from_request_parts(parts, state)
            .await?
            .0
            .map(AuthUser)
            .ok_or(AppError::Unauthorized)
    }
}

/// For the endpoints an anonymous visitor is allowed to reach.
pub struct MaybeUser(pub Option<User>);

impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized)?;

        let Some(token) = jar.get(COOKIE).map(|c| c.value().to_string()) else {
            return Ok(MaybeUser(None));
        };
        // Expiry is checked in the SQL, so a stale row can never read as a live login.
        Ok(MaybeUser(db::user_for_token(&state.pool, &token_hash(&token)).await?))
    }
}

// --- throttling ---------------------------------------------------------------

pub const LOGIN_MAX: u32 = 5;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);

/// Registration is rate-limited by address rather than by email, because the thing being
/// abused is the ability to ask "does this account exist?" over and over. Five an hour is
/// far more than a real person needs and far less than an enumeration sweep wants.
pub const REGISTER_MAX: u32 = 5;
pub const REGISTER_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Posting is limited per account rather than per address: an account costs a
/// registration, which is itself limited, so it is the more expensive identity to churn.
/// The numbers are what a person discussing a problem needs, not what a spammer does.
pub const POST_MAX: u32 = 5;
pub const POST_WINDOW: Duration = Duration::from_secs(60 * 60);
pub const COMMENT_MAX: u32 = 30;
pub const COMMENT_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Above this many tracked keys, expired entries are swept on the next write. Without it
/// a spray across distinct keys grows the map forever.
const SWEEP_AT: usize = 4096;

/// A fixed-window counter keyed by whatever the caller considers an identity.
///
/// ponytail: in-process, so it resets on deploy and does not survive a second server.
/// Move it into a table when there is a second server — not before.
pub struct Throttle {
    inner: Mutex<HashMap<String, (u32, Instant)>>,
    max: u32,
    window: Duration,
    message: &'static str,
}

impl Throttle {
    pub fn new(max: u32, window: Duration, message: &'static str) -> Self {
        Self { inner: Mutex::new(HashMap::new()), max, window, message }
    }

    /// Errors once a key has been used too often. Called before any expensive work, so a
    /// throttled request costs no argon2 time either.
    pub fn check(&self, key: &str) -> Result<(), AppError> {
        let mut map = self.lock();
        match map.get(key) {
            Some((n, since)) if *n >= self.max && since.elapsed() < self.window => {
                Err(AppError::Throttled(self.message))
            }
            // The window has rolled over; forget the old count rather than let it linger.
            Some((_, since)) if since.elapsed() >= self.window => {
                map.remove(key);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn record(&self, key: &str) {
        let mut map = self.lock();
        if map.len() > SWEEP_AT {
            let window = self.window;
            map.retain(|_, (_, since)| since.elapsed() < window);
        }
        let entry = map.entry(key.to_string()).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= self.window {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    pub fn clear(&self, key: &str) {
        self.lock().remove(key);
    }

    /// A poisoned mutex here would mean a panic while holding it. Recovering the map is
    /// strictly better than taking the whole server down over a rate-limit counter.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (u32, Instant)>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The address to throttle a request against.
///
/// Behind a reverse proxy every connection arrives from the proxy, so the socket address
/// would throttle the whole internet as one client. `X-Forwarded-For` fixes that, but it
/// is caller-supplied and trivially forged when nothing strips it — so it is only honoured
/// when `SYSPRACTICE_TRUST_PROXY` says a proxy is actually in front.
pub fn client_key(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(first) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return first.to_string();
        }
    }
    peer.ip().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_password_verifies_against_its_own_hash_and_nothing_else() {
        let hash = hash_password("correct horse battery".into()).await.unwrap();

        assert!(hash.starts_with("$argon2id$"), "expected argon2id, got {hash}");
        assert!(!hash.contains("correct horse"), "the password itself must not be in the hash");
        assert!(verify_password("correct horse battery".into(), hash.clone()).await.unwrap());
        assert!(!verify_password("correct horse batteru".into(), hash).await.unwrap());
    }

    #[tokio::test]
    async fn the_same_password_hashes_differently_every_time() {
        // Per-user salt. Without it, identical passwords are visibly identical rows and
        // one cracked hash cracks every account that shares it.
        let a = hash_password("correct horse battery".into()).await.unwrap();
        let b = hash_password("correct horse battery".into()).await.unwrap();
        assert_ne!(a, b, "two hashes of one password matched — the salt is not being used");
    }

    #[tokio::test]
    async fn a_corrupt_hash_rejects_rather_than_erroring() {
        assert!(!verify_password("anything".into(), "not-a-phc-string".into()).await.unwrap());
    }

    #[test]
    fn the_token_is_not_recoverable_from_what_is_stored() {
        let token = "AAAA-a-token-BBBB";
        let stored = token_hash(token);
        assert_eq!(stored.len(), 32);
        assert_ne!(stored, token.as_bytes(), "the token was stored verbatim");
        assert_eq!(stored, token_hash(token), "hashing must be stable or logins break");
    }

    #[test]
    fn a_forwarded_address_is_only_believed_behind_a_proxy() {
        let peer: SocketAddr = "10.0.0.9:5000".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.7, 10.0.0.1".parse().unwrap());

        // Forged by anyone who can reach the socket, so it must be ignored by default —
        // otherwise a single client varies one header and defeats the rate limit entirely.
        assert_eq!(client_key(&headers, peer, false), "10.0.0.9");
        assert_eq!(client_key(&headers, peer, true), "203.0.113.7");
        assert_eq!(client_key(&HeaderMap::new(), peer, true), "10.0.0.9");
    }

    #[test]
    fn passwords_and_emails_are_checked_before_anything_expensive() {
        assert!(check_password_rules("short").is_err());
        assert!(check_password_rules("long enough to pass").is_ok());
        assert!(check_password_rules(&"x".repeat(1000)).is_err());

        assert!(check_email("a@b.co").is_ok());
        assert!(check_email("no-at-sign").is_err());
        assert!(check_email("has space@b.co").is_err());
        assert!(check_email("@b.co").is_err());
    }

    #[test]
    fn the_throttle_stops_an_account_being_guessed_at() {
        let t = Throttle::new(LOGIN_MAX, LOGIN_WINDOW, "too many attempts");
        for _ in 0..LOGIN_MAX {
            assert!(t.check("a@example.com").is_ok());
            t.record("a@example.com");
        }
        assert!(t.check("a@example.com").is_err(), "the sixth attempt should be refused");
        assert!(t.check("b@example.com").is_ok(), "one account must not throttle another");

        // A successful sign-in clears the count, so a user who finally remembers their
        // password is not locked out by their own earlier typos.
        t.clear("a@example.com");
        assert!(t.check("a@example.com").is_ok());
    }
}
