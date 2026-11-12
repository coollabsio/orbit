-- Postgres: the same change as ../20261112175306_task_cycle_carryovers.sql (issue #71).

CREATE TABLE task_cycle_carryovers (
    cycle_id TEXT NOT NULL REFERENCES task_cycles(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    carried_at BIGINT NOT NULL,
    PRIMARY KEY (cycle_id, task_id)
);

CREATE INDEX task_cycle_carryovers_task ON task_cycle_carryovers (task_id);
