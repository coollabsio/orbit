-- 1 while a job moves the person's tasks to the member an admin picked (Settings → Import), so the
-- page shows the work after a reload. The job that finishes the latest pick sets it back to 0.
ALTER TABLE import_people ADD COLUMN assigning BIGINT NOT NULL DEFAULT 0 CHECK (assigning IN (0, 1));
