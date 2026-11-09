-- People named as assignees by an Asana, Linear or Trello import who had no Orbit member with the
-- same email (Trello: the same full name). Their tasks were left unassigned and name them in the
-- description. An admin maps a person to any member in Settings → Import, which assigns all
-- their tasks; a member who joins later with the person's email is mapped on join.
-- `name_key` is lower(trim(name)); `email` is lowercase, NULL when the source did not give one.
CREATE TABLE import_people (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('asana', 'linear', 'trello')),
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    email TEXT,
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE (workspace_id, source, name_key)
);
CREATE INDEX import_people_email ON import_people (workspace_id, email);
-- For the SET NULL when a user is deleted.
CREATE INDEX import_people_user ON import_people (user_id);

-- The imported tasks each person was the source assignee of.
CREATE TABLE import_person_tasks (
    person_id TEXT NOT NULL REFERENCES import_people(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (person_id, task_id)
);
CREATE INDEX import_person_tasks_task ON import_person_tasks (task_id);

-- Imports made before this table existed: a pass at server start reads the people from their
-- task descriptions, then deletes the row. Imports still running are queued too, for the tasks
-- they created before the upgrade. New imports record their people themselves.
CREATE TABLE import_people_backfill (
    import_id TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('asana', 'linear', 'trello')),
    PRIMARY KEY (source, import_id)
);
INSERT INTO import_people_backfill (import_id, source)
SELECT id, 'asana' FROM asana_imports
UNION ALL SELECT id, 'linear' FROM linear_imports
UNION ALL SELECT id, 'trello' FROM trello_imports;
