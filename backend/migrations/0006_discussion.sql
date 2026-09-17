-- The discussion tab: posts, flat comments, votes on posts.
--
-- Removal is `deleted_at`, not DELETE. A moderator taking down spam should leave the row
-- that proves who posted it, and a hard delete of a post takes its comment thread with it
-- via ON DELETE CASCADE. Every read filters on `deleted_at IS NULL`.

-- Set by hand in SQL. There is no admin UI, and an endpoint that grants this is a much
-- larger thing to get right than the one query that reads it.
ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0;

CREATE TABLE posts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Optional, and deliberately not a foreign key: scenarios are static data in
    -- `scenarios.rs`, not a table. Handlers validate it against that list instead.
    scenario_id TEXT,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    deleted_at  TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_posts_scenario ON posts (scenario_id, created_at);
CREATE INDEX idx_posts_created ON posts (created_at);

CREATE TABLE comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id    INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body       TEXT NOT NULL,
    deleted_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_comments_post ON comments (post_id, created_at);

-- The primary key is what makes a second vote an edit rather than a second ballot.
-- Score is summed from here on read; there is no counter column to drift.
CREATE TABLE post_votes (
    post_id INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    value   INTEGER NOT NULL CHECK (value IN (-1, 1)),
    PRIMARY KEY (post_id, user_id)
);
