-- SQLite
-- Custom task fields (issue #87). Postgres: postgres/20261124132943_task_custom_fields.sql.
--
-- `task_fields` is the workspace's field gallery. A project shows the fields of
-- `project_task_fields`, in `position` order. A task keeps one JSON value per field in
-- `task_field_values`; the server checks the value against the field's kind. A value stays when
-- its task moves to a project without the field, and shows again when the field is added there.

CREATE TABLE task_fields (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('text', 'number', 'date', 'select', 'multi_select', 'checkbox', 'url')),
    -- select and multi_select: [{"id", "name", "color"}]; other kinds: []
    options_json TEXT NOT NULL DEFAULT '[]',
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX task_fields_workspace_name ON task_fields (workspace_id, lower(name));

CREATE TABLE project_task_fields (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    field_id TEXT NOT NULL REFERENCES task_fields(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (project_id, field_id)
);

CREATE INDEX project_task_fields_field ON project_task_fields (field_id);

CREATE TABLE task_field_values (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    field_id TEXT NOT NULL REFERENCES task_fields(id) ON DELETE CASCADE,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (task_id, field_id)
);

CREATE INDEX task_field_values_field ON task_field_values (field_id);
