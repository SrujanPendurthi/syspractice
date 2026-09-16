-- Which concepts a user has marked as read. A join table rather than a column on `users`
-- so adding a concept needs no migration, and so "when" is available for free.
CREATE TABLE concept_reads (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    concept_id TEXT NOT NULL,
    read_at    TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, concept_id)
);
