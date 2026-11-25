-- SQLite. Postgres: postgres/20261125081144_time_entry_external_ids.sql.
--
-- The source entry of an imported time entry (issue #95): `asana:<gid>` for the Asana import, NULL
-- otherwise. Unique per task, so a retried or resumed import never logs the same entry twice.
ALTER TABLE task_time_entries ADD COLUMN external_id TEXT;
CREATE UNIQUE INDEX task_time_entries_external_id ON task_time_entries (task_id, external_id);
