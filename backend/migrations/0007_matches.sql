-- 1v1 head-to-head. One scenario, one level, one clock, two ordinary attempts.
--
-- There is no `deadline` column: it is `datetime(started_at, '+N minutes')`, and storing
-- it too would be a second copy of the same fact that a changed `minutes` could contradict.
-- The clock starts when the guest joins, not when the match is created, so an open
-- challenge sitting in the lobby does not burn its own time.

CREATE TABLE matches (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Static data again, so not a foreign key. Validated in the handler like everywhere else.
    scenario_id TEXT NOT NULL,
    -- One level for both sides: the rubric has to be filtered identically or the two
    -- designs are not being graded against the same thing.
    level       TEXT NOT NULL,
    minutes     INTEGER NOT NULL,

    host_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- NULL until somebody joins. That is what "open" means; there is no status column,
    -- because a status column and these timestamps can disagree and then one of them is lying.
    guest_id         INTEGER REFERENCES users(id) ON DELETE CASCADE,
    host_attempt_id  INTEGER REFERENCES attempts(id),
    guest_attempt_id INTEGER REFERENCES attempts(id),

    started_at   TEXT,
    -- Claimed before the model call, not written after it. Two people opening the result
    -- at the same moment would otherwise both pay for and run the same grading.
    graded_at    TEXT,
    verdict      TEXT,
    winner       TEXT,
    cancelled_at TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_matches_lobby ON matches (started_at, cancelled_at, created_at);
CREATE INDEX idx_matches_host  ON matches (host_id);
CREATE INDEX idx_matches_guest ON matches (guest_id);
