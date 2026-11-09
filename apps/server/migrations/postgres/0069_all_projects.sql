-- A token or grant with `all_projects` set may use the tasks of every live project of its
-- workspace, including projects created later. It stores no projects of its own.
ALTER TABLE api_tokens ADD COLUMN all_projects BIGINT NOT NULL DEFAULT 0 CHECK (all_projects IN (0, 1));
ALTER TABLE oauth_grants ADD COLUMN all_projects BIGINT NOT NULL DEFAULT 0 CHECK (all_projects IN (0, 1));
