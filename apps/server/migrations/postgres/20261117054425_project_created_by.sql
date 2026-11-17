-- Postgres: the same change as ../20261117054425_project_created_by.sql (issue #73).
ALTER TABLE projects ADD COLUMN created_by TEXT REFERENCES users(id) ON DELETE SET NULL;
