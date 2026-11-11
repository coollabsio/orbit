//! Outgoing webhooks: signed HTTP posts of task events to URLs that a workspace owner or admin
//! subscribes (docs/webhooks.md). `audit::record` calls [`capture`] for every task audit event,
//! inside the transaction of the change, so a delivery exists exactly when the change commits.
//! The `webhook.deliver` job ([`delivery`]) sends it, at least once.

pub(crate) mod delivery;
pub mod routes;
mod store;

use orbit_platform::{Id, Job, JobKind, JobStore, JobStoreError, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use crate::repositories::tasks::{TaskError, TaskRecord, webhook_task_in_tx};
use crate::token_api::ReadTask;

pub use delivery::Deliverer;

pub(crate) const DELIVER_JOB: &str = "webhook.deliver";
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[schema(as = WebhookEventType)]
pub enum WebhookEventType {
    #[serde(rename = "task.created")]
    TaskCreated,
    #[serde(rename = "task.updated")]
    TaskUpdated,
    #[serde(rename = "task.deleted")]
    TaskDeleted,
}

impl WebhookEventType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TaskCreated => "task.created",
            Self::TaskUpdated => "task.updated",
            Self::TaskDeleted => "task.deleted",
        }
    }
}

/// What `data.changes` of an event holds.
enum Changes {
    /// The audit metadata's `changes` (field by field, as the activity feed shows them).
    Recorded,
    /// One field, marked `true`; its old and new values may name tasks of other projects.
    Field(&'static str),
}

/// The one map from a task audit action to the event it sends. Actions not listed send nothing:
/// a purge (the task is gone; `task.deleted` was sent when it went to the trash), relation
/// changes, reorders, and the summary row of a bulk update (each task has its own row).
fn task_event(action: &str) -> Option<(WebhookEventType, Changes)> {
    use WebhookEventType::{TaskCreated, TaskDeleted, TaskUpdated};
    Some(match action {
        "task.created" => (TaskCreated, Changes::Recorded),
        "task.updated" => (TaskUpdated, Changes::Recorded),
        "task.restored" => (TaskUpdated, Changes::Field("deleted_at")),
        "task.auto_closed" => (TaskUpdated, Changes::Field("status")),
        "task.parent_changed" => (TaskUpdated, Changes::Field("parent")),
        "task.marked_duplicate" | "task.unmarked_duplicate" => {
            (TaskUpdated, Changes::Field("duplicate_of"))
        }
        "task.deleted" => (TaskDeleted, Changes::Recorded),
        _ => return None,
    })
}

pub(crate) fn job_kind() -> JobKind {
    JobKind::new(DELIVER_JOB).with_workspace_limit(4)
}

/// Queues the job that sends `delivery_id`, and names it as the delivery's sender.
pub(crate) async fn enqueue_in(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    delivery_id: Id,
    now: TimestampMillis,
) -> Result<(), JobStoreError> {
    let job = Job::new(
        job_kind(),
        json!({ "delivery_id": delivery_id.to_string() }),
        now,
    )
    .with_workspace(workspace_id.to_string());
    JobStore::enqueue_in(transaction, &job).await?;
    sqlx::query("UPDATE webhook_deliveries SET job_id = $1 WHERE id = $2")
        .bind(job.id.to_string())
        .bind(delivery_id.to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[derive(Serialize)]
struct Envelope<'a> {
    id: Id,
    #[serde(rename = "type")]
    event_type: WebhookEventType,
    schema_version: u32,
    occurred_at: TimestampMillis,
    workspace_id: Id,
    resource: Resource,
    actor: EventActor<'a>,
    origin: Origin<'a>,
    data: Value,
}

#[derive(Serialize)]
struct Resource {
    #[serde(rename = "type")]
    resource_type: &'static str,
    id: Id,
}

/// Who made the change. A service-account token write has no `user_id`.
#[derive(Serialize)]
struct EventActor<'a> {
    user_id: Option<Id>,
    api_token_id: Option<&'a Value>,
    service_account_id: Option<&'a Value>,
    service_account_name: Option<&'a Value>,
}

#[derive(Serialize)]
struct Origin<'a> {
    /// The audit action, such as `task.auto_closed`.
    action: &'a str,
}

/// `ReadTask` plus the identifier and the parent, which is shown only when the subscription
/// covers the parent's project.
#[derive(Serialize)]
struct EventTask {
    #[serde(flatten)]
    task: ReadTask,
    identifier: String,
    parent_task_id: Option<Id>,
}

/// What one subscription that takes the event may see of it.
enum Scope {
    /// It covers the task's project: the whole task. `from_visible` is false when the task moved
    /// in from a project it does not cover, whose id is then left out.
    Task {
        parent_visible: bool,
        from_visible: bool,
    },
    /// It covers only the project the task moved out of: the task's id and that project.
    MovedOut { from: Id },
}

/// Writes the deliveries of a task audit event, in the transaction that records it. Returns at
/// once when no enabled subscription of the workspace covers the task's project.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn capture(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    task_id: Id,
    action: &str,
    actor_id: Option<Id>,
    metadata: &Value,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    let Some((event_type, changes)) = task_event(action) else {
        return Ok(());
    };
    // A move between projects also reaches the subscriptions of the project it left.
    let moved_from = match changes {
        Changes::Recorded => metadata
            .pointer("/changes/project/from")
            .and_then(Value::as_str)
            .and_then(|id| id.parse::<Id>().ok()),
        Changes::Field(_) => None,
    };
    let rows = sqlx::query(
        "SELECT webhook_subscriptions.id, webhook_subscriptions.event_types, \
         webhook_subscription_projects.project_id, tasks.project_id AS task_project_id FROM tasks \
         JOIN webhook_subscription_projects ON (webhook_subscription_projects.project_id = tasks.project_id \
              OR webhook_subscription_projects.project_id = $3) \
         JOIN webhook_subscriptions ON webhook_subscriptions.id = webhook_subscription_projects.subscription_id \
         WHERE tasks.id = $1 AND webhook_subscriptions.workspace_id = $2 AND webhook_subscriptions.enabled = 1",
    )
    .bind(task_id.to_string())
    .bind(workspace_id.to_string())
    .bind(moved_from.map(|id| id.to_string()))
    .fetch_all(&mut **transaction)
    .await?;
    // Per subscription: whether it covers the task's project, and whether the project it left.
    let mut matches = std::collections::BTreeMap::<Id, (bool, bool)>::new();
    for row in &rows {
        if !store::event_types(&row.get::<String, _>("event_types")).contains(&event_type) {
            continue;
        }
        let covered = row.get::<String, _>("project_id");
        let entry = matches
            .entry(store::parse_id(&row.get::<String, _>("id"))?)
            .or_default();
        if covered == row.get::<String, _>("task_project_id") {
            entry.0 = true;
        } else {
            entry.1 = true;
        }
    }
    if matches.is_empty() {
        return Ok(());
    }
    let Some((task, project_key)) = webhook_task_in_tx(transaction, workspace_id, task_id)
        .await
        .map_err(task_error)?
    else {
        return Ok(());
    };
    let mut targets = Vec::with_capacity(matches.len());
    for (subscription_id, (covers_task, covers_from)) in matches {
        let scope = match moved_from {
            Some(from) if !covers_task => Scope::MovedOut { from },
            _ => Scope::Task {
                parent_visible: match &task.parent {
                    Some(parent) => {
                        store::covers(transaction, subscription_id, parent.project_id).await?
                    }
                    None => false,
                },
                from_visible: moved_from.is_none() || covers_from,
            },
        };
        targets.push((subscription_id, scope));
    }

    let event_id = Id::new_v7();
    let changes = match changes {
        Changes::Recorded => metadata
            .get("changes")
            .cloned()
            .unwrap_or_else(|| json!({})),
        Changes::Field(field) => json!({ field: true }),
    };
    let project_id = task.project_id;
    let identifier = format!("{project_key}-{}", task.number);
    let envelope = |data: Value| Envelope {
        id: event_id,
        event_type,
        schema_version: SCHEMA_VERSION,
        occurred_at: now,
        workspace_id,
        resource: Resource {
            resource_type: "task",
            id: task_id,
        },
        actor: EventActor {
            user_id: actor_id,
            api_token_id: metadata.get("api_token_id"),
            service_account_id: metadata.get("actor_service_account_id"),
            service_account_name: metadata.get("actor_service_account_name"),
        },
        origin: Origin { action },
        data,
    };
    let version = crate::repositories::pages::revision(task.version, task.updated_at);
    let body = |scope: &Scope| -> Result<String, sqlx::Error> {
        let data = match (event_type, scope) {
            (_, Scope::MovedOut { from }) => json!({
                "project_id": from,
                "moved_out": true,
                "task": { "id": task.id, "version": version },
                "changes": { "project": { "from": from, "to": null } },
            }),
            (WebhookEventType::TaskDeleted, Scope::Task { .. }) => json!({
                "project_id": project_id,
                "task": {
                    "id": task.id,
                    "project_id": project_id,
                    "identifier": identifier,
                    "version": version,
                    "deleted_at": task.deleted_at,
                },
            }),
            (
                WebhookEventType::TaskCreated | WebhookEventType::TaskUpdated,
                Scope::Task {
                    parent_visible,
                    from_visible,
                },
            ) => {
                let mut changes = changes.clone();
                // The old project and its status belong to a project this webhook does not select.
                if !from_visible {
                    for field in ["project", "status"] {
                        if let Some(change) = changes.get_mut(field) {
                            change["from"] = Value::Null;
                        }
                    }
                }
                json!({
                    "project_id": project_id,
                    "task": event_task(&task, &identifier, *parent_visible),
                    "changes": changes,
                })
            }
        };
        serde_json::to_string(&envelope(data))
            .map_err(|error| sqlx::Error::Protocol(format!("webhook body: {error}")))
    };
    for (subscription_id, scope) in targets {
        let delivery_id = Id::new_v7();
        // A moved-out delivery belongs to the project it left: the send-time check needs it to
        // still be live and selected.
        let delivery_project = match &scope {
            Scope::MovedOut { from } => *from,
            Scope::Task { .. } => project_id,
        };
        sqlx::query(
            "INSERT INTO webhook_deliveries (id, subscription_id, workspace_id, project_id, event_id, \
             event_type, resource_id, body, state, attempt_count, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending', 0, $9, $10)",
        )
        .bind(delivery_id.to_string())
        .bind(subscription_id.to_string())
        .bind(workspace_id.to_string())
        .bind(delivery_project.to_string())
        .bind(event_id.to_string())
        .bind(event_type.as_str())
        .bind(task_id.to_string())
        .bind(body(&scope)?)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
        enqueue_in(transaction, workspace_id, delivery_id, now)
            .await
            .map_err(|error| match error {
                JobStoreError::Database(error) => error,
                other => sqlx::Error::Protocol(other.to_string()),
            })?;
    }
    Ok(())
}

fn event_task(task: &TaskRecord, identifier: &str, parent_visible: bool) -> EventTask {
    EventTask {
        task: ReadTask::new(task.clone()),
        identifier: identifier.to_owned(),
        parent_task_id: task
            .parent
            .as_ref()
            .filter(|_| parent_visible)
            .map(|parent| parent.id),
    }
}

fn task_error(error: TaskError) -> sqlx::Error {
    match error {
        TaskError::Unavailable(error) => error,
        other => sqlx::Error::Protocol(other.to_string()),
    }
}
