-- Postgres: the same change as apps/server/migrations/20261125131647_time_tracking_backfill.sql

-- Ids are canonical lowercase UUIDv7, as in 0051.
INSERT INTO task_fields (id, workspace_id, name, kind, options_json, created_by, version, created_at, updated_at)
SELECT
    substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
        || substr('89ab', 1 + floor(random() * 4)::INT, 1) || substr(seed.r, 4, 3) || '-'
        || substr(seed.r, 7, 12),
    seed.workspace_id,
    CASE WHEN EXISTS (
        SELECT 1 FROM task_fields AS taken
        WHERE taken.workspace_id = seed.workspace_id AND lower(taken.name) = 'time tracking'
    ) THEN 'Logged time' ELSE 'Time tracking' END,
    'time_tracking', '[]', NULL, 0, seed.now_ms, seed.now_ms
FROM (
    SELECT
        logged.workspace_id,
        clock.now_ms,
        lpad(to_hex(clock.now_ms), 12, '0') AS ts,
        substr(replace(gen_random_uuid()::TEXT, '-', ''), 1, 12)
            || substr(replace(gen_random_uuid()::TEXT, '-', ''), 1, 6) AS r
    FROM (SELECT DISTINCT workspace_id FROM task_time_entries) AS logged
    CROSS JOIN (SELECT (extract(epoch FROM clock_timestamp()) * 1000)::BIGINT AS now_ms) AS clock
    WHERE NOT EXISTS (
        SELECT 1 FROM task_fields AS existing
        WHERE existing.workspace_id = logged.workspace_id AND existing.kind = 'time_tracking'
    )
) AS seed
ON CONFLICT DO NOTHING;

INSERT INTO project_task_fields (project_id, field_id, position)
SELECT
    logged.project_id,
    field.id,
    COALESCE(
        (SELECT MAX(shown.position) + 1 FROM project_task_fields AS shown
         WHERE shown.project_id = logged.project_id),
        0
    )
FROM (
    SELECT DISTINCT tasks.workspace_id, tasks.project_id
    FROM task_time_entries JOIN tasks ON tasks.id = task_time_entries.task_id
) AS logged
JOIN task_fields AS field
    ON field.workspace_id = logged.workspace_id AND field.kind = 'time_tracking'
ON CONFLICT DO NOTHING;
