//! The docs side of an import: pages in two passes (create parents first, then fill each with
//! its BlockNote content, files and cover), then their comment threads. Teamspace pages go to
//! the matching teamspace (full import only); private pages of the exporter land in the
//! importer's private space.

use std::collections::{HashMap, HashSet};

use orbit_platform::{Id, TimestampMillis};
use serde_json::{Value, json};

use super::archive::{self, Archive};
use super::model::{self, Page, Thread};
use super::tasks::{author_header, read_file, source_time};
use super::{Item, OrbitImportService, Run};
use crate::notion::files::sanitize_file_name;
use crate::notion::import::clamp;
use crate::repositories::page_comments::{CommentBody, PageCommentError};
use crate::repositories::page_files::{page_file_url, parse_page_file_url};
use crate::repositories::page_versions::SaveOrigin;
use crate::repositories::pages::{CreatePage, PageChanges, PageError, PageKind, SpaceRequest};
use crate::task_import::Failure;
use crate::workspace_export::ExportMode;

const MAX_TITLE_CHARS: usize = 500;
const MAX_TITLE_BYTES: usize = 2_000;
const MAX_DEPTH: usize = 50;

/// Lists the selected pages, parents before children. A page whose parent is not selected
/// becomes a root. Blocking reads.
pub(super) async fn plan_pages(run: &Run) -> Result<Vec<Item>, Failure> {
    let teamspaces = run.selection.teamspace_ids.clone();
    let private = run.selection.include_private_pages;
    let full = run.mode == ExportMode::Full;
    let listed = archive::blocking(&run.archive, move |archive| {
        let mut pages = Vec::new();
        for path in archive.paths("pages/", ".blocknote.json") {
            let page: Page = archive.read_json(&path)?;
            if page.id.is_empty() {
                continue;
            }
            let keep = match &page.teamspace_id {
                Some(teamspace) => full && teamspaces.contains(teamspace),
                None => private,
            };
            if keep {
                pages.push((page.id, page.parent_id, page.position, path, page.title));
            }
        }
        Ok(pages)
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
            listed[*position].2,
            listed[*position].3.clone(),
        )
    });
    Ok(order
        .into_iter()
        .map(|position| {
            let (id, _, _, path, title) = &listed[position];
            let parent = (depth[position].unwrap_or_default() > 0)
                .then(|| parent_of(position))
                .flatten()
                .map(|parent| listed[parent].0.clone());
            Item {
                kind: "page",
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

pub(super) async fn import_pages(
    service: &OrbitImportService,
    run: &mut Run,
) -> Result<(), Failure> {
    let mut teamspaces = HashMap::new();
    for source in &run.selection.teamspace_ids.clone() {
        if let Some(id) = service.linked(run, "teamspace", source).await? {
            teamspaces.insert(source.clone(), id);
        }
    }
    let comments = read_comments(run).await?;
    let mut items = service.load_items(run.import_id, "page").await?;

    // Pass 1: create every page, parents first. A page an earlier import made is filled at
    // once (its content stays as it is here) so only its new comments are added.
    for index in 0..items.len() {
        if items[index].state != "pending"
            || service
                .skip_if_taken_by_full_import(run, &mut items[index])
                .await?
        {
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
                create_page(service, run, item, parent, &teamspaces, &comments).await
            })
            .await?;
    }
    service.save_report(run).await?;

    // Pass 2: content, files, cover and comments.
    let pages: HashMap<String, Id> = items
        .iter()
        .filter(|item| matches!(item.state.as_str(), "created" | "filled"))
        .filter_map(|item| Some((item.source_id.clone(), item.orbit_id?)))
        .collect();
    for item in &mut items {
        if item.state != "created" {
            continue;
        }
        let filled = service
            .step(run, item, async |service, run, item| {
                fill_page(service, run, item, &pages, &comments).await?;
                service.set_item_state(run, item, "filled").await
            })
            .await?;
        if filled {
            service.save_report(run).await?;
        }
    }
    Ok(())
}

/// Source page id → its comment threads, from `pages/comments.json` (absent in old archives).
async fn read_comments(run: &Run) -> Result<HashMap<String, Vec<Thread>>, Failure> {
    if run.archive.size("pages/comments.json").is_none() {
        return Ok(HashMap::new());
    }
    let pages: Vec<model::PageComments> = archive::blocking(&run.archive, |archive: &Archive| {
        archive.read_json("pages/comments.json")
    })
    .await?;
    Ok(pages
        .into_iter()
        .map(|page| (page.page_id, page.threads))
        .collect())
}

async fn read_page(run: &Run, path: &str) -> Result<Page, Failure> {
    let path = path.to_owned();
    archive::blocking(&run.archive, move |archive: &Archive| {
        archive.read_json(&path)
    })
    .await
    .map_err(|error| Failure::Item(error.to_string()))
}

async fn create_page(
    service: &OrbitImportService,
    run: &mut Run,
    item: &mut Item,
    parent: Option<Id>,
    teamspaces: &HashMap<String, Id>,
    comments: &HashMap<String, Vec<Thread>>,
) -> Result<(), Failure> {
    let (page_id, existing) = service.item_id(run, item).await?;
    if existing {
        let private = page_is_private(service, page_id).await?;
        import_threads(
            service,
            run,
            page_id,
            private,
            comments.get(&item.source_id),
        )
        .await?;
        return service.set_item_state(run, item, "filled").await;
    }
    let page = read_page(run, &item.path).await?;
    let (parent_id, space) = match parent {
        Some(parent) => (Some(parent), SpaceRequest::default()),
        None => match (&page.teamspace_id, run.mode) {
            (Some(source), ExportMode::Full) => (
                None,
                SpaceRequest {
                    teamspace_id: Some(*teamspaces.get(source).ok_or_else(|| {
                        Failure::Item("Its teamspace could not be imported.".to_owned())
                    })?),
                    private: Some(false),
                },
            ),
            _ => (
                None,
                SpaceRequest {
                    teamspace_id: None,
                    private: Some(true),
                },
            ),
        },
    };
    let title = clamp(page.title.trim(), MAX_TITLE_CHARS, MAX_TITLE_BYTES);
    let input = CreatePage {
        parent_id,
        space,
        title: if title.is_empty() {
            "Untitled".to_owned()
        } else {
            title
        },
        icon: page.icon.clone().filter(|icon| icon.chars().count() <= 64),
        position: None,
        kind: PageKind::Page,
    };
    match service
        .inner
        .pages
        .create_page_with_id(
            page_id,
            run.workspace_id,
            run.user_id,
            input,
            &run.request_id,
            source_time(&page.created_at),
        )
        .await
    {
        Ok(_) => {}
        Err(PageError::Unavailable(error)) => return Err(Failure::from(error)),
        Err(PageError::TrialLimit(reached)) => {
            return Err(Failure::Fatal(format!(
                "The import stopped at the trial's limit. {}",
                reached.detail()
            )));
        }
        Err(_) if parent.is_some() => {
            return Err(Failure::Item(
                "The parent page was deleted during the import.".to_owned(),
            ));
        }
        Err(error) => return Err(Failure::Item(format!("Orbit refused the page: {error}."))),
    }
    run.report.created.pages += 1;
    service.set_item_state(run, item, "created").await
}

async fn page_is_private(service: &OrbitImportService, page_id: Id) -> Result<bool, Failure> {
    let teamspace: Option<Option<String>> =
        sqlx::query_scalar("SELECT teamspace_id FROM pages WHERE id = $1")
            .bind(page_id.to_string())
            .fetch_optional(service.database().pool())
            .await?;
    Ok(teamspace.is_none_or(|teamspace| teamspace.is_none()))
}

/// The page's files (attached once, by source file id), then its content with every link to
/// a file or an imported page rewritten, then its comments.
async fn fill_page(
    service: &OrbitImportService,
    run: &mut Run,
    item: &Item,
    pages: &HashMap<String, Id>,
    comments: &HashMap<String, Vec<Thread>>,
) -> Result<(), Failure> {
    let page_id = item
        .orbit_id
        .ok_or_else(|| Failure::Item("The page was not created.".to_owned()))?;
    let page = read_page(run, &item.path).await?;
    let source_workspace: Id = run
        .source_workspace_id
        .parse()
        .map_err(|_| Failure::Fatal("Stored import data is invalid.".to_owned()))?;
    let source_page: Option<Id> = page.id.parse().ok();

    let mut urls: HashMap<String, String> = HashMap::new();
    for file in &page.files {
        let file_id = match service.linked(run, "page_file", &file.id).await? {
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
                let stored = match service
                    .inner
                    .page_files
                    .create_from_bytes(
                        run.workspace_id,
                        page_id,
                        run.user_id,
                        &name,
                        &bytes,
                        &run.request_id,
                    )
                    .await
                {
                    Ok(stored) => stored,
                    Err(error) => {
                        tracing::debug!(error = %error, "imported page file could not be attached");
                        continue;
                    }
                };
                run.report.created.files += 1;
                service
                    .record_link(run, "page_file", &file.id, stored.id)
                    .await?;
                stored.id
            }
        };
        if let (Some(source_page), Ok(source_file)) = (source_page, file.id.parse::<Id>()) {
            urls.insert(
                page_file_url(source_workspace, source_page, source_file),
                page_file_url(run.workspace_id, page_id, file_id),
            );
        }
    }

    // Links to pages: the ones of this import, else ones an earlier import made.
    let mut page_ids = HashMap::new();
    for source in referenced_pages(&page.content) {
        let target = match pages.get(&source) {
            Some(id) => Some(*id),
            None => service.linked(run, "page", &source).await?,
        };
        if let Some(target) = target {
            page_ids.insert(source, target);
        }
    }
    let mut content = page.content;
    for block in &mut content {
        rewrite_strings(block, &|text| rewrite(text, &urls, &page_ids));
    }
    let cover_url = page
        .cover_url
        .as_deref()
        .map(|url| match parse_page_file_url(url) {
            Some(_) => urls.get(url).cloned(),
            None if url.starts_with("https://") => Some(url.to_owned()),
            None => None,
        })
        .filter(|url| url.is_some());

    let current = service
        .inner
        .pages
        .get_page(run.workspace_id, page_id, run.user_id)
        .await
        .map_err(page_failure)?;
    service
        .inner
        .pages
        .update_page(
            run.workspace_id,
            page_id,
            run.user_id,
            current.version,
            PageChanges {
                content: Some(content),
                cover_url,
                origin: SaveOrigin::Import,
                ..PageChanges::default()
            },
            &run.request_id,
            TimestampMillis::now(),
        )
        .await
        .map_err(page_failure)?;
    import_threads(
        service,
        run,
        page_id,
        page.teamspace_id.is_none(),
        comments.get(&item.source_id),
    )
    .await
}

fn page_failure(error: PageError) -> Failure {
    match error {
        PageError::Unavailable(error) => Failure::from(error),
        PageError::TrialLimit(reached) => Failure::Fatal(format!(
            "The import stopped at the trial's limit. {}",
            reached.detail()
        )),
        other => Failure::Item(format!("Orbit refused the page: {other}.")),
    }
}

/// Source page ids the content links to (`/docs/<id>` hrefs and `page` blocks).
fn referenced_pages(content: &[Value]) -> HashSet<String> {
    fn walk(value: &Value, found: &mut HashSet<String>) {
        match value {
            Value::String(text) => {
                if let Some(id) = text.strip_prefix("/docs/")
                    && id.parse::<Id>().is_ok()
                {
                    found.insert(id.to_owned());
                }
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, found)),
            Value::Object(map) => {
                if map.get("type").and_then(Value::as_str) == Some("page")
                    && let Some(Value::String(id)) =
                        map.get("props").and_then(|props| props.get("pageId"))
                {
                    found.insert(id.clone());
                }
                map.values().for_each(|item| walk(item, found));
            }
            _ => {}
        }
    }
    let mut found = HashSet::new();
    content.iter().for_each(|block| walk(block, &mut found));
    found
}

/// A string of the content with its file URL or page link pointed at the import.
fn rewrite(
    text: &str,
    urls: &HashMap<String, String>,
    pages: &HashMap<String, Id>,
) -> Option<String> {
    if let Some(url) = urls.get(text) {
        return Some(url.clone());
    }
    if let Some(id) = text.strip_prefix("/docs/") {
        return pages.get(id).map(|target| format!("/docs/{target}"));
    }
    pages.get(text).map(ToString::to_string)
}

/// Applies `f` to every string value of a block (`pageId` props, `url` and `href` props, text).
fn rewrite_strings(value: &mut Value, f: &dyn Fn(&str) -> Option<String>) {
    match value {
        Value::String(text) => {
            if let Some(replaced) = f(text) {
                *text = replaced;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rewrite_strings(item, f)),
        Value::Object(map) => map.values_mut().for_each(|item| rewrite_strings(item, f)),
        _ => {}
    }
}

/// The page's comment threads, each once. A matched author writes their own comment on a
/// teamspace page of a full import; everyone else's comment, every comment of a private page
/// (only the importer can see it) and every comment of a personal import (the archive is the
/// importer's own upload) is written by the importer with a name and time header.
async fn import_threads(
    service: &OrbitImportService,
    run: &mut Run,
    page_id: Id,
    private: bool,
    threads: Option<&Vec<Thread>>,
) -> Result<(), Failure> {
    let Some(threads) = threads else {
        return Ok(());
    };
    let private = private || run.mode == ExportMode::Personal;
    for thread in threads {
        let mut comments: Vec<&model::PageComment> = thread
            .comments
            .iter()
            .filter(|comment| !comment.deleted)
            .collect();
        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        let mut pending = comments.into_iter();
        let thread_id = match service.linked(run, "page_thread", &thread.id).await? {
            Some(id) => id,
            None => {
                let Some(first) = pending.next() else {
                    continue;
                };
                let (actor_id, body) = comment_body(run, first, private);
                let Some(body) = body else {
                    continue;
                };
                let created = match service
                    .inner
                    .page_comments
                    .create_thread(
                        run.workspace_id,
                        page_id,
                        actor_id,
                        body,
                        thread.quote.as_deref().unwrap_or_default(),
                        &run.request_id,
                        source_time(&thread.created_at),
                    )
                    .await
                {
                    Ok(created) => created,
                    Err(PageCommentError::Unavailable(error)) => return Err(Failure::from(error)),
                    Err(error) => {
                        tracing::debug!(error = %error, "imported page thread refused");
                        continue;
                    }
                };
                service
                    .record_link(run, "page_thread", &thread.id, created.id)
                    .await?;
                if let Some(comment) = created.comments.first() {
                    service
                        .record_link(run, "page_comment", &first.id, comment.id)
                        .await?;
                }
                if thread.resolved_at.is_some() {
                    let _ = service
                        .inner
                        .page_comments
                        .set_resolved(
                            run.workspace_id,
                            page_id,
                            created.id,
                            run.user_id,
                            true,
                            &run.request_id,
                            TimestampMillis::now(),
                        )
                        .await;
                }
                created.id
            }
        };
        for comment in pending {
            if service
                .linked(run, "page_comment", &comment.id)
                .await?
                .is_some()
            {
                continue;
            }
            let (actor_id, body) = comment_body(run, comment, private);
            let Some(body) = body else {
                continue;
            };
            match service
                .inner
                .page_comments
                .add_comment(
                    run.workspace_id,
                    page_id,
                    thread_id,
                    actor_id,
                    body,
                    &run.request_id,
                    source_time(&comment.created_at),
                )
                .await
            {
                Ok(created) => {
                    service
                        .record_link(run, "page_comment", &comment.id, created.id)
                        .await?;
                }
                Err(PageCommentError::Unavailable(error)) => return Err(Failure::from(error)),
                Err(error) => tracing::debug!(error = %error, "imported page comment refused"),
            }
        }
    }
    Ok(())
}

/// Who writes the comment here and its body: the matched author, or the importer with a header.
fn comment_body(
    run: &mut Run,
    comment: &model::PageComment,
    private: bool,
) -> (Id, Option<CommentBody>) {
    let author = comment
        .author
        .as_ref()
        .and_then(|person| run.member(&person.id, person.name.as_deref()));
    let name = comment
        .author
        .as_ref()
        .map(|person| run.name_of(&person.id, person.name.as_deref()))
        .unwrap_or_else(|| "Unknown member".to_owned());
    let text = comment.body.trim();
    let paragraph = |content: Vec<Value>| json!({"type": "paragraph", "props": {}, "content": content, "children": []});
    let mut blocks = Vec::new();
    let actor = match author {
        Some(id) if !private => id,
        _ => {
            let header = author_header(&name, &comment.created_at).replace("**", "");
            blocks.push(paragraph(vec![
                json!({"type": "text", "text": header, "styles": {"bold": true}}),
            ]));
            run.user_id
        }
    };
    if !text.is_empty() {
        blocks.push(paragraph(vec![
            json!({"type": "text", "text": clamp(text, 10_000, 40_000), "styles": {}}),
        ]));
    }
    if blocks.is_empty() {
        return (actor, None);
    }
    (actor, CommentBody::parse(Value::Array(blocks)).ok())
}
