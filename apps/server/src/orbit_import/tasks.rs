//! The task side of an import: teamspaces (matched by name, else created), projects with their
//! statuses and labels, then the tasks (parents first) with their attachments, comments and
//! relations. Only in a full import.

use std::collections::HashMap;

use orbit_platform::{Flag, Id, TimestampMillis};

use super::archive::{self, Archive};
use super::model::{self, Task};
use super::{Item, OrbitImportService, Run};
use crate::notion::files::sanitize_file_name;
use crate::notion::import::clamp;
use crate::repositories::task_relations::NewTaskRelationType;
use crate::repositories::tasks::{
    CommentContent, CreateTask, ImportedTask, TaskChanges, TaskError, TaskUpdate,
};
use crate::repositories::teamspaces::TeamspaceError;
use crate::task_import::{
    Attached, Failure, LABEL_COLORS, NewProject, PROJECT_COLORS, attach_once, attachment_url,
    category_color, display_instant, ensure_project, escape_markdown, find_or_create_label,
    mentions_any, parse_instant_millis, project_key_base, project_statuses, rewrite_links,
    with_source_details,
};

/// Sub-task levels kept below a root; a deeper task (or a parent cycle) starts a new tree.
const MAX_DEPTH: usize = 50;

/// The Orbit side of one imported project.
pub(super) struct ProjectContext {
    project_id: Id,
    /// Source status id → status id.
    statuses: HashMap<String, Id>,
    /// Lowercase status name → status id, for a task whose status is not in `workspace.json`.
    statuses_by_name: HashMap<String, Id>,
    open_status: Id,
    /// Source label id → label id.
    labels: HashMap<String, Id>,
}

/// Lists the live tasks of the selected projects, parents before sub-tasks. Blocking reads.
pub(super) async fn plan_tasks(run: &Run) -> Result<Vec<Item>, Failure> {
    let selected = run.selection.project_ids.clone();
    let listed = archive::blocking(&run.archive, move |archive| {
        let mut tasks = Vec::new();
        for path in archive.paths("tasks/", ".json") {
            let task: Task = archive.read_json(&path)?;
            if task.deleted || task.id.is_empty() || !selected.contains(&task.project_id) {
                continue;
            }
            tasks.push((
                task.id,
                task.parent_task_id,
                path,
                task_label(&task.key, &task.title),
            ));
        }
        Ok(tasks)
    })
    .await?;
    let index: HashMap<&str, usize> = listed
        .iter()
        .enumerate()
        .map(|(position, (id, ..))| (id.as_str(), position))
        .collect();
    let parent_of = |position: usize| {
        listed[position]
            .1
            .as_deref()
            .and_then(|parent| index.get(parent).copied())
    };
    let mut depth: Vec<Option<usize>> = vec![None; listed.len()];
    for start in 0..listed.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(position) = current {
            if depth[position].is_some() || chain.contains(&position) {
                break;
            }
            chain.push(position);
            current = parent_of(position);
        }
        let mut base = current.and_then(|position| depth[position]);
        for position in chain.into_iter().rev() {
            let level = match (base, parent_of(position)) {
                (Some(parent), Some(_)) if parent < MAX_DEPTH => parent + 1,
                _ => 0,
            };
            depth[position] = Some(level);
            base = Some(level);
        }
    }
    let mut order: Vec<usize> = (0..listed.len()).collect();
    order.sort_by_key(|position| {
        (
            depth[*position].unwrap_or_default(),
            listed[*position].2.clone(),
        )
    });
    Ok(order
        .into_iter()
        .map(|position| {
            let (id, _, path, title) = &listed[position];
            let parent = (depth[position].unwrap_or_default() > 0)
                .then(|| parent_of(position))
                .flatten()
                .map(|parent| listed[parent].0.clone());
            Item {
                kind: "task",
                source_id: id.clone(),
                parent_id: parent,
                path: path.clone(),
                title: clamp(title, 500, 2_000),
                orbit_id: None,
                state: "pending".to_owned(),
            }
        })
        .collect())
}

fn task_label(key: &str, title: &str) -> String {
    match (key.trim(), title.trim()) {
        ("", title) => title.to_owned(),
        (key, "") => key.to_owned(),
        (key, title) => format!("{key} {title}"),
    }
}

/// Teamspaces and projects (with statuses and labels) of the selection.
pub(super) async fn import_structure(
    service: &OrbitImportService,
    run: &mut Run,
    workspace: &model::Workspace,
) -> Result<(), Failure> {
    for mut item in service.load_items(run.import_id, "teamspace").await? {
        if item.state == "filled" || item.state == "failed" {
            continue;
        }
        let teamspace = workspace
            .teamspaces
            .iter()
            .find(|teamspace| teamspace.id == item.source_id)
            .cloned();
        service
            .step(run, &mut item, async |service, run, item| {
                let teamspace = teamspace.as_ref().ok_or_else(|| {
                    Failure::Item("The teamspace is not in the archive.".to_owned())
                })?;
                ensure_teamspace(service, run, item, teamspace).await
            })
            .await?;
    }
    for (index, mut item) in service
        .load_items(run.import_id, "project")
        .await?
        .into_iter()
        .enumerate()
    {
        if item.state == "filled" || item.state == "failed" {
            continue;
        }
        let project = workspace
            .projects
            .iter()
            .find(|project| project.id == item.source_id)
            .cloned();
        service
            .step(run, &mut item, async |service, run, item| {
                let project = project.as_ref().ok_or_else(|| {
                    Failure::Item("The project is not in the archive.".to_owned())
                })?;
                ensure_project_structure(service, run, item, project, index).await
            })
            .await?;
    }
    Ok(())
}

/// The teamspace an earlier import made, else one here with the same name, else a new one.
async fn ensure_teamspace(
    service: &OrbitImportService,
    run: &mut Run,
    item: &mut Item,
    teamspace: &model::Teamspace,
) -> Result<(), Failure> {
    let existing = match item.orbit_id {
        Some(id) => Some(id),
        None => service.linked(run, "teamspace", &teamspace.id).await?,
    };
    let id = match existing {
        Some(id) => id,
        None => {
            let name = clamp(teamspace.name.trim(), 100, 400);
            let same_name: Option<String> = sqlx::query_scalar(
                "SELECT id FROM teamspaces WHERE workspace_id = $1 AND lower(name) = lower($2) ORDER BY position LIMIT 1",
            )
            .bind(run.workspace_id.to_string())
            .bind(&name)
            .fetch_optional(service.database().pool())
            .await?;
            match same_name.and_then(|id| id.parse().ok()) {
                Some(id) => id,
                None => {
                    let created = service
                        .inner
                        .teamspaces
                        .create_teamspace(
                            run.workspace_id,
                            run.user_id,
                            if name.is_empty() {
                                "Imported".to_owned()
                            } else {
                                name
                            },
                            teamspace.icon.clone(),
                            &run.request_id,
                            TimestampMillis::now(),
                        )
                        .await
                        .map_err(|error| match error {
                            TeamspaceError::Unavailable(error) => Failure::from(error),
                            TeamspaceError::NotFound => Failure::Fatal(
                                "The importing member left the workspace.".to_owned(),
                            ),
                            other => {
                                Failure::Item(format!("Orbit refused the teamspace: {other}."))
                            }
                        })?;
                    created.id
                }
            }
        }
    };
    if item.orbit_id.is_none() {
        sqlx::query(
            "UPDATE orbit_import_items SET orbit_id = $1 WHERE import_id = $2 AND kind = 'teamspace' AND source_id = $3",
        )
        .bind(id.to_string())
        .bind(run.import_id.to_string())
        .bind(&teamspace.id)
        .execute(service.database().pool())
        .await?;
        service
            .record_link(run, "teamspace", &teamspace.id, id)
            .await?;
        item.orbit_id = Some(id);
    }
    service.set_item_state(run, item, "filled").await
}

/// The project under its recorded id, then a status for each source status (same name reused)
/// and a label for each source label.
async fn ensure_project_structure(
    service: &OrbitImportService,
    run: &mut Run,
    item: &mut Item,
    project: &model::Project,
    index: usize,
) -> Result<(), Failure> {
    let (project_id, existing) = service.item_id(run, item).await?;
    if !existing {
        let key_source = if project.key.trim().is_empty() {
            &project.name
        } else {
            &project.key
        };
        ensure_project(
            service.database(),
            &service.inner.tasks,
            NewProject {
                id: project_id,
                workspace_id: run.workspace_id,
                user_id: run.user_id,
                name: &project.name,
                fallback_name: "Imported project",
                key_base: project_key_base(key_source, "IMP"),
                color: if is_hex_color(&project.color) {
                    &project.color
                } else {
                    PROJECT_COLORS[index % PROJECT_COLORS.len()]
                },
                request_id: &run.request_id,
            },
        )
        .await?;
        run.report.created.projects += 1;
    }
    service.set_item_state(run, item, "created").await?;

    let mut statuses = project_statuses(service.database(), project_id).await?;
    for status in &project.statuses {
        if service.linked(run, "status", &status.id).await?.is_some() {
            continue;
        }
        let name = clamp(status.name.trim(), 100, 100);
        let name = if name.is_empty() {
            "Untitled status".to_owned()
        } else {
            name
        };
        let category = match status.category.as_str() {
            "started" => "started",
            "completed" => "completed",
            // Orbit's Duplicate status is entered only through a duplicate relation.
            "cancelled" | "duplicate" => "cancelled",
            _ => "unstarted",
        };
        let same_name = statuses
            .iter()
            .find(|(_, existing, existing_category)| {
                existing_category != "duplicate" && same_status_name(existing, &name)
            })
            .map(|(id, _, _)| *id);
        let same_name = match (same_name, status.category.as_str()) {
            (None, "duplicate") => statuses
                .iter()
                .find(|(_, _, category)| category == "cancelled")
                .map(|(id, _, _)| *id),
            (found, _) => found,
        };
        let status_id = match same_name {
            Some(id) => id,
            None => {
                let color = if is_hex_color(&status.color) {
                    status.color.clone()
                } else {
                    category_color(category).to_owned()
                };
                let created = service
                    .inner
                    .tasks
                    .create_status(
                        run.workspace_id,
                        project_id,
                        run.user_id,
                        name.clone(),
                        clamp(&status.description, 500, 2_000),
                        color,
                        category.to_owned(),
                        None,
                        &run.request_id,
                        TimestampMillis::now(),
                    )
                    .await?;
                statuses.push((created.id, name, category.to_owned()));
                created.id
            }
        };
        service
            .record_link(run, "status", &status.id, status_id)
            .await?;
    }
    for (position, label) in project.labels.iter().enumerate() {
        if service.linked(run, "label", &label.id).await?.is_some() {
            continue;
        }
        let name = clamp(label.name.trim(), 100, 100);
        if name.is_empty() {
            continue;
        }
        let color = if is_hex_color(&label.color) {
            label.color.as_str()
        } else {
            LABEL_COLORS[position % LABEL_COLORS.len()]
        };
        let label_id = find_or_create_label(
            service.database(),
            &service.inner.tasks,
            run.workspace_id,
            project_id,
            run.user_id,
            &name,
            color,
            &run.request_id,
        )
        .await?;
        service
            .record_link(run, "label", &label.id, label_id)
            .await?;
    }
    service.set_item_state(run, item, "filled").await
}

/// Statuses and labels of the imported projects, from the links.
async fn project_contexts(
    service: &OrbitImportService,
    run: &Run,
    workspace: &model::Workspace,
) -> Result<HashMap<String, ProjectContext>, Failure> {
    let mut contexts = HashMap::new();
    for project in &workspace.projects {
        if !run.selection.project_ids.contains(&project.id) {
            continue;
        }
        let Some(project_id) = service.linked(run, "project", &project.id).await? else {
            continue;
        };
        let statuses = project_statuses(service.database(), project_id).await?;
        let mut by_source = HashMap::new();
        for status in &project.statuses {
            if let Some(id) = service.linked(run, "status", &status.id).await? {
                by_source.insert(status.id.clone(), id);
            }
        }
        let open_status = statuses
            .iter()
            .find(|(_, _, category)| category == "unstarted")
            .or_else(|| {
                statuses
                    .iter()
                    .find(|(_, _, category)| category != "duplicate")
            })
            .map(|(id, _, _)| *id)
            .ok_or_else(|| Failure::Fatal("The project has no open status.".to_owned()))?;
        let mut labels = HashMap::new();
        for label in &project.labels {
            if let Some(id) = service.linked(run, "label", &label.id).await? {
                labels.insert(label.id.clone(), id);
            }
        }
        contexts.insert(
            project.id.clone(),
            ProjectContext {
                project_id,
                statuses: by_source,
                statuses_by_name: statuses
                    .iter()
                    .filter(|(_, _, category)| category != "duplicate")
                    .map(|(id, name, _)| (name.trim().to_lowercase(), *id))
                    .collect(),
                open_status,
                labels,
            },
        );
    }
    Ok(contexts)
}

/// Creates every task (parents first), then fills each with files, comments and relations.
pub(super) async fn import_tasks(
    service: &OrbitImportService,
    run: &mut Run,
    workspace: &model::Workspace,
) -> Result<(), Failure> {
    let projects = project_contexts(service, run, workspace).await?;
    let mut items = service.load_items(run.import_id, "task").await?;
    for index in 0..items.len() {
        if items[index].state != "pending" {
            continue;
        }
        let parent = items[index].parent_id.as_ref().and_then(|parent| {
            items
                .iter()
                .find(|item| {
                    &item.source_id == parent && matches!(item.state.as_str(), "created" | "filled")
                })
                .and_then(|item| item.orbit_id)
        });
        service
            .step(run, &mut items[index], async |service, run, item| {
                create_task(service, run, item, parent, &projects).await
            })
            .await?;
    }
    service.save_report(run).await?;
    for item in &mut items {
        if item.state != "created" {
            continue;
        }
        let filled = service
            .step(run, item, async |service, run, item| {
                fill_task(service, run, item).await?;
                service.set_item_state(run, item, "filled").await
            })
            .await?;
        if filled {
            service.save_report(run).await?;
        }
    }
    Ok(())
}

async fn read_task(run: &Run, path: &str) -> Result<Task, Failure> {
    let path = path.to_owned();
    archive::blocking(&run.archive, move |archive: &Archive| {
        archive.read_json(&path)
    })
    .await
    .map_err(|error| Failure::Item(error.to_string()))
}

async fn create_task(
    service: &OrbitImportService,
    run: &mut Run,
    item: &mut Item,
    parent: Option<Id>,
    projects: &HashMap<String, ProjectContext>,
) -> Result<(), Failure> {
    let (task_id, existing) = service.item_id(run, item).await?;
    if !existing {
        let task = read_task(run, &item.path).await?;
        let context = projects
            .get(&task.project_id)
            .ok_or_else(|| Failure::Item("Its project could not be imported.".to_owned()))?;
        let status_id = context
            .statuses
            .get(&task.status.id)
            .copied()
            .or_else(|| {
                task.status
                    .name
                    .as_deref()
                    .and_then(|name| context.statuses_by_name.get(&name.trim().to_lowercase()))
                    .copied()
            })
            .unwrap_or(context.open_status);
        let mut assignee_ids = Vec::new();
        let mut unmatched_assignees = Vec::new();
        for person in &task.assignees {
            match run.member(&person.id, person.name.as_deref()) {
                Some(id) if !assignee_ids.contains(&id) => assignee_ids.push(id),
                Some(_) => {}
                None => unmatched_assignees.push(run.name_of(&person.id, person.name.as_deref())),
            }
        }
        let creator = task.creator.as_ref().map(|person| {
            (
                run.member(&person.id, person.name.as_deref()),
                run.name_of(&person.id, person.name.as_deref()),
            )
        });
        let (actor_id, unmatched_creator) = match creator {
            Some((Some(id), _)) => (id, None),
            Some((None, name)) => (run.user_id, Some(name)),
            None => (run.user_id, None),
        };
        let mut label_ids = Vec::new();
        for label in &task.labels {
            if let Some(id) = context.labels.get(&label.id)
                && !label_ids.contains(id)
            {
                label_ids.push(*id);
            }
        }
        let title = clamp(task.title.trim(), 500, 500);
        let mut input = CreateTask {
            project_id: context.project_id,
            status_id,
            title: if title.is_empty() {
                "Untitled task".to_owned()
            } else {
                title
            },
            description: description(
                &task,
                &task.description,
                &unmatched_assignees,
                unmatched_creator.as_deref(),
            ),
            description_json: None,
            source_url: task
                .source_url
                .clone()
                .filter(|url| url.starts_with("https://") && url.len() <= 2_048),
            priority: match task.priority.as_str() {
                priority @ ("low" | "medium" | "high" | "urgent") => priority.to_owned(),
                _ => "none".to_owned(),
            },
            position: None,
            assignee_ids,
            label_ids,
            due_start_at: task
                .due_start_at
                .as_deref()
                .and_then(parse_instant_millis)
                .map(TimestampMillis::from_millis),
            due_at: task
                .due_at
                .as_deref()
                .and_then(parse_instant_millis)
                .map(TimestampMillis::from_millis),
            parent_task_id: parent,
        };
        let completed_at = task
            .completed_at
            .as_deref()
            .and_then(parse_instant_millis)
            .map(TimestampMillis::from_millis);
        let created_at = source_time(&task.created_at);
        let mut created = service
            .inner
            .tasks
            .import_task(
                task_id,
                run.workspace_id,
                actor_id,
                input.clone(),
                ImportedTask::completed(completed_at),
                &run.request_id,
                created_at,
            )
            .await;
        if matches!(created, Err(TaskError::ParentInvalid)) && input.parent_task_id.is_some() {
            input.parent_task_id = None;
            created = service
                .inner
                .tasks
                .import_task(
                    task_id,
                    run.workspace_id,
                    actor_id,
                    input,
                    ImportedTask::completed(completed_at),
                    &run.request_id,
                    created_at,
                )
                .await;
        }
        created?;
        run.report.created.tasks += 1;
    }
    service.set_item_state(run, item, "created").await
}

/// The source time, else now (an unparseable time must not fail the record).
pub(super) fn source_time(value: &str) -> TimestampMillis {
    parse_instant_millis(value)
        .map(TimestampMillis::from_millis)
        .unwrap_or_else(TimestampMillis::now)
}

/// `**Name** · 2024-05-01 09:30 UTC`, above a comment or message the importer writes for
/// someone who has no account here.
pub(super) fn author_header(name: &str, created_at: &str) -> String {
    let mut header = format!("**{}**", escape_markdown(name));
    if !created_at.trim().is_empty() {
        header.push_str(&format!(" · {}", display_instant(created_at)));
    }
    header
}

fn description(
    task: &Task,
    body: &str,
    unmatched_assignees: &[String],
    unmatched_creator: Option<&str>,
) -> String {
    let mut fields: Vec<(&str, String)> = Vec::new();
    if !task.key.trim().is_empty() {
        fields.push(("Task", task.key.clone()));
    }
    if let Some(creator) = unmatched_creator {
        fields.push(("Created by", creator.to_owned()));
    }
    if !unmatched_assignees.is_empty() {
        fields.push(("Assignees", unmatched_assignees.join(", ")));
    }
    with_source_details(body, "Imported from Orbit", &fields, &[])
}

/// Files, comments and relations of one task. Each step skips what an earlier import or attempt
/// already did.
async fn fill_task(
    service: &OrbitImportService,
    run: &mut Run,
    item: &Item,
) -> Result<(), Failure> {
    let task_id = item
        .orbit_id
        .ok_or_else(|| Failure::Item("The task was not created.".to_owned()))?;
    let task = read_task(run, &item.path).await?;
    let source_task: Option<Id> = task.id.parse().ok();
    let mut attached = service
        .inner
        .attachments
        .task_files(run.workspace_id, task_id)
        .await
        .map_err(|error| Failure::Transient(error.to_string()))?;
    let mut urls: Vec<(String, String)> = Vec::new();
    let comment_files: Vec<&model::File> = task
        .comments
        .iter()
        .flat_map(|comment| comment.attachments.iter())
        .collect();
    for file in task.attachments.iter().chain(comment_files) {
        let attachment_id = match service.linked(run, "attachment", &file.id).await? {
            Some(id) => id,
            None => {
                let Some(path) = file.path.as_deref() else {
                    continue;
                };
                let Some(bytes) =
                    read_file(run, path, service.inner.settings.max_file_bytes).await?
                else {
                    continue;
                };
                let name = sanitize_file_name(&file.name);
                let name = if name.is_empty() {
                    "file".to_owned()
                } else {
                    name
                };
                let id = match attach_once(
                    &service.inner.attachments,
                    &mut attached,
                    run.workspace_id,
                    task_id,
                    run.user_id,
                    &name,
                    &bytes,
                )
                .await
                {
                    Attached::Existing(id) => id,
                    Attached::Created(id) => {
                        run.report.created.files += 1;
                        id
                    }
                    Attached::Refused => continue,
                };
                service.record_link(run, "attachment", &file.id, id).await?;
                id
            }
        };
        if let (Some(source_task), Ok(source_attachment)) = (source_task, file.id.parse::<Id>()) {
            let source_workspace: Id = run
                .source_workspace_id
                .parse()
                .map_err(|_| Failure::Fatal("Stored import data is invalid.".to_owned()))?;
            urls.push((
                attachment_url(source_workspace, source_task, source_attachment),
                attachment_url(run.workspace_id, task_id, attachment_id),
            ));
        }
    }
    if mentions_any(&task.description, &urls) {
        let version: i64 = sqlx::query_scalar("SELECT version FROM tasks WHERE id = $1")
            .bind(task_id.to_string())
            .fetch_one(service.database().pool())
            .await?;
        let current: String = sqlx::query_scalar("SELECT description FROM tasks WHERE id = $1")
            .bind(task_id.to_string())
            .fetch_one(service.database().pool())
            .await?;
        service
            .inner
            .tasks
            .update_task(
                run.workspace_id,
                run.user_id,
                &TaskUpdate {
                    id: task_id,
                    expected_version: u64::try_from(version).unwrap_or_default(),
                    changes: TaskChanges {
                        description: Some(rewrite_links(&current, &urls)),
                        ..TaskChanges::default()
                    },
                },
                &run.request_id,
                TimestampMillis::now(),
            )
            .await?;
    }

    let mut comments: Vec<&model::Comment> = task.comments.iter().collect();
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    for comment in comments {
        if service.linked(run, "comment", &comment.id).await?.is_some() {
            continue;
        }
        let author = match (&comment.author, &comment.author_service_account) {
            (Some(person), None) => run.member(&person.id, person.name.as_deref()),
            _ => None,
        };
        let author_name = match (&comment.author, &comment.author_service_account) {
            (_, Some(account)) => account.display(),
            (Some(person), None) => run.name_of(&person.id, person.name.as_deref()),
            (None, None) => "Unknown member".to_owned(),
        };
        let parent_id = match &comment.parent_id {
            Some(parent) => service.linked(run, "comment", parent).await?,
            None => None,
        };
        let mut text = rewrite_links(comment.body.trim(), &urls);
        for file in &comment.attachments {
            if let Some((_, url)) = urls.iter().find(|(source, _)| {
                file.id
                    .parse::<Id>()
                    .is_ok_and(|id| source.contains(&id.to_string()))
            }) {
                text.push_str(&format!("\n\n[{}](<{url}>)", escape_markdown(&file.name)));
            }
        }
        let (actor_id, body) = match author {
            Some(id) => (id, text),
            None => {
                let header = author_header(&author_name, &comment.created_at);
                (
                    run.user_id,
                    if text.is_empty() {
                        header
                    } else {
                        format!("{header}\n\n{text}")
                    },
                )
            }
        };
        if body.trim().is_empty() {
            continue;
        }
        let created = service
            .inner
            .tasks
            .create_comment(
                run.workspace_id,
                task_id,
                actor_id,
                parent_id,
                CommentContent::Markdown {
                    body,
                    mentioned_user_ids: Vec::new(),
                },
                &run.request_id,
                source_time(&comment.created_at),
            )
            .await?;
        service
            .record_link(run, "comment", &comment.id, created.id)
            .await?;
    }

    for relation in &task.relations {
        if relation.task_id != task.id {
            continue;
        }
        let Some(target) = service
            .linked(run, "task", &relation.related_task_id)
            .await?
        else {
            continue;
        };
        link_tasks(service, run, task_id, target, &relation.kind).await?;
    }
    Ok(())
}

/// Reads a file of the archive; `None` when it is over the size limit (counted nowhere, like
/// a file the source had lost).
pub(super) async fn read_file(run: &Run, path: &str, max: u64) -> Result<Option<Vec<u8>>, Failure> {
    let path = path.to_owned();
    match archive::blocking(&run.archive, move |archive| archive.read(&path, max)).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(archive::ArchiveError::EntryTooLarge(..)) => Ok(None),
        Err(archive::ArchiveError::Read(_)) => Ok(None),
        Err(error) => Err(Failure::from(error)),
    }
}

/// A blocks, related or duplicate relation between two imported tasks; one Orbit cannot hold
/// (a second relation between the same pair, a second duplicate) is left out.
async fn link_tasks(
    service: &OrbitImportService,
    run: &mut Run,
    source: Id,
    target: Id,
    kind: &str,
) -> Result<(), Failure> {
    if source == target {
        return Ok(());
    }
    let kind = match kind {
        "blocks" | "related" | "duplicate" => kind,
        _ => return Ok(()),
    };
    let existing = sqlx::query(
        "SELECT task_id FROM task_relations \
         WHERE (task_id = $1 AND related_task_id = $2) OR (task_id = $2 AND related_task_id = $1)",
    )
    .bind(source.to_string())
    .bind(target.to_string())
    .fetch_optional(service.database().pool())
    .await?;
    if existing.is_some() {
        return Ok(());
    }
    let result = if kind == "duplicate" {
        let Flag(blocked) = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM task_relations WHERE type = 'duplicate' \
             AND (task_id = $1 OR task_id = $2))",
        )
        .bind(source.to_string())
        .bind(target.to_string())
        .fetch_one(service.database().pool())
        .await?;
        if blocked {
            return Ok(());
        }
        let version: i64 = sqlx::query_scalar("SELECT version FROM tasks WHERE id = $1")
            .bind(source.to_string())
            .fetch_one(service.database().pool())
            .await?;
        service
            .inner
            .tasks
            .update_task(
                run.workspace_id,
                run.user_id,
                &TaskUpdate {
                    id: source,
                    expected_version: u64::try_from(version).unwrap_or_default(),
                    changes: TaskChanges {
                        duplicate_of_id: Some(Some(target)),
                        ..TaskChanges::default()
                    },
                },
                &run.request_id,
                TimestampMillis::now(),
            )
            .await
            .map(|_| ())
    } else {
        let kind = if kind == "blocks" {
            NewTaskRelationType::Blocks
        } else {
            NewTaskRelationType::Related
        };
        service
            .inner
            .tasks
            .add_task_relation(
                run.workspace_id,
                source,
                run.user_id,
                kind,
                target,
                &run.request_id,
                TimestampMillis::now(),
            )
            .await
            .map(|_| ())
    };
    match result {
        Ok(()) => Ok(()),
        Err(TaskError::Unavailable(error)) => Err(Failure::from(error)),
        // A task deleted or changed meanwhile.
        Err(_) => Ok(()),
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Status names match ignoring case and the "canceled"/"cancelled" spelling.
fn same_status_name(a: &str, b: &str) -> bool {
    let normal = |name: &str| name.trim().to_lowercase().replace("cancelled", "canceled");
    normal(a) == normal(b)
}
