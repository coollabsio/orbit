//! People an Asana, Linear or Trello import could not assign: the source assignee had no Orbit
//! member with the same email (Trello: the same full name), so the task was left unassigned and
//! names them in its description. Each such person is kept with their tasks; an admin maps them
//! to any member in Settings → Import, which assigns all their tasks in one step. A member who
//! joins with the person's email is mapped on join, and later imports assign mapped people.

use std::collections::HashMap;

use orbit_domain::Permission;
use orbit_platform::{
    Database, Id, Job, JobError, JobKind, JobKindRegistrationError, JobQueue, JobStore, Row,
    TimestampMillis, Worker, is_transient_lock_error,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use utoipa::ToSchema;

use crate::repositories::membership;
use crate::repositories::tasks::{TaskError, TaskRepository};

/// The tool an import came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = ImportSource)]
pub enum Source {
    Asana,
    Linear,
    Trello,
}

impl Source {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asana => "asana",
            Self::Linear => "linear",
            Self::Trello => "trello",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "asana" => Some(Self::Asana),
            "linear" => Some(Self::Linear),
            "trello" => Some(Self::Trello),
            _ => None,
        }
    }
}

/// Names the importers write when the source gave none; never a person.
const PLACEHOLDERS: [&str; 2] = ["unknown", "trello member"];

fn clean_name(name: &str) -> String {
    name.replace(['\r', '\n'], " ").trim().to_owned()
}

fn name_key(name: &str) -> String {
    clean_name(name).to_lowercase()
}

/// How an importer assigns one source assignee.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Match {
    /// A member has the source email.
    Email(Id),
    /// An admin mapped the person to this member.
    Mapped(Id),
    Unmatched,
}

impl Match {
    pub(crate) const fn user_id(self) -> Option<Id> {
        match self {
            Self::Email(id) | Self::Mapped(id) => Some(id),
            Self::Unmatched => None,
        }
    }
}

/// `members` maps lowercase email to member (`task_import::member_emails`), `mapped` the name
/// key of a mapped person to their member ([`mapped_members`]).
pub(crate) fn match_assignee(
    members: &HashMap<String, Id>,
    mapped: &HashMap<String, Id>,
    email: Option<&str>,
    name: &str,
) -> Match {
    if let Some(id) = email.and_then(|email| members.get(&email.trim().to_lowercase())) {
        return Match::Email(*id);
    }
    match mapped_member(mapped, name) {
        Some(id) => Match::Mapped(id),
        None => Match::Unmatched,
    }
}

/// The member a person named `name` is mapped to, in `mapped` from [`mapped_members`].
pub(crate) fn mapped_member(mapped: &HashMap<String, Id>, name: &str) -> Option<Id> {
    mapped.get(&name_key(name)).copied()
}

/// Keeps `name` as a person of the import's workspace with `task_id` among their tasks (the
/// email is added when it was unknown). Idempotent; placeholder names are skipped.
pub(crate) async fn record(
    database: &Database,
    workspace_id: Id,
    source: Source,
    name: &str,
    email: Option<&str>,
    task_id: Id,
) -> Result<(), sqlx::Error> {
    let name = clean_name(name);
    let key = name.to_lowercase();
    if key.is_empty() || PLACEHOLDERS.contains(&key.as_str()) {
        return Ok(());
    }
    let email = email
        .map(|email| email.trim().to_lowercase())
        .filter(|email| !email.is_empty());
    let now = TimestampMillis::now().as_millis();
    let person_id: String = sqlx::query_scalar(
        "INSERT INTO import_people (id, workspace_id, source, name, name_key, email, user_id, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $7) \
         ON CONFLICT (workspace_id, source, name_key) DO UPDATE SET \
         email = COALESCE(import_people.email, excluded.email), \
         updated_at = CASE WHEN import_people.email IS NULL AND excluded.email IS NOT NULL \
         THEN excluded.updated_at ELSE import_people.updated_at END \
         RETURNING id",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace_id.to_string())
    .bind(source.as_str())
    .bind(&name)
    .bind(&key)
    .bind(email)
    .bind(now)
    .fetch_one(database.pool())
    .await?;
    sqlx::query(
        "INSERT INTO import_person_tasks (person_id, task_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(person_id)
    .bind(task_id.to_string())
    .execute(database.pool())
    .await?;
    Ok(())
}

/// Name key → member of the source's mapped people whose member is still active.
pub(crate) async fn mapped_members(
    database: &Database,
    workspace_id: Id,
    source: Source,
) -> Result<HashMap<String, Id>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT import_people.name_key, import_people.user_id FROM import_people \
         JOIN memberships ON memberships.workspace_id = import_people.workspace_id \
         AND memberships.user_id = import_people.user_id \
         JOIN users ON users.id = import_people.user_id \
         WHERE import_people.workspace_id = $1 AND import_people.source = $2 \
         AND users.suspended_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(source.as_str())
    .fetch_all(database.pool())
    .await?
    .into_iter()
    .filter_map(|row| {
        let id = row.get::<String, _>("user_id").parse().ok()?;
        Some((row.get("name_key"), id))
    })
    .collect())
}

/// A person from an import, with the number of their tasks that are not deleted.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ImportPerson {
    #[schema(value_type = String)]
    pub id: Id,
    pub source: Source,
    pub name: String,
    /// Lowercase; `null` when the source did not give one.
    pub email: Option<String>,
    /// The member their tasks are assigned to; `null` when not mapped.
    #[schema(value_type = Option<String>)]
    pub user_id: Option<Id>,
    pub task_count: u32,
    /// A job is still moving the tasks to `user_id`.
    pub assigning: bool,
}

#[derive(Debug, Error)]
pub enum ImportPeopleError {
    #[error("import person was not found")]
    NotFound,
    #[error("only workspace admins map imported people")]
    Forbidden,
    #[error("the user is not an active member of the workspace")]
    InvalidMember,
    #[error("import people storage failed")]
    Unavailable(#[from] sqlx::Error),
    #[error("task update failed: {0}")]
    Task(TaskError),
}

impl From<TaskError> for ImportPeopleError {
    fn from(error: TaskError) -> Self {
        match error {
            TaskError::Unavailable(error) => Self::Unavailable(error),
            other => Self::Task(other),
        }
    }
}

const PERSON_COLUMNS: &str = "import_people.id, import_people.source, import_people.name, \
     import_people.email, import_people.user_id, import_people.assigning, COUNT(tasks.id) AS task_count \
     FROM import_people \
     JOIN import_person_tasks ON import_person_tasks.person_id = import_people.id \
     JOIN tasks ON tasks.id = import_person_tasks.task_id AND tasks.workspace_id = import_people.workspace_id \
     AND tasks.deleted_at IS NULL \
     JOIN projects ON projects.id = tasks.project_id AND projects.deleted_at IS NULL";

const PERSON_GROUP: &str = "GROUP BY import_people.id, import_people.source, import_people.name, \
     import_people.email, import_people.user_id, import_people.assigning, import_people.name_key";

fn person_from_row(row: &sqlx::any::AnyRow) -> Option<ImportPerson> {
    Some(ImportPerson {
        id: row.get::<String, _>("id").parse().ok()?,
        source: Source::parse(&row.get::<String, _>("source"))?,
        name: row.get("name"),
        email: row.get("email"),
        user_id: row
            .get::<Option<String>, _>("user_id")
            .and_then(|id| id.parse().ok()),
        task_count: u32::try_from(row.get::<i64, _>("task_count")).unwrap_or(u32::MAX),
        assigning: row.get::<bool, _>("assigning"),
    })
}

async fn require_admin(
    database: &Database,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), ImportPeopleError> {
    match membership::actor(database.pool(), workspace_id, actor_id).await? {
        None => Err(ImportPeopleError::NotFound),
        Some(actor) if !actor.can(Permission::MembersManage) => Err(ImportPeopleError::Forbidden),
        Some(_) => Ok(()),
    }
}

/// The workspace's people with at least one task that is not deleted, by name. Admins only.
pub async fn list(
    database: &Database,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Vec<ImportPerson>, ImportPeopleError> {
    require_admin(database, workspace_id, actor_id).await?;
    let sql = format!(
        "SELECT {PERSON_COLUMNS} WHERE import_people.workspace_id = $1 {PERSON_GROUP} \
         ORDER BY import_people.name_key, import_people.source, import_people.id"
    );
    Ok(sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(workspace_id.to_string())
        .fetch_all(database.pool())
        .await?
        .iter()
        .filter_map(person_from_row)
        .collect())
}

async fn person(
    database: &Database,
    workspace_id: Id,
    person_id: Id,
) -> Result<ImportPerson, ImportPeopleError> {
    let sql = format!(
        "SELECT {PERSON_COLUMNS} WHERE import_people.workspace_id = $1 AND import_people.id = $2 \
         {PERSON_GROUP}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(workspace_id.to_string())
        .bind(person_id.to_string())
        .fetch_optional(database.pool())
        .await?;
    match row.as_ref().and_then(person_from_row) {
        Some(person) => Ok(person),
        // Every task of the person was deleted meanwhile.
        None => {
            let row = sqlx::query(
                "SELECT id, source, name, email, user_id, assigning, CAST(0 AS BIGINT) AS task_count FROM import_people \
                 WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id.to_string())
            .bind(person_id.to_string())
            .fetch_optional(database.pool())
            .await?;
            row.as_ref()
                .and_then(person_from_row)
                .ok_or(ImportPeopleError::NotFound)
        }
    }
}

/// The job that moves a person's tasks after an admin picked a member.
pub const ASSIGN_JOB: &str = "import_people.assign";

/// Maps the person to `user_id` (`None` unmaps) and queues the job that moves each of their
/// tasks that is not deleted from the previous member to the new one, without notifications.
/// The person is `assigning` until that job ends. Admins only.
pub async fn set_member(
    database: &Database,
    workspace_id: Id,
    actor_id: Id,
    person_id: Id,
    user_id: Option<Id>,
    request_id: &str,
) -> Result<ImportPerson, ImportPeopleError> {
    require_admin(database, workspace_id, actor_id).await?;
    let previous = map(database, workspace_id, person_id, user_id, true).await?;
    let job = Job::new(
        JobKind::new(ASSIGN_JOB),
        json!(AssignJob {
            workspace_id,
            actor_id,
            person_id,
            from: previous,
            to: user_id,
            request_id: request_id.to_owned(),
        }),
        TimestampMillis::now(),
    )
    .with_workspace(workspace_id.to_string());
    if let Err(error) = JobQueue::new(JobStore::new(database.clone()))
        .enqueue(job)
        .await
    {
        tracing::warn!(person_id = %person_id, %error, "the import person job could not be queued");
        sqlx::query("UPDATE import_people SET assigning = 0 WHERE id = $1")
            .bind(person_id.to_string())
            .execute(database.pool())
            .await?;
    }
    person(database, workspace_id, person_id).await
}

#[derive(Deserialize, Serialize)]
struct AssignJob {
    workspace_id: Id,
    actor_id: Id,
    person_id: Id,
    from: Option<Id>,
    to: Option<Id>,
    request_id: String,
}

/// Registers the [`ASSIGN_JOB`] handler. A failed run is retried: moving a task twice changes
/// nothing. After the last attempt the person stops showing as `assigning`.
pub fn register_job(
    worker: Worker,
    database: Database,
) -> Result<Worker, JobKindRegistrationError> {
    worker.with_handler(JobKind::new(ASSIGN_JOB), move |context| {
        let database = database.clone();
        async move {
            let last_attempt = context.claim.attempt >= context.job.max_attempts;
            run_assign_job(&database, &context.job.payload, last_attempt).await
        }
    })
}

/// One run of [`ASSIGN_JOB`] with its payload.
pub async fn run_assign_job(
    database: &Database,
    payload: &serde_json::Value,
    last_attempt: bool,
) -> Result<(), JobError> {
    let Ok(job) = serde_json::from_value::<AssignJob>(payload.clone()) else {
        return Err(JobError::Permanent(format!("invalid {ASSIGN_JOB} payload")));
    };
    let result = move_tasks(
        database,
        job.workspace_id,
        job.actor_id,
        job.person_id,
        (job.from, job.to),
        &job.request_id,
    )
    .await;
    if result.is_ok() || last_attempt {
        // only the job of the latest pick ends the work: an older one sees another member
        sqlx::query(
            "UPDATE import_people SET assigning = 0 WHERE id = $1 AND user_id IS NOT DISTINCT FROM $2",
        )
        .bind(job.person_id.to_string())
        .bind(job.to.map(|id| id.to_string()))
        .execute(database.pool())
        .await
        .map_err(|error| JobError::Retryable(error.to_string()))?;
    }
    result.map_err(|error| JobError::Retryable(error.to_string()))
}

/// Sets the person's member and returns the previous one. `assigning` marks the tasks as not
/// moved yet.
async fn map(
    database: &Database,
    workspace_id: Id,
    person_id: Id,
    user_id: Option<Id>,
    assigning: bool,
) -> Result<Option<Id>, ImportPeopleError> {
    let mut tx = database.immediate_transaction().await?;
    let previous: Option<Option<String>> =
        sqlx::query_scalar("SELECT user_id FROM import_people WHERE id = $1 AND workspace_id = $2")
            .bind(person_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
    let previous: Option<Id> = previous
        .ok_or(ImportPeopleError::NotFound)?
        .and_then(|id| id.parse().ok());
    if let Some(user_id) = user_id {
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = $1 AND memberships.user_id = $2 \
             AND users.suspended_at IS NULL",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active != 1 {
            return Err(ImportPeopleError::InvalidMember);
        }
    }
    sqlx::query(
        "UPDATE import_people SET user_id = $1, assigning = $2, updated_at = $3 WHERE id = $4",
    )
    .bind(user_id.map(|id| id.to_string()))
    .bind(i64::from(assigning))
    .bind(TimestampMillis::now().as_millis())
    .bind(person_id.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(previous)
}

/// Tasks per transaction while a person's tasks change hands. A write transaction holds every
/// writer of the database, and 4,000 tasks in one held them for 15 seconds.
const ASSIGN_BATCH: usize = 200;

/// Moves the person's tasks from `from` to `to` in batches. Stops when the person was mapped to
/// someone else meanwhile: the newer pick moves every task from `to` to its own member.
async fn move_tasks(
    database: &Database,
    workspace_id: Id,
    actor_id: Id,
    person_id: Id,
    (from, to): (Option<Id>, Option<Id>),
    request_id: &str,
) -> Result<(), ImportPeopleError> {
    let now = TimestampMillis::now();
    let task_ids: Vec<Id> = sqlx::query_scalar::<_, String>(
        "SELECT task_id FROM import_person_tasks WHERE person_id = $1 ORDER BY task_id",
    )
    .bind(person_id.to_string())
    .fetch_all(database.pool())
    .await?
    .into_iter()
    .filter_map(|id| id.parse().ok())
    .collect();
    let tasks = TaskRepository::new(database.clone());
    for batch in task_ids.chunks(ASSIGN_BATCH) {
        let mut tx = database.immediate_transaction().await?;
        let current: Option<Option<String>> =
            sqlx::query_scalar("SELECT user_id FROM import_people WHERE id = $1")
                .bind(person_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if current.flatten() != to.map(|id| id.to_string()) {
            break;
        }
        tasks
            .swap_assignee_in_tx(
                &mut tx,
                workspace_id,
                actor_id,
                batch,
                (from, to),
                request_id,
                now,
            )
            .await?;
        tx.commit().await?;
    }
    Ok(())
}

/// After a member joined: maps every unmapped person of the workspace with the member's email
/// to them and assigns their tasks, with the member as the actor. Never fails the join.
pub async fn map_joined_member(database: &Database, membership_id: Id, request_id: &str) {
    if let Err(error) = try_map_joined_member(database, membership_id, request_id).await {
        tracing::warn!(error = %error, membership_id = %membership_id, "mapping imported people to a new member failed");
    }
}

async fn try_map_joined_member(
    database: &Database,
    membership_id: Id,
    request_id: &str,
) -> Result<(), ImportPeopleError> {
    let Some(member) = sqlx::query(
        "SELECT memberships.workspace_id, memberships.user_id, users.normalized_email \
         FROM memberships JOIN users ON users.id = memberships.user_id WHERE memberships.id = $1",
    )
    .bind(membership_id.to_string())
    .fetch_optional(database.pool())
    .await?
    else {
        return Ok(());
    };
    let (Ok(workspace_id), Ok(user_id)) = (
        member.get::<String, _>("workspace_id").parse::<Id>(),
        member.get::<String, _>("user_id").parse::<Id>(),
    ) else {
        return Ok(());
    };
    let email = member.get::<String, _>("normalized_email").to_lowercase();
    let people: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM import_people WHERE workspace_id = $1 AND email = $2 AND user_id IS NULL",
    )
    .bind(workspace_id.to_string())
    .bind(email)
    .fetch_all(database.pool())
    .await?;
    for person_id in people.iter().filter_map(|id| id.parse().ok()) {
        let previous = map(database, workspace_id, person_id, Some(user_id), false).await?;
        move_tasks(
            database,
            workspace_id,
            user_id,
            person_id,
            (previous, Some(user_id)),
            request_id,
        )
        .await?;
    }
    Ok(())
}

/// Server start: reads the people of the imports queued in `import_people_backfill` (imports
/// made before people were kept) from their task descriptions, then dequeues each import. A
/// failure leaves the rest queued for the next start.
pub async fn backfill(database: Database) {
    if let Err(error) = try_backfill(&database).await {
        if is_transient_lock_error(&error) {
            tracing::info!(error = %error, "imported people backfill postponed to the next start");
        } else {
            tracing::warn!(error = %error, "imported people backfill failed");
        }
    }
}

async fn try_backfill(database: &Database) -> Result<(), sqlx::Error> {
    let queued = sqlx::query("SELECT import_id, source FROM import_people_backfill")
        .fetch_all(database.pool())
        .await?;
    for row in queued {
        let import_id: String = row.get("import_id");
        let source_name: String = row.get("source");
        if let Some(source) = Source::parse(&source_name) {
            let (imports, items, kind) = match source {
                Source::Asana => ("asana_imports", "asana_import_items", "task"),
                Source::Linear => ("linear_imports", "linear_import_items", "issue"),
                Source::Trello => ("trello_imports", "trello_import_items", "card"),
            };
            let sql = format!(
                "SELECT tasks.id, tasks.workspace_id, tasks.description FROM {items} \
                 JOIN {imports} ON {imports}.id = {items}.import_id \
                 JOIN tasks ON tasks.id = {items}.orbit_id AND tasks.workspace_id = {imports}.workspace_id \
                 WHERE {items}.import_id = $1 AND {items}.kind = $2"
            );
            let tasks = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&import_id)
                .bind(kind)
                .fetch_all(database.pool())
                .await?;
            for task in tasks {
                let (Ok(task_id), Ok(workspace_id)) = (
                    task.get::<String, _>("id").parse::<Id>(),
                    task.get::<String, _>("workspace_id").parse::<Id>(),
                ) else {
                    continue;
                };
                let description: String = task.get("description");
                for name in source_people(source, &description) {
                    record(database, workspace_id, source, &name, None, task_id).await?;
                }
            }
        }
        sqlx::query("DELETE FROM import_people_backfill WHERE source = $1 AND import_id = $2")
            .bind(&source_name)
            .bind(&import_id)
            .execute(database.pool())
            .await?;
    }
    Ok(())
}

/// The unassigned people an importer named in a task description: Linear's `- Assignee:` and
/// Trello's `- Members:` lines under their bold source heading, Asana's `Asana assignee:` line.
#[must_use]
pub fn source_people(source: Source, markdown: &str) -> Vec<String> {
    let lines: Vec<&str> = markdown.lines().map(str::trim).collect();
    let names: Vec<String> = match source {
        Source::Asana => lines
            .iter()
            .rev()
            .find_map(|line| line.strip_prefix("Asana assignee:"))
            .map(|name| vec![unescape(name.trim())])
            .unwrap_or_default(),
        Source::Linear => detail(&lines, "**Linear**", "Assignee")
            .map(|name| vec![unescape(&name)])
            .unwrap_or_default(),
        Source::Trello => detail(&lines, "**Trello**", "Members")
            .map(|names| {
                names
                    .split(", ")
                    .map(|name| unescape(name.trim()))
                    .collect()
            })
            .unwrap_or_default(),
    };
    let mut people: Vec<String> = Vec::new();
    for name in names {
        let key = name_key(&name);
        if !key.is_empty()
            && !PLACEHOLDERS.contains(&key.as_str())
            && !people.iter().any(|seen| name_key(seen) == key)
        {
            people.push(name);
        }
    }
    people
}

/// The value of the `- {field}: …` item in the list under the last `heading` line (see
/// `task_import::with_source_details`), still escaped.
fn detail(lines: &[&str], heading: &str, field: &str) -> Option<String> {
    let start = lines.iter().rposition(|line| *line == heading)?;
    for line in &lines[start + 1..] {
        if line.is_empty() {
            continue;
        }
        let item = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))?;
        if let Some(value) = item
            .strip_prefix(field)
            .and_then(|rest| rest.strip_prefix(':'))
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// Removes markdown backslash escapes (a backslash before ASCII punctuation).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && next.is_ascii_punctuation()
        {
            out.push(next);
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_import::with_source_details;

    #[test]
    fn reads_linear_assignees() {
        let markdown = with_source_details(
            "Body\n\n**Linear**\n\n- Assignee: Decoy",
            "Linear",
            &[
                ("Issue", "ENG-7".to_owned()),
                ("Assignee", "Ada *the* [Great] #1".to_owned()),
                ("Parent", "ENG-1".to_owned()),
            ],
            &[(
                "PR #1".to_owned(),
                "https://github.com/a/b/pull/1".to_owned(),
            )],
        );
        assert_eq!(
            source_people(Source::Linear, &markdown),
            vec!["Ada *the* [Great] #1".to_owned()]
        );
        assert_eq!(
            source_people(
                Source::Linear,
                "**Linear**\n\n- Issue: ENG-7\n- Assignee: unknown"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            source_people(
                Source::Linear,
                "**Linear**\n\n- Issue: ENG-7\n\n**Links**\n\n- Assignee: Nope"
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn reads_trello_members() {
        let markdown = with_source_details(
            "Card body",
            "Trello",
            &[
                (
                    "Members",
                    "Grace Hopper, Trello member, a_b_, grace hopper".to_owned(),
                ),
                ("Due date", "marked complete".to_owned()),
            ],
            &[],
        );
        assert_eq!(
            source_people(Source::Trello, &markdown),
            vec!["Grace Hopper".to_owned(), "a_b_".to_owned()]
        );
    }

    #[test]
    fn reads_asana_assignees() {
        assert_eq!(
            source_people(
                Source::Asana,
                "Line one\n\nAsana assignee: Grace\n\nAlso in Asana projects: Ops"
            ),
            vec!["Grace".to_owned()]
        );
        // The rich-text path: the paragraph as the blocks' markdown copy escapes it.
        assert_eq!(
            source_people(Source::Asana, "Notes\n\nAsana assignee: Jo \\*J\\* \\[x\\]"),
            vec!["Jo *J* [x]".to_owned()]
        );
        assert_eq!(
            source_people(Source::Asana, "Asana assignee: unknown"),
            Vec::<String>::new()
        );
        assert_eq!(
            source_people(Source::Asana, "No people here"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn matches_by_email_then_mapping() {
        let member = Id::new_v7();
        let mapped_member = Id::new_v7();
        let members = HashMap::from([("ada@example.com".to_owned(), member)]);
        let mapped = HashMap::from([("grace hopper".to_owned(), mapped_member)]);
        assert_eq!(
            match_assignee(&members, &mapped, Some(" Ada@Example.com "), "Grace Hopper"),
            Match::Email(member)
        );
        assert_eq!(
            match_assignee(&members, &mapped, Some("g@example.com"), " grace HOPPER "),
            Match::Mapped(mapped_member)
        );
        assert_eq!(
            match_assignee(&members, &mapped, None, "Linus"),
            Match::Unmatched
        );
    }
}
