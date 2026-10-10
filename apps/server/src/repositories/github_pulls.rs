//! Pull requests that refer to tasks: the links, and the status automation of the task's
//! project. A pull request links to a task when it names the task's identifier in its branch,
//! in its title, or after a keyword in its body.

use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};
use utoipa::ToSchema;

use super::sub_issues;
use super::task_notifications::{self, TaskEvent};
use super::tasks::{
    TaskError, TaskRepository, github_service_account_in_tx, parse_id, parse_task_identifier,
    record_mutation, require_project_tx,
};
use crate::audit::{self, AuditOutcome};

/// A pull request can link to this many tasks; more references are ignored.
const MAX_LINKED_TASKS: usize = 20;

const CLOSING_KEYWORDS: [&str; 9] = [
    "fixes", "fixed", "fix", "closes", "closed", "close", "resolves", "resolved", "resolve",
];
const MENTION_KEYWORDS: [&str; 5] = ["references", "refs", "ref", "part of", "related to"];

/// The events a project has a rule for, and the status category of the default rule (`None`:
/// no change).
const PR_EVENTS: [(&str, Option<&str>); 4] = [
    ("draft", None),
    ("open", Some("started")),
    ("review", None),
    ("merged", Some("completed")),
];

/// A pull request as a webhook delivery describes it.
#[derive(Clone, Debug)]
pub struct GithubPull {
    pub repository: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub branch: String,
    pub body: String,
    /// `draft`, `open`, `in_review`, `merged` or `closed`.
    pub state: &'static str,
    /// The author is an owner, member or collaborator of the repository.
    pub trusted: bool,
}

/// A place where a pull request names a task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskReference {
    /// The identifiers the text can mean, the most specific first: a project key can contain
    /// `_`, so `feature_eng-12` is `FEATURE_ENG-12` or `ENG-12`.
    pub candidates: Vec<String>,
    /// The pull request closes the task (branch, title, or a closing keyword).
    pub closes: bool,
}

const fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte >= 0x80
}

/// Every `KEY-number` in `text` whose neighbours are not a letter or a digit, as
/// (start, end, candidates).
fn identifiers(text: &str) -> Vec<(usize, usize, Vec<String>)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for dash in (0..bytes.len()).filter(|index| bytes[*index] == b'-') {
        let mut end = dash + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == dash + 1 || bytes.get(end).is_some_and(|byte| is_word(*byte)) {
            continue;
        }
        let mut start = dash;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }
        if start > 0 && bytes[start - 1] >= 0x80 {
            continue;
        }
        let run = &text[start..dash];
        let number = &text[dash..end];
        // The whole run, then what follows each `_`.
        let candidates: Vec<String> = std::iter::once(0)
            .chain(run.match_indices('_').map(|(index, _)| index + 1))
            .filter_map(|from| parse_task_identifier(&format!("{}{number}", &run[from..])))
            .map(|(key, number)| format!("{key}-{number}"))
            .collect();
        if !candidates.is_empty() {
            found.push((start, end, candidates));
        }
    }
    found
}

/// The tasks a pull request names. Every identifier in the branch name and in the title
/// closes its task. In the body an identifier counts only after a keyword: `Fixes`, `Closes`,
/// `Resolves` (closing), or `Refs`, `References`, `Part of`, `Related to` (not closing). A
/// keyword can be followed by more than one identifier.
#[must_use]
pub fn scan_task_references(branch: &str, title: &str, body: &str) -> Vec<TaskReference> {
    let mut references: Vec<TaskReference> = identifiers(branch)
        .into_iter()
        .chain(identifiers(title))
        .map(|(_, _, candidates)| TaskReference {
            candidates,
            closes: true,
        })
        .collect();
    let found = identifiers(body);
    let lower = body.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for at in 0..bytes.len() {
        if at > 0 && is_word(bytes[at - 1]) {
            continue;
        }
        let keyword = |keywords: &[&str]| {
            keywords.iter().find_map(|keyword| {
                let end = at + keyword.len();
                (bytes[at..].starts_with(keyword.as_bytes())
                    && !bytes.get(end).is_some_and(|byte| is_word(*byte)))
                .then_some(end)
            })
        };
        let (mut cursor, closes) = match (keyword(&CLOSING_KEYWORDS), keyword(&MENTION_KEYWORDS)) {
            (Some(end), _) => (end, true),
            (None, Some(end)) => (end, false),
            (None, None) => continue,
        };
        loop {
            // Between the keyword and an identifier, and between identifiers.
            loop {
                if matches!(bytes.get(cursor), Some(b' ' | b'\t' | b':' | b',' | b'&')) {
                    cursor += 1;
                } else if bytes[cursor.min(bytes.len())..].starts_with(b"and ") {
                    cursor += 4;
                } else {
                    break;
                }
            }
            let Some((_, end, candidates)) = found.iter().find(|(start, ..)| *start == cursor)
            else {
                break;
            };
            references.push(TaskReference {
                candidates: candidates.clone(),
                closes,
            });
            cursor = *end;
        }
    }
    references
}

/// How a project answers one pull request event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrAutomationMode {
    /// The rule of the event when the project has none of its own.
    Default,
    /// The event changes no status.
    None,
    /// The task moves to `status_id`.
    Status,
}

/// The rule of a project for one pull request event (`draft`, `open`, `review`, `merged`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PrAutomationRule {
    pub event: String,
    pub mode: PrAutomationMode,
    /// Set when `mode` is `status`: a status of the project.
    #[schema(value_type = Option<String>)]
    pub status_id: Option<Id>,
}

impl TaskRepository {
    /// Makes the links of the pull request match the tasks it names now, then lets a trusted
    /// pull request move those tasks by the rules of their projects. GitHub is the actor.
    pub async fn sync_github_pull_links(
        &self,
        workspace_id: Id,
        actor_id: Id,
        pull: &GithubPull,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), TaskError> {
        // The tasks, in the order they are named; a closing reference wins.
        let mut targets: Vec<(Id, bool)> = Vec::new();
        for reference in scan_task_references(&pull.branch, &pull.title, &pull.body) {
            for candidate in &reference.candidates {
                match self.resolve_task_id(workspace_id, candidate).await {
                    Ok(task_id) => {
                        match targets.iter_mut().find(|(id, _)| *id == task_id) {
                            Some(target) => target.1 |= reference.closes,
                            None => targets.push((task_id, reference.closes)),
                        }
                        break;
                    }
                    Err(TaskError::NotFound) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        let mut tx = self.database().immediate_transaction().await?;
        let existing = sqlx::query(
            "SELECT task_id, state, closes, trusted FROM github_pull_links \
             WHERE workspace_id = ? AND repository = ? AND pull_number = ?",
        )
        .bind(workspace_id.to_string())
        .bind(&pull.repository)
        .bind(pull.number)
        .fetch_all(&mut *tx)
        .await?;
        if targets.is_empty() && existing.is_empty() {
            return Ok(());
        }
        // A pull request never links to the task that it is itself the source of.
        let own: Option<String> = sqlx::query_scalar(
            "SELECT task_id FROM github_issue_links WHERE workspace_id = ? AND repository = ? \
             AND issue_number = ? AND kind = 'pull_request'",
        )
        .bind(workspace_id.to_string())
        .bind(&pull.repository)
        .bind(pull.number)
        .fetch_optional(&mut *tx)
        .await?;
        let mut live = Vec::with_capacity(targets.len());
        for (task_id, closes) in targets {
            let is_live: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM tasks JOIN projects ON projects.id = tasks.project_id \
                 WHERE tasks.id = ? AND tasks.workspace_id = ? AND tasks.deleted_at IS NULL \
                 AND projects.deleted_at IS NULL)",
            )
            .bind(task_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if is_live && own.as_deref() != Some(task_id.to_string().as_str()) {
                live.push((task_id, closes));
            }
        }
        live.truncate(MAX_LINKED_TASKS);
        let (service_account_id, service_account_name) =
            github_service_account_in_tx(&mut tx, workspace_id, actor_id, now).await?;
        let github = GithubActor {
            workspace_id,
            actor_id,
            service_account: (service_account_id, &service_account_name),
            request_id,
            now,
        };
        for row in &existing {
            let task_id = parse_id(row.get("task_id"))?;
            if live.iter().any(|(id, _)| *id == task_id) {
                continue;
            }
            sqlx::query(
                "DELETE FROM github_pull_links WHERE workspace_id = ? AND repository = ? \
                 AND pull_number = ? AND task_id = ?",
            )
            .bind(workspace_id.to_string())
            .bind(&pull.repository)
            .bind(pull.number)
            .bind(task_id.to_string())
            .execute(&mut *tx)
            .await?;
            record_link_audit(&mut tx, github, "task.pull_request_unlinked", task_id, pull).await?;
        }
        // Every link is written before a status moves: the "last closing pull request" rule
        // reads the links.
        let mut moved = Vec::new();
        for (task_id, closes) in &live {
            let before = existing
                .iter()
                .find(|row| row.get::<String, _>("task_id") == task_id.to_string());
            sqlx::query(
                "INSERT INTO github_pull_links (workspace_id, repository, pull_number, task_id, title, \
                 url, branch, state, closes, trusted, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (workspace_id, repository, pull_number, task_id) DO UPDATE SET \
                 title = excluded.title, url = excluded.url, branch = excluded.branch, \
                 state = excluded.state, closes = excluded.closes, trusted = excluded.trusted, \
                 updated_at = excluded.updated_at",
            )
            .bind(workspace_id.to_string())
            .bind(&pull.repository)
            .bind(pull.number)
            .bind(task_id.to_string())
            .bind(&pull.title)
            .bind(&pull.url)
            .bind(&pull.branch)
            .bind(pull.state)
            .bind(closes)
            .bind(pull.trusted)
            .bind(now.as_millis())
            .execute(&mut *tx)
            .await?;
            if before.is_none() {
                record_link_audit(&mut tx, github, "task.pull_request_linked", *task_id, pull)
                    .await?;
            }
            // Only a change of the link moves the task: a later delivery with the same state
            // (a push to the branch) leaves a status that a person set since.
            let changed = before.is_none_or(|row| {
                row.get::<String, _>("state") != pull.state
                    || row.get::<bool, _>("closes") != *closes
                    || row.get::<bool, _>("trusted") != pull.trusted
            });
            if changed && pull.trusted {
                moved.push((*task_id, *closes));
            }
        }
        for (task_id, closes) in moved {
            apply_pull_state_in_tx(&mut tx, github, task_id, closes, pull).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The rule of the project for each pull request event.
    pub async fn pr_automation(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
    ) -> Result<Vec<PrAutomationRule>, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        let rules = pr_automation_in_tx(&mut tx, project_id).await?;
        tx.commit().await?;
        Ok(rules)
    }

    /// Sets the rules of the events that `rules` names; the other events stay as they are.
    pub async fn set_pr_automation(
        &self,
        workspace_id: Id,
        project_id: Id,
        actor_id: Id,
        rules: &[PrAutomationRule],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Vec<PrAutomationRule>, TaskError> {
        let mut tx = self.database().immediate_transaction().await?;
        require_project_tx(&mut tx, workspace_id, project_id, actor_id).await?;
        for rule in rules {
            if !PR_EVENTS.iter().any(|(event, _)| *event == rule.event) {
                return Err(TaskError::Invalid { field: "event" });
            }
            let status_id = match (rule.mode, rule.status_id) {
                (PrAutomationMode::Status, Some(status_id)) => {
                    // The Duplicate status is entered only by marking a duplicate.
                    let valid: bool = sqlx::query_scalar(
                        "SELECT EXISTS (SELECT 1 FROM task_statuses WHERE id = ? AND workspace_id = ? \
                         AND project_id = ? AND category <> 'duplicate')",
                    )
                    .bind(status_id.to_string())
                    .bind(workspace_id.to_string())
                    .bind(project_id.to_string())
                    .fetch_one(&mut *tx)
                    .await?;
                    if !valid {
                        return Err(TaskError::Invalid { field: "status_id" });
                    }
                    Some(status_id)
                }
                (PrAutomationMode::Default | PrAutomationMode::None, None) => None,
                _ => return Err(TaskError::Invalid { field: "status_id" }),
            };
            sqlx::query("DELETE FROM project_pr_automation WHERE project_id = ? AND event = ?")
                .bind(project_id.to_string())
                .bind(&rule.event)
                .execute(&mut *tx)
                .await?;
            if rule.mode != PrAutomationMode::Default {
                sqlx::query(
                    "INSERT INTO project_pr_automation (project_id, event, status_id) VALUES (?, ?, ?)",
                )
                .bind(project_id.to_string())
                .bind(&rule.event)
                .bind(status_id.map(|id| id.to_string()))
                .execute(&mut *tx)
                .await?;
            }
        }
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "project.updated",
            "project",
            project_id,
            request_id,
            now,
        )
        .await?;
        let rules = pr_automation_in_tx(&mut tx, project_id).await?;
        tx.commit().await?;
        Ok(rules)
    }
}

async fn pr_automation_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Id,
) -> Result<Vec<PrAutomationRule>, TaskError> {
    let rows =
        sqlx::query("SELECT event, status_id FROM project_pr_automation WHERE project_id = ?")
            .bind(project_id.to_string())
            .fetch_all(&mut **tx)
            .await?;
    PR_EVENTS
        .iter()
        .map(|(event, _)| {
            let stored = rows
                .iter()
                .find(|row| row.get::<String, _>("event") == *event)
                .map(|row| row.get::<Option<String>, _>("status_id"));
            let (mode, status_id) = match stored {
                None => (PrAutomationMode::Default, None),
                Some(None) => (PrAutomationMode::None, None),
                Some(Some(id)) => (PrAutomationMode::Status, Some(parse_id(id)?)),
            };
            Ok(PrAutomationRule {
                event: (*event).to_owned(),
                mode,
                status_id,
            })
        })
        .collect()
}

/// GitHub acting through its service account.
#[derive(Clone, Copy)]
struct GithubActor<'a> {
    workspace_id: Id,
    /// The workspace owner: the service account acts in their name where a user is needed.
    actor_id: Id,
    service_account: (Id, &'a str),
    request_id: &'a str,
    now: TimestampMillis,
}

async fn record_link_audit(
    tx: &mut Transaction<'_, Sqlite>,
    github: GithubActor<'_>,
    action: &str,
    task_id: Id,
    pull: &GithubPull,
) -> Result<(), TaskError> {
    audit::record(
        tx,
        github.workspace_id,
        None,
        action,
        AuditOutcome::Success,
        "task",
        Some(task_id),
        github.request_id,
        json!({
            "repository": pull.repository,
            "number": pull.number,
            // Titles stay short so the event fits the audit size cap.
            "title": pull.title.chars().take(200).collect::<String>(),
            "actor_service_account_id": github.service_account.0,
            "actor_service_account_name": github.service_account.1,
        }),
        github.now,
    )
    .await?;
    Ok(())
}

/// Moves the task to the status that its project sets for the state of the pull request.
async fn apply_pull_state_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    github: GithubActor<'_>,
    task_id: Id,
    closes: bool,
    pull: &GithubPull,
) -> Result<(), TaskError> {
    let (event, default_category) = match pull.state {
        "draft" => PR_EVENTS[0],
        "open" => PR_EVENTS[1],
        "in_review" => PR_EVENTS[2],
        "merged" => PR_EVENTS[3],
        // Closed without a merge: the work possibly continues in a different pull request.
        _ => return Ok(()),
    };
    let task = sqlx::query(
        "SELECT tasks.status_id, tasks.project_id, task_statuses.category FROM tasks \
         JOIN task_statuses ON task_statuses.id = tasks.status_id WHERE tasks.id = ?",
    )
    .bind(task_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    // A late pull request must not reopen finished work.
    if matches!(
        task.get::<String, _>("category").as_str(),
        "completed" | "cancelled" | "duplicate"
    ) {
        return Ok(());
    }
    if event == "merged" {
        // A task with two closing pull requests is done when the last one merges.
        let other_open: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM github_pull_links WHERE task_id = ? AND closes = 1 \
             AND trusted = 1 AND state IN ('draft', 'open', 'in_review') \
             AND NOT (repository = ? AND pull_number = ?))",
        )
        .bind(task_id.to_string())
        .bind(&pull.repository)
        .bind(pull.number)
        .fetch_one(&mut **tx)
        .await?;
        if !closes || other_open {
            return Ok(());
        }
    }
    let project_id: String = task.get("project_id");
    let rule: Option<Option<String>> = sqlx::query_scalar(
        "SELECT status_id FROM project_pr_automation WHERE project_id = ? AND event = ?",
    )
    .bind(&project_id)
    .bind(event)
    .fetch_optional(&mut **tx)
    .await?;
    let target: Option<String> = match (rule, default_category) {
        (Some(status_id), _) => status_id,
        (None, Some(category)) => {
            sqlx::query_scalar(
                "SELECT id FROM task_statuses WHERE workspace_id = ? AND project_id = ? \
                 AND category = ? ORDER BY position, id LIMIT 1",
            )
            .bind(github.workspace_id.to_string())
            .bind(&project_id)
            .bind(category)
            .fetch_optional(&mut **tx)
            .await?
        }
        (None, None) => None,
    };
    let current: String = task.get("status_id");
    let Some(target) = target.filter(|target| *target != current) else {
        return Ok(());
    };
    let before = sub_issues::snapshot_in_tx(tx, task_id).await?;
    sqlx::query(
        "UPDATE tasks SET status_id = ?, version = version + 1, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&target)
    .bind(github.now.as_millis())
    .bind(task_id.to_string())
    .execute(&mut **tx)
    .await?;
    audit::record(
        tx,
        github.workspace_id,
        None,
        "task.updated",
        AuditOutcome::Success,
        "task",
        Some(task_id),
        github.request_id,
        json!({
            "changes": { "status": { "from": current, "to": target } },
            "actor_service_account_id": github.service_account.0,
            "actor_service_account_name": github.service_account.1,
        }),
        github.now,
    )
    .await?;
    task_notifications::status_changed_in_tx(
        tx,
        TaskEvent {
            workspace_id: github.workspace_id,
            actor: None,
            task_id,
            now: github.now,
        },
        parse_id(current)?,
        parse_id(target)?,
        &[],
    )
    .await?;
    let after = sub_issues::snapshot_in_tx(tx, task_id).await?;
    if let (Some(before), Some(after)) = (before, after) {
        let actor = sub_issues::AutomationActor {
            workspace_id: github.workspace_id,
            actor_id: github.actor_id,
            service_account: Some(github.service_account),
            request_id: github.request_id,
            now: github.now,
        };
        // The webhook has no caller to tell which tasks the automation closed.
        let mut auto_closed = Vec::new();
        sub_issues::run_automation_in_tx(tx, actor, task_id, &before, &after, &mut auto_closed)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first candidate of each reference, with `!` for a closing one.
    fn scan(branch: &str, title: &str, body: &str) -> Vec<String> {
        scan_task_references(branch, title, body)
            .into_iter()
            .map(|reference| {
                format!(
                    "{}{}",
                    reference.candidates[0],
                    if reference.closes { "!" } else { "" }
                )
            })
            .collect()
    }

    #[test]
    fn a_branch_or_title_identifier_closes_its_task() {
        assert_eq!(scan("feature/eng-12-fix-login", "", ""), ["ENG-12!"]);
        assert_eq!(scan("eng-12_fix", "", ""), ["ENG-12!"]);
        assert_eq!(scan("ENG-12", "", ""), ["ENG-12!"]);
        assert_eq!(
            scan("", "Fix login (ENG-12, ops-3)", ""),
            ["ENG-12!", "OPS-3!"]
        );
        // The neighbours of an identifier are not a letter or a digit.
        assert_eq!(scan("xeng-12", "", ""), ["XENG-12!"]);
        assert!(scan("eng-123a", "eng-12é eng- 12 eng-0 -12 eng12", "").is_empty());
        // Accepted: a branch such as `utf-8-fix` names UTF-8.
        assert_eq!(scan("utf-8-fix", "", ""), ["UTF-8!"]);
    }

    #[test]
    fn a_key_can_follow_an_underscore() {
        let references = scan_task_references("feature_eng-12", "", "");
        assert_eq!(references[0].candidates, ["FEATURE_ENG-12", "ENG-12"]);
    }

    #[test]
    fn a_body_identifier_needs_a_keyword() {
        assert!(scan("", "", "See ENG-12 for the details.").is_empty());
        for keyword in [
            "Fixes", "fix", "FIXED", "Closes", "close", "closed", "Resolves", "resolve", "resolved",
        ] {
            assert_eq!(
                scan("", "", &format!("{keyword} ENG-12")),
                ["ENG-12!"],
                "{keyword}"
            );
        }
        for keyword in ["Ref", "refs", "References", "Part of", "related to"] {
            assert_eq!(
                scan("", "", &format!("{keyword}: ENG-12.")),
                ["ENG-12"],
                "{keyword}"
            );
        }
        // More than one identifier after a keyword; the list ends at other text.
        assert_eq!(
            scan(
                "",
                "",
                "Fixes ENG-12, ENG-13 and OPS-2 but not ENG-99.\nRefs ENG-14"
            ),
            ["ENG-12!", "ENG-13!", "OPS-2!", "ENG-14"]
        );
        // A keyword is a whole word, and the identifier follows it directly.
        assert!(scan("", "", "prefix ENG-12, fixes the ENG-13 bug, Fixes\nENG-14").is_empty());
    }
}
