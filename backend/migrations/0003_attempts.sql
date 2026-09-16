-- "session" is about to mean "a logged-in browser", which is what every auth crate and
-- doc calls it. This table means "one practice attempt at a scenario". Renaming now costs
-- four files and no data; leaving it costs the same confusion that `difficulty` vs `level`
-- already caused once.
ALTER TABLE sessions RENAME TO attempts;

-- RENAME TO carries indexes across under their old names.
DROP INDEX IF EXISTS idx_sessions_scenario;
CREATE INDEX idx_attempts_scenario ON attempts (scenario_id);
