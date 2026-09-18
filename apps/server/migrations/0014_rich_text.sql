-- Rich text documents replace the Markdown strings in tasks.description and
-- task_comments.body. The JSON column is the source of truth; the _text column
-- is derived by the server on every write and feeds list rows and search.
ALTER TABLE tasks ADD COLUMN description_json TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN description_text TEXT NOT NULL DEFAULT '';
ALTER TABLE task_comments ADD COLUMN body_json TEXT NOT NULL DEFAULT '';
ALTER TABLE task_comments ADD COLUMN body_text TEXT NOT NULL DEFAULT '';

-- One row per task. workspace_id is INDEXED (not UNINDEXED) so every query ANDs
-- it into the MATCH and the index never scans across workspaces. It stores the
-- hyphen-stripped UUID because unicode61 splits on hyphens.
CREATE VIRTUAL TABLE task_search USING fts5(
    task_id UNINDEXED,
    workspace_id,
    identifier,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);
