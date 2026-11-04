//! What a workspace export ZIP holds and how it is written. The layout is in [`README`].
//!
//! Every query reads one table for the whole workspace and groups the rows in memory (text only);
//! file bytes stream from the blob store into the archive, which is written to a file on disk.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use orbit_platform::{BlobStore, BlobStoreError, Database, Id, Row, TimestampMillis};
use serde_json::{Map, Value, json};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::ExportError;
use crate::export::names::{
    NameSet, STEM_MAX_CHARS, sanitize_segment, split_file_name, with_extension,
};
use crate::export::{ExportEntry, ExportLimits, compressed_type, plan, zip_time};
use crate::repositories::page_export::{PageExportError, PageExportRepository};

pub(super) const README: &str = "\
Orbit workspace export
======================

All JSON field names are snake_case and stable. Times are RFC 3339 in UTC. People appear as
{\"id\", \"name\"}; their email and role are in workspace.json. Chat and comment text keeps
Orbit's mention tokens (<@user-id>, <#channel-id>, <!channel>, <!here>).

workspace.json
    The workspace (name, created_at, exported_at, exported_by), its members (name, email,
    role, joined_at), teamspaces, and projects with their statuses (workflow) and labels.

tasks/<KEY>-<number>.json
    One file per task, also tasks in the trash (\"deleted\": true) and tasks of deleted
    projects: title, Markdown description, status, priority, dates, creator, assignees,
    labels, relations, comments, attachments and activity (the task's audit events).

pages/<Teamspace>/<Page>.md, pages/Private/<Page>.md
    Every live page as Markdown, with its sub-pages in the folder <Page>/ next to it and its
    files in <Page>/assets/. Links between exported pages are relative. Private pages are
    only those of the member who started the export.
pages/comments.json
    The comment threads of the exported pages.

chat/<channel>.json, chat/DM - <names>.json
    One file per conversation: its members and messages, each with reactions, files and
    thread replies (\"replies\"). Public channels are all included. Private channels and
    direct messages are included only if the member who started the export is in them.

files/<file-id>/<name>
    The files attached to tasks, comments and chat messages, once each. The JSON files
    refer to them by \"path\". A file that was deleted from storage has \"path\": null.

Not included: trashed pages, page version history, notifications, saved views, API tokens,
integrations, custom emoji and stickers, and other members' private pages.
";

/// What the export reads and as whom.
pub(super) struct Source<'a> {
    pub database: &'a Database,
    pub blobs: &'a dyn BlobStore,
    pub workspace_id: Id,
    pub actor_id: Id,
    pub origin: &'a str,
    pub now: TimestampMillis,
    pub cancel: &'a CancellationToken,
}

/// Writes the archive to `path` and returns its size in bytes.
pub(super) async fn write(source: &Source<'_>, path: PathBuf) -> Result<u64, ExportError> {
    let zip = ZipSink::create(path, zip_time(source.now));
    let result = contents(source, &zip).await;
    let written = zip.finish().await;
    match (result, written) {
        (Ok(()), Ok(size)) => Ok(size),
        // A closed channel is explained by the writer's own error.
        (Err(ExportError::Storage(_)), Err(error)) | (Ok(()), Err(error)) => Err(io(error)),
        (Err(error), _) => Err(error),
    }
}

fn io(error: std::io::Error) -> ExportError {
    ExportError::Storage(error.to_string())
}

enum Piece {
    Start { path: String, compress: bool },
    Bytes(Vec<u8>),
}

/// The ZIP writer runs on a blocking thread; entries reach it in pieces through a short channel,
/// so a large file never sits in memory.
struct ZipSink {
    sender: mpsc::Sender<Piece>,
    writer: tokio::task::JoinHandle<std::io::Result<u64>>,
}

impl ZipSink {
    fn create(path: PathBuf, modified: zip::DateTime) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Piece>(8);
        let writer = tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
            let file = std::fs::File::create(&path)?;
            let mut zip = ZipWriter::new(std::io::BufWriter::new(file));
            while let Some(piece) = receiver.blocking_recv() {
                match piece {
                    Piece::Start { path, compress } => {
                        let method = if compress {
                            CompressionMethod::Deflated
                        } else {
                            CompressionMethod::Stored
                        };
                        let options = SimpleFileOptions::default()
                            .compression_method(method)
                            .last_modified_time(modified)
                            .unix_permissions(0o644)
                            .large_file(true);
                        zip.start_file(path, options)
                            .map_err(std::io::Error::other)?;
                    }
                    Piece::Bytes(bytes) => zip.write_all(&bytes)?,
                }
            }
            let file = zip
                .finish()
                .map_err(std::io::Error::other)?
                .into_inner()
                .map_err(std::io::IntoInnerError::into_error)?;
            file.sync_all()?;
            Ok(file.metadata()?.len())
        });
        Self { sender, writer }
    }

    async fn send(&self, piece: Piece) -> Result<(), ExportError> {
        self.sender
            .send(piece)
            .await
            .map_err(|_| ExportError::Storage("the archive writer stopped".to_owned()))
    }

    async fn file(&self, path: String, bytes: Vec<u8>) -> Result<(), ExportError> {
        self.send(Piece::Start {
            path,
            compress: true,
        })
        .await?;
        self.send(Piece::Bytes(bytes)).await
    }

    async fn json(&self, path: String, value: &Value) -> Result<(), ExportError> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|error| io(error.into()))?;
        self.file(path, bytes).await
    }

    /// Streams a stored blob into the entry `path`. `false` when the blob is not in storage.
    async fn blob(
        &self,
        blobs: &dyn BlobStore,
        cancel: &CancellationToken,
        path: String,
        storage_key: &str,
        compress: bool,
    ) -> Result<bool, ExportError> {
        let mut reader = match blobs.open(storage_key).await {
            Ok(reader) => reader,
            Err(BlobStoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(false);
            }
            Err(error) => return Err(ExportError::Storage(error.to_string())),
        };
        self.send(Piece::Start { path, compress }).await?;
        loop {
            if cancel.is_cancelled() {
                return Err(ExportError::Cancelled);
            }
            let mut buffer = vec![0_u8; 256 * 1024];
            let count = reader.read(&mut buffer).await.map_err(io)?;
            if count == 0 {
                return Ok(true);
            }
            buffer.truncate(count);
            self.send(Piece::Bytes(buffer)).await?;
        }
    }

    async fn finish(self) -> std::io::Result<u64> {
        drop(self.sender);
        self.writer.await.map_err(std::io::Error::other)?
    }
}

/// The files of tasks, comments and chat messages, each blob once under `files/`.
#[derive(Default)]
struct Files {
    paths: HashMap<String, String>,
    /// `(path, storage_key, compress)` in the order they were first referenced.
    order: Vec<(String, String, bool)>,
}

impl Files {
    /// The archive path of the blob, `None` for a deleted file.
    fn add(&mut self, row: &sqlx::any::AnyRow, name: &str, media_type: &str) -> Value {
        if row.get::<Option<i64>, _>("blob_deleted_at").is_some() {
            return Value::Null;
        }
        let blob_id: String = row.get("blob_id");
        if let Some(path) = self.paths.get(&blob_id) {
            return json!(path);
        }
        let (stem, extension) = split_file_name(name);
        let path = format!("files/{blob_id}/{}", with_extension(&stem, &extension));
        self.paths.insert(blob_id, path.clone());
        self.order.push((
            path.clone(),
            row.get("storage_key"),
            !compressed_type(media_type),
        ));
        json!(path)
    }
}

fn time(millis: i64) -> Value {
    json!(TimestampMillis::from_millis(millis))
}

fn optional_time(millis: Option<i64>) -> Value {
    millis.map_or(Value::Null, time)
}

/// `{"id", "name"}` of a user id.
struct People(HashMap<String, String>);

impl People {
    fn of(&self, id: Option<String>) -> Value {
        match id {
            Some(id) => json!({"name": self.0.get(&id), "id": id}),
            None => Value::Null,
        }
    }
}

fn push(map: &mut HashMap<String, Vec<Value>>, key: String, value: Value) {
    map.entry(key).or_default().push(value);
}

async fn contents(source: &Source<'_>, zip: &ZipSink) -> Result<(), ExportError> {
    let pool = source.database.pool();
    let workspace = source.workspace_id.to_string();
    let people = People(
        sqlx::query(
            "SELECT id, display_name FROM users WHERE id IN ( \
             SELECT user_id FROM memberships WHERE workspace_id = $1 \
             UNION SELECT creator_id FROM tasks WHERE workspace_id = $1 \
             UNION SELECT author_id FROM task_comments WHERE workspace_id = $1 \
             UNION SELECT author_id FROM page_comments WHERE workspace_id = $1 \
             UNION SELECT chat_messages.author_id FROM chat_messages JOIN chat_conversations \
             ON chat_conversations.id = chat_messages.conversation_id \
             WHERE chat_conversations.workspace_id = $1)",
        )
        .bind(&workspace)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| (row.get("id"), row.get("display_name")))
        .collect(),
    );
    let mut files = Files::default();
    workspace_json(source, zip, &people).await?;
    tasks(source, zip, &people, &mut files).await?;
    pages(source, zip, &people).await?;
    chat(source, zip, &people, &mut files).await?;

    let mut missing = Vec::new();
    for (path, storage_key, compress) in files.order {
        if !zip
            .blob(
                source.blobs,
                source.cancel,
                path.clone(),
                &storage_key,
                compress,
            )
            .await?
        {
            missing.push(path);
        }
    }
    let mut readme = README.to_owned();
    if !missing.is_empty() {
        tracing::warn!(count = missing.len(), workspace_id = %workspace, "export: files missing from storage");
        readme.push_str("\nFiles that could not be read from storage (not in this archive):\n");
        for path in missing {
            readme.push_str(&format!("    {path}\n"));
        }
    }
    zip.file("README.txt".to_owned(), readme.into_bytes()).await
}

async fn workspace_json(
    source: &Source<'_>,
    zip: &ZipSink,
    people: &People,
) -> Result<(), ExportError> {
    let pool = source.database.pool();
    let workspace = source.workspace_id.to_string();
    let row = sqlx::query("SELECT name, created_at FROM workspaces WHERE id = $1")
        .bind(&workspace)
        .fetch_one(pool)
        .await?;
    let members: Vec<Value> = sqlx::query(
        "SELECT users.id, users.display_name, users.email, memberships.role, memberships.created_at \
         FROM memberships JOIN users ON users.id = memberships.user_id \
         WHERE memberships.workspace_id = $1 ORDER BY memberships.created_at, users.id",
    )
    .bind(&workspace)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("display_name"),
            "email": row.get::<String, _>("email"),
            "role": row.get::<String, _>("role"),
            "joined_at": time(row.get("created_at")),
        })
    })
    .collect();
    let teamspaces: Vec<Value> = sqlx::query(
        "SELECT id, name, icon, created_at FROM teamspaces WHERE workspace_id = $1 \
         ORDER BY position, id",
    )
    .bind(&workspace)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "icon": row.get::<Option<String>, _>("icon"),
            "created_at": time(row.get("created_at")),
        })
    })
    .collect();
    let mut statuses = HashMap::new();
    for row in sqlx::query(
        "SELECT id, project_id, name, description, color, category FROM task_statuses \
         WHERE workspace_id = $1 ORDER BY position, id",
    )
    .bind(&workspace)
    .fetch_all(pool)
    .await?
    {
        push(
            &mut statuses,
            row.get("project_id"),
            json!({
                "id": row.get::<String, _>("id"),
                "name": row.get::<String, _>("name"),
                "description": row.get::<String, _>("description"),
                "color": row.get::<String, _>("color"),
                "category": row.get::<String, _>("category"),
            }),
        );
    }
    let mut labels = HashMap::new();
    for row in sqlx::query(
        "SELECT id, project_id, name, color FROM labels WHERE workspace_id = $1 ORDER BY name, id",
    )
    .bind(&workspace)
    .fetch_all(pool)
    .await?
    {
        push(
            &mut labels,
            row.get("project_id"),
            json!({
                "id": row.get::<String, _>("id"),
                "name": row.get::<String, _>("name"),
                "color": row.get::<String, _>("color"),
            }),
        );
    }
    let projects: Vec<Value> = sqlx::query(
        "SELECT id, name, project_key, color, deleted_at, created_at FROM projects \
         WHERE workspace_id = $1 ORDER BY created_at, id",
    )
    .bind(&workspace)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        let id: String = row.get("id");
        json!({
            "key": row.get::<String, _>("project_key"),
            "name": row.get::<String, _>("name"),
            "color": row.get::<String, _>("color"),
            "deleted": row.get::<Option<i64>, _>("deleted_at").is_some(),
            "created_at": time(row.get("created_at")),
            "statuses": statuses.remove(&id).unwrap_or_default(),
            "labels": labels.remove(&id).unwrap_or_default(),
            "id": id,
        })
    })
    .collect();
    zip.json(
        "workspace.json".to_owned(),
        &json!({
            "id": workspace,
            "name": row.get::<String, _>("name"),
            "created_at": time(row.get("created_at")),
            "exported_at": json!(source.now),
            "exported_by": people.of(Some(source.actor_id.to_string())),
            "members": members,
            "teamspaces": teamspaces,
            "projects": projects,
        }),
    )
    .await
}

async fn tasks(
    source: &Source<'_>,
    zip: &ZipSink,
    people: &People,
    files: &mut Files,
) -> Result<(), ExportError> {
    let pool = source.database.pool();
    let workspace = source.workspace_id.to_string();
    let fetch = |sql: &'static str| sqlx::query(sql).bind(workspace.clone()).fetch_all(pool);

    let mut assignees = HashMap::new();
    for row in fetch(
        "SELECT task_assignees.task_id, task_assignees.user_id FROM task_assignees \
         JOIN tasks ON tasks.id = task_assignees.task_id WHERE tasks.workspace_id = $1 \
         ORDER BY task_assignees.user_id",
    )
    .await?
    {
        push(
            &mut assignees,
            row.get("task_id"),
            people.of(row.get("user_id")),
        );
    }
    let mut labels = HashMap::new();
    for row in fetch(
        "SELECT task_labels.task_id, labels.id, labels.name FROM task_labels \
         JOIN labels ON labels.id = task_labels.label_id WHERE labels.workspace_id = $1 \
         ORDER BY labels.name, labels.id",
    )
    .await?
    {
        push(
            &mut labels,
            row.get("task_id"),
            json!({"id": row.get::<String, _>("id"), "name": row.get::<String, _>("name")}),
        );
    }
    let mut relations = HashMap::new();
    for row in fetch(
        "SELECT task_id, related_task_id, type FROM task_relations WHERE workspace_id = $1 \
         ORDER BY created_at, id",
    )
    .await?
    {
        let (from, to): (String, String) = (row.get("task_id"), row.get("related_task_id"));
        let relation =
            json!({"type": row.get::<String, _>("type"), "task_id": from, "related_task_id": to});
        push(&mut relations, from, relation.clone());
        push(&mut relations, to, relation);
    }
    // Attachments of a task (`comment_id` NULL) and of its comments.
    let mut task_files = HashMap::new();
    let mut comment_files = HashMap::new();
    for row in fetch(
        "SELECT attachment_references.id, attachment_references.task_id, attachment_references.comment_id, \
         attachment_references.display_name, attachment_references.media_type, attachment_references.inline, \
         attachment_references.created_at, attachment_blobs.id AS blob_id, attachment_blobs.byte_size, \
         attachment_blobs.storage_key, attachment_blobs.deleted_at AS blob_deleted_at \
         FROM attachment_references JOIN attachment_blobs ON attachment_blobs.id = attachment_references.blob_id \
         WHERE attachment_references.workspace_id = $1 \
         ORDER BY attachment_references.created_at, attachment_references.id",
    )
    .await?
    {
        let name: String = row.get("display_name");
        let media_type: String = row.get("media_type");
        let attachment = json!({
            "id": row.get::<String, _>("id"),
            "name": name,
            "media_type": media_type,
            "size_bytes": row.get::<i64, _>("byte_size"),
            "inline": row.get::<bool, _>("inline"),
            "created_at": time(row.get("created_at")),
            "path": files.add(&row, &name, &media_type),
        });
        match row.get::<Option<String>, _>("comment_id") {
            Some(comment) => push(&mut comment_files, comment, attachment),
            None => push(&mut task_files, row.get("task_id"), attachment),
        }
    }
    let mut comments = HashMap::new();
    for row in fetch(
        "SELECT id, task_id, parent_id, author_id, body, created_at, updated_at FROM task_comments \
         WHERE workspace_id = $1 ORDER BY created_at, id",
    )
    .await?
    {
        let id: String = row.get("id");
        push(
            &mut comments,
            row.get("task_id"),
            json!({
                "parent_id": row.get::<Option<String>, _>("parent_id"),
                "author": people.of(row.get("author_id")),
                "body": row.get::<String, _>("body"),
                "created_at": time(row.get("created_at")),
                "updated_at": time(row.get("updated_at")),
                "attachments": comment_files.remove(&id).unwrap_or_default(),
                "id": id,
            }),
        );
    }
    let mut activity = HashMap::new();
    for row in fetch(
        "SELECT resource_id, action, actor_id, metadata_json, occurred_at FROM audit_events \
         WHERE workspace_id = $1 AND resource_type = 'task' AND resource_id IS NOT NULL \
         ORDER BY occurred_at, id",
    )
    .await?
    {
        let metadata: Value =
            serde_json::from_str(row.get::<&str, _>("metadata_json")).unwrap_or(Value::Null);
        push(
            &mut activity,
            row.get("resource_id"),
            json!({
                "action": row.get::<String, _>("action"),
                "actor": people.of(row.get("actor_id")),
                "occurred_at": time(row.get("occurred_at")),
                "metadata": metadata,
            }),
        );
    }

    let mut names = NameSet::default();
    for row in fetch(
        "SELECT tasks.id, tasks.number, projects.project_key, tasks.project_id, tasks.status_id, \
         task_statuses.name AS status_name, tasks.title, tasks.description, tasks.priority, \
         tasks.parent_task_id, tasks.creator_id, tasks.source_url, tasks.due_start_at, tasks.due_at, \
         tasks.completed_at, tasks.deleted_at, tasks.created_at, tasks.updated_at \
         FROM tasks JOIN projects ON projects.id = tasks.project_id \
         LEFT JOIN task_statuses ON task_statuses.id = tasks.status_id \
         WHERE tasks.workspace_id = $1 ORDER BY projects.project_key, tasks.number, tasks.id",
    )
    .await?
    {
        if source.cancel.is_cancelled() {
            return Err(ExportError::Cancelled);
        }
        let id: String = row.get("id");
        let key = match row.get::<Option<i64>, _>("number") {
            Some(number) => format!("{}-{number}", row.get::<String, _>("project_key")),
            None => id.clone(),
        };
        let file = names.claim(&sanitize_segment(&key, STEM_MAX_CHARS, "task"), &["json"]);
        let task = json!({
            "id": id,
            "key": key,
            "project_id": row.get::<String, _>("project_id"),
            "status": {
                "id": row.get::<String, _>("status_id"),
                "name": row.get::<Option<String>, _>("status_name"),
            },
            "title": row.get::<String, _>("title"),
            "description": row.get::<String, _>("description"),
            "priority": row.get::<String, _>("priority"),
            "parent_task_id": row.get::<Option<String>, _>("parent_task_id"),
            "creator": people.of(row.get("creator_id")),
            "source_url": row.get::<Option<String>, _>("source_url"),
            "due_start_at": optional_time(row.get("due_start_at")),
            "due_at": optional_time(row.get("due_at")),
            "completed_at": optional_time(row.get("completed_at")),
            "deleted": row.get::<Option<i64>, _>("deleted_at").is_some(),
            "deleted_at": optional_time(row.get("deleted_at")),
            "created_at": time(row.get("created_at")),
            "updated_at": time(row.get("updated_at")),
            "assignees": assignees.remove(&id).unwrap_or_default(),
            "labels": labels.remove(&id).unwrap_or_default(),
            "relations": relations.remove(&id).unwrap_or_default(),
            "attachments": task_files.remove(&id).unwrap_or_default(),
            "comments": comments.remove(&id).unwrap_or_default(),
            "activity": activity.remove(&id).unwrap_or_default(),
        });
        zip.json(format!("tasks/{file}.json"), &task).await?;
    }
    Ok(())
}

fn page_error(error: PageExportError) -> ExportError {
    match error {
        PageExportError::Unavailable(error) => ExportError::Unavailable(error),
        PageExportError::NotFound => ExportError::NotFound,
        error => ExportError::Storage(error.to_string()),
    }
}

async fn pages(source: &Source<'_>, zip: &ZipSink, people: &People) -> Result<(), ExportError> {
    let pages = PageExportRepository::new(source.database.clone())
        .load_workspace(source.workspace_id, source.actor_id, "pages")
        .await
        .map_err(page_error)?;
    if pages.pages.is_empty() {
        return Ok(());
    }
    let unlimited = ExportLimits {
        max_pages: usize::MAX,
        max_bytes: u64::MAX,
    };
    let plan = plan(
        &pages,
        source.workspace_id,
        source.origin,
        source.now,
        unlimited,
    )
    .map_err(page_error)?;
    // The plan lists the Markdown files first, in the order of `pages.pages`.
    let paths: HashMap<String, (String, String)> = pages
        .pages
        .iter()
        .zip(&plan.entries)
        .map(|(page, entry)| {
            (
                page.id.to_string(),
                (entry.path().to_owned(), page.title.clone()),
            )
        })
        .collect();
    for entry in plan.entries {
        match entry {
            ExportEntry::Markdown { path, text } => zip.file(path, text.into_bytes()).await?,
            ExportEntry::Asset {
                path,
                page_id,
                file_id,
                compress,
                ..
            } => {
                let file = &pages.files[&(page_id, file_id)];
                zip.blob(
                    source.blobs,
                    source.cancel,
                    path,
                    &file.storage_key,
                    compress,
                )
                .await?;
            }
        }
    }

    let pool = source.database.pool();
    let workspace = source.workspace_id.to_string();
    let mut comments = HashMap::new();
    for row in sqlx::query(
        "SELECT id, thread_id, author_id, body_text, created_at, edited_at, deleted_at \
         FROM page_comments WHERE workspace_id = $1 ORDER BY created_at, id",
    )
    .bind(&workspace)
    .fetch_all(pool)
    .await?
    {
        push(
            &mut comments,
            row.get("thread_id"),
            json!({
                "id": row.get::<String, _>("id"),
                "author": people.of(row.get("author_id")),
                "body": row.get::<String, _>("body_text"),
                "created_at": time(row.get("created_at")),
                "edited_at": optional_time(row.get("edited_at")),
                "deleted": row.get::<Option<i64>, _>("deleted_at").is_some(),
            }),
        );
    }
    let mut threads: HashMap<String, Vec<Value>> = HashMap::new();
    let mut order = Vec::new();
    for row in sqlx::query(
        "SELECT id, page_id, created_by, quote, resolved_at, resolved_by, created_at \
         FROM page_threads WHERE workspace_id = $1 ORDER BY created_at, id",
    )
    .bind(&workspace)
    .fetch_all(pool)
    .await?
    {
        let page_id: String = row.get("page_id");
        if !paths.contains_key(&page_id) {
            continue;
        }
        let id: String = row.get("id");
        if !threads.contains_key(&page_id) {
            order.push(page_id.clone());
        }
        push(
            &mut threads,
            page_id,
            json!({
                "quote": row.get::<Option<String>, _>("quote"),
                "created_by": people.of(row.get("created_by")),
                "created_at": time(row.get("created_at")),
                "resolved_at": optional_time(row.get("resolved_at")),
                "resolved_by": people.of(row.get("resolved_by")),
                "comments": comments.remove(&id).unwrap_or_default(),
                "id": id,
            }),
        );
    }
    if order.is_empty() {
        return Ok(());
    }
    let pages: Vec<Value> = order
        .into_iter()
        .map(|page_id| {
            let (path, title) = &paths[&page_id];
            json!({
                "page_id": page_id,
                "title": title,
                "path": path,
                "threads": threads.remove(&page_id).unwrap_or_default(),
            })
        })
        .collect();
    zip.json("pages/comments.json".to_owned(), &json!(pages))
        .await
}

async fn chat(
    source: &Source<'_>,
    zip: &ZipSink,
    people: &People,
    files: &mut Files,
) -> Result<(), ExportError> {
    let pool = source.database.pool();
    let conversations = sqlx::query(
        "SELECT id, kind, name, topic, is_default, archived_at, created_by, created_at \
         FROM chat_conversations WHERE workspace_id = $1 AND (kind = 'public' OR EXISTS ( \
         SELECT 1 FROM chat_members WHERE chat_members.conversation_id = chat_conversations.id \
         AND chat_members.user_id = $2)) ORDER BY kind DESC, name, id",
    )
    .bind(source.workspace_id.to_string())
    .bind(source.actor_id.to_string())
    .fetch_all(pool)
    .await?;
    let mut names = NameSet::default();
    for conversation in conversations {
        if source.cancel.is_cancelled() {
            return Err(ExportError::Cancelled);
        }
        let id: String = conversation.get("id");
        let kind: String = conversation.get("kind");
        let fetch = |sql: &'static str| sqlx::query(sql).bind(id.clone()).fetch_all(pool);
        let member_ids: Vec<String> = fetch(
            "SELECT user_id FROM chat_members WHERE conversation_id = $1 ORDER BY joined_at, user_id",
        )
        .await?
        .into_iter()
        .map(|row| row.get("user_id"))
        .collect();
        let mut reactions: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for row in fetch(
            "SELECT chat_reactions.message_id, chat_reactions.emoji, chat_reactions.user_id \
             FROM chat_reactions JOIN chat_messages ON chat_messages.id = chat_reactions.message_id \
             WHERE chat_messages.conversation_id = $1 ORDER BY chat_reactions.created_at, chat_reactions.user_id",
        )
        .await?
        {
            reactions
                .entry(row.get("message_id"))
                .or_default()
                .push((row.get("emoji"), row.get("user_id")));
        }
        let mut message_files = HashMap::new();
        for row in fetch(
            "SELECT chat_message_files.id, chat_message_files.message_id, chat_message_files.file_name, \
             chat_message_files.mime_type, chat_message_files.size_bytes, chat_message_files.created_at, \
             attachment_blobs.id AS blob_id, attachment_blobs.storage_key, \
             attachment_blobs.deleted_at AS blob_deleted_at FROM chat_message_files \
             JOIN attachment_blobs ON attachment_blobs.id = chat_message_files.blob_id \
             WHERE chat_message_files.conversation_id = $1 AND chat_message_files.message_id IS NOT NULL \
             ORDER BY chat_message_files.created_at, chat_message_files.id",
        )
        .await?
        {
            let name: String = row.get("file_name");
            let media_type: String = row.get("mime_type");
            push(
                &mut message_files,
                row.get("message_id"),
                json!({
                    "id": row.get::<String, _>("id"),
                    "name": name,
                    "media_type": media_type,
                    "size_bytes": row.get::<i64, _>("size_bytes"),
                    "path": files.add(&row, &name, &media_type),
                }),
            );
        }
        let mut messages: Vec<Value> = Vec::new();
        let mut roots: HashMap<String, usize> = HashMap::new();
        for row in fetch(
            "SELECT id, thread_root_id, kind, author_id, body, reply_to_id, forward_of_id, pinned_at, \
             edited_at, deleted_at, created_at FROM chat_messages WHERE conversation_id = $1 ORDER BY id",
        )
        .await?
        {
            let message_id: String = row.get("id");
            let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
            for (emoji, user) in reactions.remove(&message_id).unwrap_or_default() {
                match grouped.iter_mut().find(|(seen, _)| *seen == emoji) {
                    Some((_, users)) => users.push(user),
                    None => grouped.push((emoji, vec![user])),
                }
            }
            let reactions: Vec<Value> = grouped
                .into_iter()
                .map(|(emoji, users)| {
                    let users: Vec<Value> =
                        users.into_iter().map(|user| people.of(Some(user))).collect();
                    json!({"emoji": emoji, "users": users})
                })
                .collect();
            let mut message = Map::new();
            message.insert("id".to_owned(), json!(message_id));
            message.insert("kind".to_owned(), json!(row.get::<String, _>("kind")));
            message.insert("author".to_owned(), people.of(row.get("author_id")));
            message.insert("body".to_owned(), json!(row.get::<String, _>("body")));
            message.insert("created_at".to_owned(), time(row.get("created_at")));
            message.insert("edited_at".to_owned(), optional_time(row.get("edited_at")));
            message.insert(
                "deleted".to_owned(),
                json!(row.get::<Option<i64>, _>("deleted_at").is_some()),
            );
            message.insert(
                "pinned".to_owned(),
                json!(row.get::<Option<i64>, _>("pinned_at").is_some()),
            );
            message.insert(
                "reply_to_id".to_owned(),
                json!(row.get::<Option<String>, _>("reply_to_id")),
            );
            message.insert(
                "forward_of_id".to_owned(),
                json!(row.get::<Option<String>, _>("forward_of_id")),
            );
            message.insert("reactions".to_owned(), json!(reactions));
            message.insert(
                "files".to_owned(),
                json!(message_files.remove(&message_id).unwrap_or_default()),
            );
            let root = row
                .get::<Option<String>, _>("thread_root_id")
                .and_then(|root| roots.get(&root).copied());
            match root {
                Some(index) => {
                    if let Some(Value::Array(replies)) = messages[index].get_mut("replies") {
                        replies.push(Value::Object(message));
                    }
                }
                None => {
                    message.insert("replies".to_owned(), json!([]));
                    roots.insert(message_id, messages.len());
                    messages.push(Value::Object(message));
                }
            }
        }
        let title = if kind == "dm" {
            let others: Vec<String> = member_ids
                .iter()
                .filter_map(|member| people.0.get(member).cloned())
                .collect();
            format!("DM - {}", others.join(", "))
        } else {
            conversation.get::<String, _>("name")
        };
        let file = names.claim(
            &sanitize_segment(&title, STEM_MAX_CHARS, "conversation"),
            &["json"],
        );
        let members: Vec<Value> = member_ids
            .into_iter()
            .map(|member| people.of(Some(member)))
            .collect();
        let document = json!({
            "id": id,
            "kind": kind,
            "name": conversation.get::<String, _>("name"),
            "topic": conversation.get::<Option<String>, _>("topic"),
            "default": conversation.get::<bool, _>("is_default"),
            "archived": conversation.get::<Option<i64>, _>("archived_at").is_some(),
            "created_by": people.of(conversation.get("created_by")),
            "created_at": time(conversation.get("created_at")),
            "members": members,
            "messages": messages,
        });
        zip.json(format!("chat/{file}.json"), &document).await?;
    }
    Ok(())
}
