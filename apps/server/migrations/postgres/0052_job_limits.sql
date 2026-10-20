-- Job concurrency that the root user sets in Admin, read by every server on each claim
-- (crates/platform/src/jobs/store.rs). A row replaces the code defaults of one job kind; NULL
-- means no limit. `concurrency` counts the running jobs of the kind on all servers,
-- `workspace_concurrency` those of one workspace while other workspaces wait.
CREATE TABLE job_limits (
    kind TEXT PRIMARY KEY,
    concurrency BIGINT CHECK (concurrency IS NULL OR concurrency > 0),
    workspace_concurrency BIGINT CHECK (workspace_concurrency IS NULL OR workspace_concurrency > 0),
    updated_at BIGINT NOT NULL
);
