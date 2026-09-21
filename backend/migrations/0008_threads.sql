-- Threaded comments: a comment may answer another comment on the same post.
--
-- NULL is a top-level comment. The foreign key only proves the parent row exists; that it
-- is visible and on the same post is checked by the conditional INSERT in `create_comment`,
-- because a reference cannot express either. Deleting a parent is a `deleted_at` stamp like
-- everything else here, so the CASCADE only ever fires on a hard delete of the whole post.
ALTER TABLE comments ADD COLUMN parent_id INTEGER REFERENCES comments(id) ON DELETE CASCADE;
