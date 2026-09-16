CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    -- `email` keeps the form the user typed; `email_folded` is the uniqueness key, so
    -- signing up twice with different capitalisation is rejected. A UNIQUE COLLATE NOCASE
    -- index would have done it for ASCII only, and silently not for anything else.
    email         TEXT NOT NULL,
    email_folded  TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    -- argon2id PHC string. Carries its own salt and parameters, so a future parameter
    -- bump can be detected per row rather than forcing a global reset.
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- One row per logged-in browser. The column holds SHA-256 of the cookie value, never the
-- value itself: a leaked database backup is then not a pile of live sessions.
CREATE TABLE auth_sessions (
    token_hash  BLOB PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at  TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_auth_sessions_user ON auth_sessions (user_id);

-- One row per critique served. This is the quota window; it is also the only record of
-- what the GPU was spent on.
CREATE TABLE critique_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_critique_log_user_time ON critique_log (user_id, created_at);

-- Nullable so this migration applies to a local database that predates accounts. Every
-- query filters on user_id, so those rows become invisible rather than being handed to
-- whoever registers first.
ALTER TABLE attempts ADD COLUMN user_id INTEGER REFERENCES users(id);
CREATE INDEX idx_attempts_user ON attempts (user_id);
