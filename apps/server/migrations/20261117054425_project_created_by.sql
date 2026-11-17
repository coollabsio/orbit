-- SQLite. Postgres: postgres/20261117054425_project_created_by.sql.
--
-- Who created a project (issue #73): its creator manages it without `projects.manage`. Projects
-- that exist already have no known creator.
ALTER TABLE projects ADD COLUMN created_by TEXT REFERENCES users(id) ON DELETE SET NULL;
