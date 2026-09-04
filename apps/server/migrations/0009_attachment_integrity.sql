CREATE INDEX attachment_references_task
    ON attachment_references (workspace_id, task_id, comment_id, created_at, id);

CREATE TRIGGER attachment_blobs_workspace_immutable
BEFORE UPDATE OF workspace_id ON attachment_blobs
WHEN NEW.workspace_id <> OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'attachment blob workspace is immutable');
END;

CREATE TRIGGER attachment_references_reject_scope_mismatch
BEFORE INSERT ON attachment_references
WHEN EXISTS (
    SELECT 1 FROM attachment_blobs
    WHERE attachment_blobs.id = NEW.blob_id
      AND attachment_blobs.workspace_id <> NEW.workspace_id
) OR EXISTS (
    SELECT 1 FROM tasks
    WHERE tasks.id = NEW.task_id
      AND tasks.workspace_id <> NEW.workspace_id
) OR (
    NEW.comment_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM task_comments
        WHERE task_comments.id = NEW.comment_id
          AND (
            task_comments.task_id <> NEW.task_id
            OR task_comments.workspace_id <> NEW.workspace_id
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'attachment reference scope mismatch');
END;

CREATE TRIGGER attachment_references_scope_immutable
BEFORE UPDATE OF workspace_id, task_id, comment_id, blob_id ON attachment_references
WHEN NEW.workspace_id <> OLD.workspace_id
  OR NEW.task_id <> OLD.task_id
  OR NEW.comment_id IS NOT OLD.comment_id
  OR NEW.blob_id <> OLD.blob_id
BEGIN
    SELECT RAISE(ABORT, 'attachment reference scope is immutable');
END;

CREATE TRIGGER task_comments_cleanup_attachments
BEFORE DELETE ON task_comments
BEGIN
    UPDATE attachment_blobs
    SET quarantine_until = MAX(
        quarantine_until,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000 + 86400000
    )
    WHERE id IN (
        SELECT blob_id FROM attachment_references WHERE comment_id = OLD.id
    );
    DELETE FROM attachment_references WHERE comment_id = OLD.id;
END;

CREATE TRIGGER tasks_cleanup_attachments
BEFORE DELETE ON tasks
BEGIN
    UPDATE attachment_blobs
    SET quarantine_until = MAX(
        quarantine_until,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000 + 86400000
    )
    WHERE id IN (
        SELECT blob_id FROM attachment_references WHERE task_id = OLD.id
    );
    DELETE FROM attachment_references WHERE task_id = OLD.id;
END;
