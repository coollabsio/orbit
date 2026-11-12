-- SQLite. Postgres: postgres/20261112175306_task_cycle_carryovers.sql.
--
-- The tasks that "Move unfinished tasks to next cycle" took out of a cycle (issue #71). Without
-- this record, a past cycle counts only the tasks still in it and looks done when its unfinished
-- tasks have moved. One row per cycle and task: a task carried out of the same cycle twice stays
-- one row.

CREATE TABLE task_cycle_carryovers (
    cycle_id TEXT NOT NULL REFERENCES task_cycles(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    carried_at INTEGER NOT NULL,
    PRIMARY KEY (cycle_id, task_id)
);

-- Serves the ON DELETE CASCADE lookup when a task is deleted.
CREATE INDEX task_cycle_carryovers_task ON task_cycle_carryovers (task_id);
