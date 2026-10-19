-- Labels belong to one project, like statuses (SQLite: ../0051_project_labels.sql).
--
-- Every workspace label is copied into every project of its workspace (soft-deleted projects
-- too). The copy in the workspace's oldest live project keeps the label id; the others get new ids.
-- Each task_labels row moves to the copy in the task's own project. Saved views keep the old
-- ids: label_name_aliases keeps each old id's name, and label filters read such an id as that
-- name, so they find every copy even after the copy that kept the id is renamed or deleted.

CREATE TABLE label_name_aliases (
    label_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL
);

INSERT INTO label_name_aliases (label_id, workspace_id, name)
SELECT id, workspace_id, name FROM labels;

-- Ids are canonical lowercase UUIDv7 from the current time, as in SQLite.
CREATE TEMPORARY TABLE label_copies ON COMMIT DROP AS
SELECT
    seed.old_id,
    seed.project_id,
    CASE WHEN seed.project_id = seed.first_project_id THEN seed.old_id
    ELSE substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
        || substr('89ab', 1 + floor(random() * 4)::INT, 1) || substr(seed.r, 4, 3) || '-'
        || substr(seed.r, 7, 12)
    END AS new_id
FROM (
    SELECT
        labels.id AS old_id,
        projects.id AS project_id,
        (SELECT first.id FROM projects AS first
         WHERE first.workspace_id = labels.workspace_id
         ORDER BY first.deleted_at IS NOT NULL, first.created_at, first.id LIMIT 1) AS first_project_id,
        lpad(to_hex((extract(epoch FROM clock_timestamp()) * 1000)::BIGINT), 12, '0') AS ts,
        substr(replace(gen_random_uuid()::TEXT, '-', ''), 1, 12)
            || substr(replace(gen_random_uuid()::TEXT, '-', ''), 1, 6) AS r
    FROM labels
    JOIN projects ON projects.workspace_id = labels.workspace_id
) AS seed;

ALTER TABLE labels ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE CASCADE;
ALTER TABLE labels DROP CONSTRAINT labels_workspace_id_name_key;

UPDATE labels SET project_id = label_copies.project_id
FROM label_copies
WHERE label_copies.old_id = labels.id AND label_copies.new_id = labels.id;

-- A workspace without projects has no task that could use its labels.
DELETE FROM labels WHERE project_id IS NULL;

INSERT INTO labels (id, workspace_id, project_id, name, color, version, created_at, updated_at)
SELECT label_copies.new_id, labels.workspace_id, label_copies.project_id,
       labels.name, labels.color, labels.version, labels.created_at, labels.updated_at
FROM label_copies
JOIN labels ON labels.id = label_copies.old_id
WHERE label_copies.new_id <> label_copies.old_id;

ALTER TABLE labels ALTER COLUMN project_id SET NOT NULL;
ALTER TABLE labels ADD CONSTRAINT labels_project_id_name_key UNIQUE (project_id, name);
CREATE INDEX labels_project_name ON labels (project_id, name, id);

CREATE OR REPLACE FUNCTION task_labels_validate_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM tasks JOIN labels ON labels.id = NEW.label_id
        WHERE tasks.id = NEW.task_id AND labels.project_id = tasks.project_id
    )
    THEN
        RAISE EXCEPTION 'task label must belong to the task project';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION task_labels_validate_scope_update_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM tasks JOIN labels ON labels.id = NEW.label_id
        WHERE tasks.id = NEW.task_id AND labels.project_id = tasks.project_id
    )
    THEN
        RAISE EXCEPTION 'task label must belong to the task project';
    END IF;
    RETURN NEW;
END;
$$;

UPDATE task_labels SET label_id = label_copies.new_id
FROM tasks, label_copies
WHERE tasks.id = task_labels.task_id
  AND label_copies.old_id = task_labels.label_id
  AND label_copies.project_id = tasks.project_id
  AND label_copies.new_id <> task_labels.label_id;

-- A rebuilt label reference cascades, as in SQLite, so deleting a label removes it from tasks.
ALTER TABLE task_labels DROP CONSTRAINT task_labels_label_id_fkey;
ALTER TABLE task_labels ADD CONSTRAINT task_labels_label_id_fkey
    FOREIGN KEY (label_id) REFERENCES labels(id) ON DELETE CASCADE;

CREATE FUNCTION labels_validate_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = NEW.project_id AND projects.workspace_id = NEW.workspace_id
    )
    THEN
        RAISE EXCEPTION 'label project must belong to the workspace';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER labels_validate_scope_insert
    BEFORE INSERT ON labels
    FOR EACH ROW
    EXECUTE FUNCTION labels_validate_scope_insert_fn();

DROP TRIGGER labels_workspace_immutable ON labels;
DROP FUNCTION labels_workspace_immutable_fn();
CREATE FUNCTION labels_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'label scope is immutable';
END;
$$;
CREATE TRIGGER labels_scope_immutable
    BEFORE UPDATE OF workspace_id, project_id ON labels
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.project_id <> OLD.project_id)
    EXECUTE FUNCTION labels_scope_immutable_fn();
