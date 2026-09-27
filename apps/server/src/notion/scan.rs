//! The scan: lists every page and data source a Notion token can see and arranges them as a
//! tree. Parents come from each object's `parent`: pages and databases directly, database rows
//! through their data source, pages inside blocks (toggles, columns, …) by walking block parents
//! to the owning page. Anything whose parent the token cannot see becomes a root.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use super::client::{NotionClient, NotionError};
use super::convert::{data_source_title, page_icon, page_title};
use super::import::{MAX_PAGES, clamp};
use super::model::{Parent, SearchKind, SearchResult, normalize_id};

const BLOCK_PARENT_DEPTH: usize = 10;
const MAX_TITLE_CHARS: usize = 500;
const MAX_TITLE_BYTES: usize = 2_000;

/// A Notion page or database in the scan tree.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotionImportNodeKind {
    Page,
    /// Imported as a page named like the database; its rows become sub-pages.
    Database,
}

/// One node of the scan tree.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct NotionImportNode {
    /// Dashed lowercase Notion id; use it in `selection.notion_ids`.
    pub notion_id: String,
    pub kind: NotionImportNodeKind,
    pub title: String,
    /// Emoji icon, when the Notion icon is an emoji.
    #[schema(required = true)]
    pub icon: Option<String>,
    /// The parent node's `notion_id`; `null` for a root (top-level, or its parent is not
    /// visible to the token).
    #[schema(required = true)]
    pub parent_id: Option<String>,
    pub child_count: u32,
}

/// What the token can see. `nodes` are in tree order: every parent before its children,
/// siblings in Notion creation order.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct NotionImportTree {
    pub nodes: Vec<NotionImportNode>,
    /// The token sees more than 5,000 pages; only the first 5,000 are listed.
    pub truncated: bool,
    /// Notion returned an incomplete search result; some pages may be missing.
    pub incomplete: bool,
}

/// Where a scanned object says it lives, before parents are resolved.
enum ParentRef {
    Root,
    Node(String),
    DataSource {
        data_source_id: String,
        database_id: Option<String>,
    },
    Block(String),
}

fn parent_ref(parent: Option<&Parent>) -> ParentRef {
    match parent {
        Some(Parent::PageId { page_id }) => {
            normalize_id(page_id).map_or(ParentRef::Root, ParentRef::Node)
        }
        Some(Parent::DatabaseId { database_id }) => {
            normalize_id(database_id).map_or(ParentRef::Root, ParentRef::Node)
        }
        Some(Parent::DataSourceId {
            data_source_id,
            database_id,
        }) => match normalize_id(data_source_id) {
            Some(data_source_id) => ParentRef::DataSource {
                data_source_id,
                database_id: database_id.as_deref().and_then(normalize_id),
            },
            None => ParentRef::Root,
        },
        Some(Parent::BlockId { block_id }) => {
            normalize_id(block_id).map_or(ParentRef::Root, ParentRef::Block)
        }
        Some(Parent::Workspace { .. } | Parent::Unknown) | None => ParentRef::Root,
    }
}

struct RawNode {
    id: String,
    kind: NotionImportNodeKind,
    title: String,
    icon: Option<String>,
    parent: ParentRef,
    created: String,
}

/// Search results read at most: the tree cap plus room for trashed and duplicate results.
/// Beyond it the scan stops listing and marks the tree `truncated`.
pub(crate) const MAX_SEARCH_RESULTS: usize = MAX_PAGES + 500;
/// Block lookups per scan (to place pages that live inside toggles, columns, …); once spent,
/// further such pages become roots.
pub(crate) const MAX_BLOCK_LOOKUPS: usize = 2_000;

/// Lets a running scan stop early (server shutdown, import cancelled). Polled before every
/// Notion request.
pub(crate) trait ScanControl: Sync {
    fn should_stop(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

/// Lists the pages and data sources the token can see (at most [`MAX_SEARCH_RESULTS`]) and
/// arranges them as a tree. Only a small record per node is kept while listing. Returns
/// `None` when `control` asked to stop.
pub(crate) async fn build_tree(
    client: &NotionClient,
    control: &dyn ScanControl,
) -> Result<Option<NotionImportTree>, NotionError> {
    let mut raw: Vec<RawNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut data_sources: HashMap<String, String> = HashMap::new();
    let mut listed = 0usize;
    let mut incomplete = false;
    let mut more = false;
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    loop {
        if control.should_stop().await {
            return Ok(None);
        }
        let page = client
            .search_page(SearchKind::All, cursor.as_deref())
            .await?;
        incomplete |= page.incomplete;
        for result in page.results {
            listed += 1;
            add_result(result, &mut raw, &mut index, &mut data_sources);
        }
        match page.next_cursor {
            Some(_) if listed >= MAX_SEARCH_RESULTS => {
                more = true;
                break;
            }
            Some(next) if seen_cursors.insert(next.clone()) => cursor = Some(next),
            _ => break,
        }
    }

    let mut block_owners: HashMap<String, Option<String>> = HashMap::new();
    let mut lookups_left = MAX_BLOCK_LOOKUPS;
    let mut parents: Vec<Option<usize>> = Vec::with_capacity(raw.len());
    for (position, node) in raw.iter().enumerate() {
        let parent = match &node.parent {
            ParentRef::Root => None,
            ParentRef::Node(id) => Some(id.clone()),
            ParentRef::DataSource {
                data_source_id,
                database_id,
            } => data_sources
                .get(data_source_id)
                .cloned()
                .or_else(|| database_id.clone()),
            ParentRef::Block(block_id) => {
                match block_owner(
                    client,
                    control,
                    block_id,
                    &data_sources,
                    &mut block_owners,
                    &mut lookups_left,
                )
                .await?
                {
                    Lookup::Owner(owner) => owner,
                    Lookup::Stopped => return Ok(None),
                }
            }
        };
        parents.push(
            parent
                .and_then(|parent| index.get(&parent).copied())
                .filter(|parent| *parent != position),
        );
    }
    // Break cycles (a corrupt or racing hierarchy): a node that reaches itself becomes a root.
    for start in 0..raw.len() {
        let mut current = parents[start];
        let mut steps = 0;
        while let Some(parent) = current {
            if parent == start || steps > raw.len() {
                parents[start] = None;
                break;
            }
            current = parents[parent];
            steps += 1;
        }
    }

    let mut children: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for (position, parent) in parents.iter().enumerate() {
        children.entry(*parent).or_default().push(position);
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| {
            let (a, b) = (&raw[*a], &raw[*b]);
            (a.created.as_str(), a.title.to_lowercase(), a.id.as_str()).cmp(&(
                b.created.as_str(),
                b.title.to_lowercase(),
                b.id.as_str(),
            ))
        });
    }
    let mut order = Vec::with_capacity(raw.len().min(MAX_PAGES));
    let mut stack: Vec<usize> = children
        .get(&None)
        .map(|roots| roots.iter().rev().copied().collect())
        .unwrap_or_default();
    let mut truncated = more;
    while let Some(position) = stack.pop() {
        if order.len() >= MAX_PAGES {
            truncated = true;
            break;
        }
        order.push(position);
        if let Some(list) = children.get(&Some(position)) {
            stack.extend(list.iter().rev().copied());
        }
    }
    let kept: HashSet<usize> = order.iter().copied().collect();
    let nodes = order
        .iter()
        .map(|&position| {
            let node = &raw[position];
            NotionImportNode {
                notion_id: node.id.clone(),
                kind: node.kind,
                title: node.title.clone(),
                icon: node.icon.clone(),
                parent_id: parents[position].map(|parent| raw[parent].id.clone()),
                child_count: children.get(&Some(position)).map_or(0, |list| {
                    u32::try_from(list.iter().filter(|child| kept.contains(child)).count())
                        .unwrap_or(u32::MAX)
                }),
            }
        })
        .collect();
    Ok(Some(NotionImportTree {
        nodes,
        truncated,
        incomplete,
    }))
}

/// Keeps a small record of one search result: pages become page nodes, data sources their
/// database's node. Trashed and duplicate results are skipped.
fn add_result(
    result: SearchResult,
    raw: &mut Vec<RawNode>,
    index: &mut HashMap<String, usize>,
    data_sources: &mut HashMap<String, String>,
) {
    let node = match result {
        SearchResult::DataSource(data_source) => {
            let Some(data_source_id) = normalize_id(&data_source.id) else {
                return;
            };
            if data_source.in_trash {
                return;
            }
            let database_id = data_source
                .database_id()
                .and_then(normalize_id)
                .unwrap_or_else(|| data_source_id.clone());
            data_sources.insert(data_source_id, database_id.clone());
            RawNode {
                id: database_id,
                kind: NotionImportNodeKind::Database,
                title: clamp(
                    &data_source_title(&data_source),
                    MAX_TITLE_CHARS,
                    MAX_TITLE_BYTES,
                ),
                icon: emoji(data_source.icon.as_ref()),
                parent: parent_ref(data_source.database_parent.as_ref()),
                created: String::new(),
            }
        }
        SearchResult::Page(page) => {
            let Some(id) = normalize_id(&page.id) else {
                return;
            };
            if page.in_trash {
                return;
            }
            RawNode {
                id,
                kind: NotionImportNodeKind::Page,
                title: clamp(&page_title(&page), MAX_TITLE_CHARS, MAX_TITLE_BYTES),
                icon: page_icon(&page),
                parent: parent_ref(page.parent.as_ref()),
                created: page.created_time.unwrap_or_default(),
            }
        }
        SearchResult::Other => return,
    };
    if index.contains_key(&node.id) {
        return;
    }
    index.insert(node.id.clone(), raw.len());
    raw.push(node);
}

enum Lookup {
    Owner(Option<String>),
    Stopped,
}

/// The page or database that contains a block (pages inside toggles, columns, …), found by
/// walking block parents. Blocks the token cannot read, and lookups beyond the budget,
/// resolve to `None` (the page becomes a root). Every walked block is cached.
async fn block_owner(
    client: &NotionClient,
    control: &dyn ScanControl,
    block_id: &str,
    data_sources: &HashMap<String, String>,
    cache: &mut HashMap<String, Option<String>>,
    lookups_left: &mut usize,
) -> Result<Lookup, NotionError> {
    let mut visited = Vec::new();
    let mut current = block_id.to_owned();
    let mut owner = None;
    for _ in 0..BLOCK_PARENT_DEPTH {
        if let Some(hit) = cache.get(&current) {
            owner = hit.clone();
            break;
        }
        if *lookups_left == 0 {
            break;
        }
        if control.should_stop().await {
            return Ok(Lookup::Stopped);
        }
        *lookups_left -= 1;
        visited.push(current.clone());
        let block = match client.retrieve_block(&current).await {
            Ok(block) => block,
            Err(
                NotionError::ObjectNotFound { .. }
                | NotionError::RestrictedResource { .. }
                | NotionError::InvalidId
                | NotionError::Decode(_),
            ) => break,
            Err(error) => return Err(error),
        };
        match parent_ref(block.parent().as_ref()) {
            ParentRef::Node(id) => {
                owner = Some(id);
                break;
            }
            ParentRef::DataSource {
                data_source_id,
                database_id,
            } => {
                owner = data_sources.get(&data_source_id).cloned().or(database_id);
                break;
            }
            ParentRef::Block(parent) => current = parent,
            ParentRef::Root => break,
        }
    }
    for block in visited {
        cache.insert(block, owner.clone());
    }
    Ok(Lookup::Owner(owner))
}

fn emoji(icon: Option<&Value>) -> Option<String> {
    let icon = icon?;
    if icon
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "emoji")
    {
        return None;
    }
    icon.get("emoji")
        .and_then(Value::as_str)
        .filter(|emoji| !emoji.is_empty())
        .map(str::to_owned)
}
