//! Which projects a member sees (private projects, #37). The rule itself is
//! `Policy::can_see_project`; this module is its only SQL form. Every query that reads or writes a
//! project, or anything inside one (tasks, comments, labels, statuses, attachments, relations,
//! notifications), puts one of these conditions in its `WHERE`.

use orbit_domain::{Actor, Policy, WorkspaceRole};
use orbit_platform::{Flag, Id};
use sqlx::Any;

/// The projects a reader sees. `All` is for owners, admins and work the server does itself
/// (integrations, automation, retention); `Member` sees workspace-wide projects and the private
/// ones that list the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectScope {
    All,
    Member(Id),
}

impl ProjectScope {
    #[must_use]
    pub(crate) fn of(actor: Actor) -> Self {
        if actor.sees_all_projects() {
            Self::All
        } else {
            Self::Member(actor.user_id)
        }
    }

    /// SQL condition: the project whose id is the expression `project_id_sql` is in scope. It
    /// binds nothing (the user id is written as a literal; an `Id` is a UUID), so it fits any
    /// query whatever its placeholders. It is false for a NULL id.
    #[must_use]
    pub(crate) fn project(self, project_id_sql: &str) -> String {
        match self {
            Self::All => format!("{project_id_sql} IS NOT NULL"),
            Self::Member(user_id) => format!(
                "EXISTS (SELECT 1 FROM projects AS scope_project WHERE scope_project.id = {project_id_sql} \
                 AND (scope_project.private = 0 OR EXISTS (SELECT 1 FROM project_members AS scope_member \
                 WHERE scope_member.project_id = scope_project.id AND scope_member.user_id = '{user_id}')))"
            ),
        }
    }

    /// SQL condition: the task whose id is the expression `task_id_sql` is in a project in scope.
    #[must_use]
    pub(crate) fn task(self, task_id_sql: &str) -> String {
        match self {
            Self::All => format!("{task_id_sql} IS NOT NULL"),
            Self::Member(_) => format!(
                "EXISTS (SELECT 1 FROM tasks AS scope_task WHERE scope_task.id = {task_id_sql} AND {})",
                self.project("scope_task.project_id")
            ),
        }
    }
}

/// SQL condition: the user whose id is the expression `user_id_sql` sees the project whose id is
/// `project_id_sql`. For queries about someone other than a known [`Actor`], or about many users
/// at once (who may be assigned or mentioned, whose push or socket this is). The role lists come
/// from `Policy::can_see_project`, so the rule still has one source. It binds nothing.
#[must_use]
pub(crate) fn user_sees_project_sql(project_id_sql: &str, user_id_sql: &str) -> String {
    let roles = |private: bool| {
        [
            WorkspaceRole::Owner,
            WorkspaceRole::Admin,
            WorkspaceRole::Member,
            WorkspaceRole::Guest,
        ]
        .into_iter()
        .filter(|role| Policy::can_see_project(*role, private, false))
        .map(|role| format!("'{}'", role.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
    };
    // `'' ` keeps `IN ()` valid if a role list is ever empty.
    format!(
        "EXISTS (SELECT 1 FROM projects AS seen_project \
         JOIN memberships AS seen_membership ON seen_membership.workspace_id = seen_project.workspace_id \
         WHERE seen_project.id = {project_id_sql} AND seen_membership.user_id = {user_id_sql} \
         AND (seen_membership.role IN ('', {see_all}) OR (seen_membership.role IN ('', {see_open}) \
         AND (seen_project.private = 0 OR EXISTS (SELECT 1 FROM project_members AS seen_member \
         WHERE seen_member.project_id = seen_project.id AND seen_member.user_id = seen_membership.user_id)))))",
        see_all = roles(true),
        see_open = roles(false),
    )
}

/// As [`user_sees_project_sql`], for the project of the task whose id is `task_id_sql`.
#[must_use]
pub(crate) fn user_sees_task_sql(task_id_sql: &str, user_id_sql: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM tasks AS seen_task WHERE seen_task.id = {task_id_sql} AND {})",
        user_sees_project_sql("seen_task.project_id", user_id_sql)
    )
}

/// Whether `user_id` is an active member (not suspended) who sees the project: who may be
/// assigned to its tasks.
pub(crate) async fn user_sees_project<'e, E>(
    executor: E,
    project_id: Id,
    user_id: Id,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let Flag(sees) = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS (SELECT 1 FROM users WHERE users.id = $1 AND users.suspended_at IS NULL AND {})",
        user_sees_project_sql("$2", "users.id")
    )))
    .bind(user_id.to_string())
    .bind(project_id.to_string())
    .fetch_one(executor)
    .await?;
    Ok(sees)
}

/// As [`user_sees_project`], for the project of the task `task_id`: who is told about it.
pub(crate) async fn user_sees_task<'e, E>(
    executor: E,
    task_id: Id,
    user_id: Id,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let Flag(sees) = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS (SELECT 1 FROM users WHERE users.id = $1 AND users.suspended_at IS NULL AND {})",
        user_sees_task_sql("$2", "users.id")
    )))
    .bind(user_id.to_string())
    .bind(task_id.to_string())
    .fetch_one(executor)
    .await?;
    Ok(sees)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_lists_follow_the_policy() {
        let sql = user_sees_project_sql("p", "u");
        assert!(
            sql.contains("seen_membership.role IN ('', 'owner', 'admin')"),
            "{sql}"
        );
        assert!(
            sql.contains("seen_membership.role IN ('', 'owner', 'admin', 'member')"),
            "{sql}"
        );
    }
}
