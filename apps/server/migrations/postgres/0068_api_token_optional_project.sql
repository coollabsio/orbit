-- A token without a task scope has no project, so `project_id` may be NULL. A set project must
-- still be active and belong to the token's workspace. SQLite now runs this check before the
-- service account trigger, which is also the Postgres (name) order, so the insert function no
-- longer repeats the service account check.
CREATE OR REPLACE FUNCTION api_tokens_validate_project_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.project_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = NEW.project_id
          AND projects.workspace_id = NEW.workspace_id
          AND projects.deleted_at IS NULL
    ) THEN
        RAISE EXCEPTION 'api token project must be active and belong to the workspace';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION api_tokens_validate_project_update_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.project_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = NEW.project_id
          AND projects.workspace_id = NEW.workspace_id
          AND projects.deleted_at IS NULL
    ) THEN
        RAISE EXCEPTION 'api token project must be active and belong to the workspace';
    END IF;
    RETURN NEW;
END;
$$;
