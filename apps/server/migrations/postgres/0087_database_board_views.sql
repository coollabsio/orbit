-- Docs databases: board views. A board groups the rows into columns by a select or status
-- property; its settings live in config_json next to the table settings.
ALTER TABLE database_views DROP CONSTRAINT database_views_type_check;
ALTER TABLE database_views ADD CONSTRAINT database_views_type_check CHECK (type IN ('table', 'board'));
