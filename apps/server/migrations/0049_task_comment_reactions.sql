-- Emoji reactions on task comments: a Unicode emoji, or `:name:` for a custom emoji of the
-- workspace (as chat stores them). One row for each (comment, person, emoji).
CREATE TABLE task_comment_reactions (
    comment_id TEXT NOT NULL REFERENCES task_comments(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji TEXT NOT NULL CHECK (length(emoji) BETWEEN 1 AND 64),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (comment_id, user_id, emoji)
);

-- For the cascade when a user is deleted.
CREATE INDEX task_comment_reactions_user ON task_comment_reactions (user_id);
