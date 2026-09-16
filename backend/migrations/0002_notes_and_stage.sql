-- Stages 1 and 2 of the interview (requirements, estimation) are answered in prose, not
-- in boxes and arrows, so a session needs somewhere to hold that text. `stage` is where
-- the candidate had got to, so reopening a session resumes the interview.
ALTER TABLE sessions ADD COLUMN notes TEXT NOT NULL DEFAULT '';
ALTER TABLE sessions ADD COLUMN stage TEXT NOT NULL DEFAULT 'requirements';
