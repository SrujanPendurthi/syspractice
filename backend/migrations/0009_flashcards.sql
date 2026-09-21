-- One row per card a user has graded at least once: its Leitner box and when it is next
-- due. A card with no row is new. The cards themselves are static — derived from the
-- probes in `concepts.rs` — so `card_id` references nothing, the same as `concept_reads`.
CREATE TABLE card_reviews (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    card_id TEXT NOT NULL,
    box     INTEGER NOT NULL,
    due_at  TEXT NOT NULL,
    PRIMARY KEY (user_id, card_id)
);
