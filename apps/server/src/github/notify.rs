//! GitHub pull request events → Orbit inbox notices (pushed by the inbox service). A GitHub account maps to the
//! Orbit member who connected it in this workspace; members without a connection get nothing, because Orbit cannot
//! know who they are. No pull request content is stored: only the repository, the number and who acted.

use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;

use crate::integration_routes::IntegrationState;
use crate::repositories::project_access::user_sees_project_sql;

/// One notice to write.
struct Notice<'a> {
    recipient_github_id: i64,
    actor_github_id: Option<i64>,
    actor_login: &'a str,
    kind: &'static str,
    review_state: Option<String>,
    dedupe_key: String,
}

/// What a webhook delivery means for the inbox; `None` when it is no notice.
fn notice_of<'a>(event: &str, delivery: &str, payload: &'a Value) -> Option<Notice<'a>> {
    let action = payload["action"].as_str().unwrap_or("");
    let repository = payload["repository"]["full_name"].as_str()?.to_lowercase();
    let pull = &payload["pull_request"];
    let number = pull["number"].as_i64()?;
    let author = pull["user"]["id"].as_i64()?;
    let login = |value: &'a Value| value["login"].as_str().unwrap_or("someone");
    match (event, action) {
        // a review asked of a person (team requests carry `requested_team` instead and are left out)
        ("pull_request", "review_requested") => {
            let reviewer = payload["requested_reviewer"]["id"].as_i64()?;
            Some(Notice {
                recipient_github_id: reviewer,
                actor_github_id: payload["sender"]["id"].as_i64(),
                actor_login: login(&payload["sender"]),
                kind: "github_review_requested",
                review_state: None,
                dedupe_key: format!("github:rr:{repository}#{number}:{reviewer}:{delivery}"),
            })
        }
        // A review on the author's pull request. GitHub sends one for a review with many inline comments, and one for
        // a single comment or a reply too (each is a one-comment review): one notice for each review id.
        ("pull_request_review", "submitted") => {
            let review = &payload["review"];
            let state = match review["state"].as_str()?.to_ascii_lowercase().as_str() {
                "approved" => "approved",
                "changes_requested" => "changes_requested",
                "commented" => "commented",
                _ => return None,
            };
            Some(Notice {
                recipient_github_id: author,
                actor_github_id: review["user"]["id"].as_i64(),
                actor_login: login(&review["user"]),
                kind: "github_review",
                review_state: Some(state.to_owned()),
                dedupe_key: format!("github:review:{}:{author}", review["id"].as_i64()?),
            })
        }
        ("pull_request", "closed") if pull["merged"] == true => {
            let by = if pull["merged_by"].is_object() { &pull["merged_by"] } else { &payload["sender"] };
            Some(Notice {
                recipient_github_id: author,
                actor_github_id: by["id"].as_i64(),
                actor_login: login(by),
                kind: "github_merged",
                review_state: None,
                dedupe_key: format!("github:merged:{repository}#{number}:{author}"),
            })
        }
        _ => None,
    }
}

/// Writes the inbox notice for a pull request event, if it is one. Unknown recipients, members who cannot see a
/// project connected to the repository, and people acting on their own pull request get nothing. A delivery GitHub
/// retries makes no second notice (`dedupe_key`).
pub async fn notify_pull_event(state: &IntegrationState, workspace: &Id, event: &str, delivery: &str, payload: &Value) -> Result<(), sqlx::Error> {
    let Some(notice) = notice_of(event, delivery, payload) else { return Ok(()) };
    if notice.actor_github_id == Some(notice.recipient_github_id) {
        return Ok(());
    }
    let repository = payload["repository"]["full_name"].as_str().unwrap_or_default().to_lowercase();
    let number = payload["pull_request"]["number"].as_i64().unwrap_or_default();
    let pool = state.database().pool();
    let member = |github_id: i64| {
        sqlx::query_scalar::<_, String>("SELECT user_id FROM github_user_connections WHERE workspace_id = $1 AND github_user_id = $2")
            .bind(workspace.to_string())
            .bind(github_id)
            .fetch_optional(pool)
    };
    let Some(recipient) = member(notice.recipient_github_id).await? else { return Ok(()) };
    let actor = match notice.actor_github_id {
        Some(id) => member(id).await?,
        None => None,
    };
    let sees: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM github_project_connections c JOIN projects p ON p.id = c.project_id \
         WHERE c.workspace_id = $1 AND lower(c.repository) = $2 AND p.deleted_at IS NULL AND {}",
        user_sees_project_sql("p.id", "$3")
    )))
    .bind(workspace.to_string())
    .bind(&repository)
    .bind(&recipient)
    .fetch_one(pool)
    .await?;
    if sees == 0 {
        return Ok(());
    }
    let mut tx = state.database().immediate_transaction().await?;
    sqlx::query(
        "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, dedupe_key, created_at, \
         github_repository, github_pull_number, github_actor_login, github_review_state) \
         SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11 \
         WHERE EXISTS (SELECT 1 FROM memberships WHERE workspace_id = $12 AND user_id = $13) \
         ON CONFLICT (dedupe_key) DO NOTHING",
    )
    .bind(Id::new_v7().to_string())
    .bind(workspace.to_string())
    .bind(&recipient)
    // the actor column needs an Orbit user: the recipient stands in for a GitHub account without one
    .bind(actor.as_deref().unwrap_or(&recipient))
    .bind(notice.kind)
    // `dedupe_key` is unique across workspaces; two workspaces can connect the same repository
    .bind(format!("{workspace}:{}", notice.dedupe_key))
    .bind(TimestampMillis::now().as_millis())
    .bind(&repository)
    .bind(number)
    .bind(notice.actor_login)
    .bind(notice.review_state.as_deref())
    .bind(workspace.to_string())
    .bind(&recipient)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// The pull requests a webhook delivery changes for an open viewer: `owner/repo#n`, lower-case. Check runs are left
/// out: their check suite reports the same change once, not once for each CI job.
#[must_use]
pub fn changed_pulls(event: &str, payload: &Value) -> Vec<String> {
    let Some(repository) = payload["repository"]["full_name"].as_str().map(str::to_lowercase) else { return Vec::new() };
    let action = payload["action"].as_str().unwrap_or("");
    let numbers: Vec<i64> = match (event, action) {
        ("pull_request", "synchronize" | "edited" | "closed" | "reopened" | "ready_for_review" | "converted_to_draft")
        | ("pull_request_review", _)
        | ("pull_request_review_comment", _)
        | ("pull_request_review_thread", _) => payload["pull_request"]["number"].as_i64().into_iter().collect(),
        ("check_suite", "completed") => payload["check_suite"]["pull_requests"].as_array().map(|list| list.iter().filter_map(|pull| pull["number"].as_i64()).collect()).unwrap_or_default(),
        _ => Vec::new(),
    };
    numbers.into_iter().take(20).map(|number| format!("{repository}#{number}")).collect()
}

/// Tells the workspace's open viewers which pull requests changed: one `github_pull` audit row each, which the
/// realtime stream reports in `github_pulls`. Only for repositories connected to a project of the workspace.
pub async fn record_pull_changes(state: &IntegrationState, workspace: &Id, pulls: &[String], request_id: &str) -> Result<(), sqlx::Error> {
    let Some(repository) = pulls.first().and_then(|pull| pull.split('#').next()) else { return Ok(()) };
    let connected: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM github_project_connections WHERE workspace_id = $1 AND lower(repository) = $2")
        .bind(workspace.to_string())
        .bind(repository)
        .fetch_one(state.database().pool())
        .await?;
    if connected == 0 {
        return Ok(());
    }
    let mut tx = state.database().immediate_transaction().await?;
    for pull in pulls {
        crate::audit::record(
            &mut tx,
            *workspace,
            None,
            "github.pull.changed",
            crate::audit::AuditOutcome::Success,
            "github_pull",
            None,
            request_id,
            serde_json::json!({ "pull": pull }),
            TimestampMillis::now(),
        )
        .await?;
    }
    tx.commit().await
}

/// `true` for the events that can make a notice (the webhook calls `notify_pull_event` for them).
#[must_use]
pub fn is_notice_event(event: &str) -> bool {
    matches!(event, "pull_request" | "pull_request_review")
}
