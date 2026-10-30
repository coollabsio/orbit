pub mod account;
pub mod api_tokens;
pub mod attachments;
pub mod billing;
pub mod chat;
pub mod identity;
pub mod instance_settings;
pub(crate) mod membership;
pub mod page_comments;
pub mod page_export;
pub mod page_files;
pub mod page_mentions;
pub mod page_versions;
pub mod pages;
pub mod sub_issues;
pub mod task_filter;
pub mod task_relations;
pub mod tasks;
pub mod sign_in_providers;
pub mod teamspaces;
pub mod two_factor;
pub mod views;
pub mod workspace_storage;
pub mod workspaces;

/// Test helper: a trigger that makes `event` (e.g. `BEFORE INSERT`, `BEFORE UPDATE OF revoked_at`)
/// on `table` fail with `message` for rows that match `when` (an SQL condition on `NEW`).
#[cfg(test)]
pub(crate) async fn fail_writes(
    database: &orbit_platform::Database,
    name: &str,
    event: &str,
    table: &str,
    when: &str,
    message: &str,
) {
    let sql = match database.backend() {
        orbit_platform::Backend::Sqlite => format!(
            "CREATE TRIGGER {name} {event} ON {table} WHEN {when} \
             BEGIN SELECT RAISE(ABORT, '{message}'); END"
        ),
        orbit_platform::Backend::Postgres => format!(
            "CREATE FUNCTION {name}_fn() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN RAISE EXCEPTION '{message}'; END $$; \
             CREATE TRIGGER {name} {event} ON {table} FOR EACH ROW WHEN ({when}) \
             EXECUTE FUNCTION {name}_fn()"
        ),
    };
    database.execute(&sql).await.unwrap();
}
