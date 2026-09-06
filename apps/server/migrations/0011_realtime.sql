-- Counters survive event retention so workspace sequences never restart.
CREATE TABLE realtime_sequences (
    workspace_id TEXT PRIMARY KEY,
    sequence INTEGER NOT NULL
);
CREATE TRIGGER audit_workspace_realtime AFTER INSERT ON audit_events
WHEN NEW.workspace_id IS NOT NULL AND NEW.outcome = 'success'
BEGIN
    INSERT INTO realtime_sequences (workspace_id, sequence) VALUES (NEW.workspace_id, 1)
    ON CONFLICT(workspace_id) DO UPDATE SET sequence = sequence + 1;
    INSERT INTO outbox_events (id, scope, sequence, workspace_id, topic, payload_json, created_at)
    SELECT NEW.id, NEW.workspace_id, sequence, NEW.workspace_id, 'workspace.changed', '{}', NEW.occurred_at
    FROM realtime_sequences WHERE workspace_id = NEW.workspace_id;
END;
CREATE INDEX outbox_events_workspace_created ON outbox_events (workspace_id, created_at);
