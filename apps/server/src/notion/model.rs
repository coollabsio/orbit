//! Serde models for the Notion API objects the import reads.
//!
//! Notion adds block, property and parent types without a new API version, so every model is
//! tolerant: unknown fields are ignored, unknown enum variants fall into an `Unknown`/`Other`
//! variant, and type-specific payloads stay as [`serde_json::Value`].

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Where a page, database, data source or block lives (`parent` in Notion objects).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Parent {
    PageId {
        page_id: String,
    },
    DatabaseId {
        database_id: String,
    },
    /// A row of a data source (API 2025-09-03+). `database_id` is included for convenience.
    DataSourceId {
        data_source_id: String,
        #[serde(default)]
        database_id: Option<String>,
    },
    /// A page nested under a block (for example inside a toggle or a column).
    BlockId {
        block_id: String,
    },
    /// A top-level page (team-level pages also report a workspace parent).
    Workspace {
        #[serde(default)]
        workspace: bool,
    },
    /// Any parent type this code does not know (for example `agent_id`).
    #[serde(other)]
    Unknown,
}

impl Parent {
    /// The parent's Notion id, when the parent is a page, database, data source or block.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::PageId { page_id } => Some(page_id),
            Self::DatabaseId { database_id } => Some(database_id),
            Self::DataSourceId { data_source_id, .. } => Some(data_source_id),
            Self::BlockId { block_id } => Some(block_id),
            Self::Workspace { .. } | Self::Unknown => None,
        }
    }
}

/// A Notion page (`object: "page"`). Partial page objects (only `id`) also deserialize.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct NotionPage {
    pub id: String,
    #[serde(default)]
    pub parent: Option<Parent>,
    /// Property name → property value (`{ "id", "type", "<type>": … }`).
    #[serde(default)]
    pub properties: Map<String, Value>,
    #[serde(default)]
    pub icon: Option<Value>,
    #[serde(default)]
    pub cover: Option<Value>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub in_trash: bool,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub last_edited_time: Option<String>,
}

/// A data source reference inside a database object.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DataSourceRef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// A Notion database (`object: "database"`). Since 2025-09-03 it holds data sources, not rows.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct NotionDatabase {
    pub id: String,
    #[serde(default)]
    pub title: Vec<Value>,
    #[serde(default)]
    pub data_sources: Vec<DataSourceRef>,
    #[serde(default)]
    pub parent: Option<Parent>,
    #[serde(default)]
    pub icon: Option<Value>,
    #[serde(default)]
    pub cover: Option<Value>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub is_inline: bool,
    #[serde(default)]
    pub in_trash: bool,
}

/// A Notion data source (`object: "data_source"`): the table of rows under a database.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct NotionDataSource {
    pub id: String,
    #[serde(default)]
    pub title: Vec<Value>,
    /// Usually `database_id`: the database that contains this data source.
    #[serde(default)]
    pub parent: Option<Parent>,
    /// The parent of the containing database (page, block or workspace).
    #[serde(default)]
    pub database_parent: Option<Parent>,
    /// Property name → property schema.
    #[serde(default)]
    pub properties: Map<String, Value>,
    #[serde(default)]
    pub icon: Option<Value>,
    #[serde(default)]
    pub in_trash: bool,
}

impl NotionDataSource {
    /// The id of the database that contains this data source.
    #[must_use]
    pub fn database_id(&self) -> Option<&str> {
        match &self.parent {
            Some(Parent::DatabaseId { database_id }) => Some(database_id),
            Some(Parent::DataSourceId { database_id, .. }) => database_id.as_deref(),
            _ => None,
        }
    }
}

/// One item of a search or data source query result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "object", rename_all = "snake_case")]
pub enum SearchResult {
    Page(NotionPage),
    DataSource(NotionDataSource),
    /// A result object this code does not know.
    #[serde(other)]
    Other,
}

/// The kind of objects `search_all` lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchKind {
    Pages,
    DataSources,
    /// No object filter: pages and data sources.
    All,
}

/// Search results; `incomplete` is true when Notion capped the result set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
    pub incomplete: bool,
}

/// One page of search results; `next_cursor` is set while more results follow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchPage {
    pub results: Vec<SearchResult>,
    pub next_cursor: Option<String>,
    pub incomplete: bool,
}

/// Rows of a data source; `incomplete` is true when Notion capped the query (10,000 rows).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryResults {
    pub pages: Vec<NotionPage>,
    pub incomplete: bool,
}

/// A Notion user (`GET /v1/users/me` returns the token's bot, or the person for a PAT).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct NotionUser {
    pub id: String,
    /// `person` or `bot`.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub person: Option<PersonInfo>,
    #[serde(default)]
    pub bot: Option<BotInfo>,
}

impl NotionUser {
    /// The workspace name, reported for bots owned by a workspace (internal connections).
    #[must_use]
    pub fn workspace_name(&self) -> Option<&str> {
        self.bot
            .as_ref()
            .and_then(|bot| bot.workspace_name.as_deref())
            .filter(|name| !name.trim().is_empty())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PersonInfo {
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct BotInfo {
    #[serde(default)]
    pub owner: Option<Value>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub workspace_name: Option<String>,
}

/// A block object. The type-specific payload stays JSON: `fields[kind]`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Block {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub has_children: bool,
    #[serde(default)]
    pub in_trash: bool,
    /// Every other key of the block object, including the `<type>` payload.
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

impl Block {
    /// The type-specific payload (`block[block.type]`), or `Null` when missing.
    #[must_use]
    pub fn data(&self) -> &Value {
        self.fields.get(&self.kind).unwrap_or(&Value::Null)
    }

    /// The block's parent (`parent` key), when present and well-formed.
    #[must_use]
    pub fn parent(&self) -> Option<Parent> {
        serde_json::from_value(self.fields.get("parent")?.clone()).ok()
    }

    /// True for `child_page` and `child_database`: their content is a separate page, so the
    /// block tree never descends into them.
    #[must_use]
    pub fn is_page_boundary(&self) -> bool {
        matches!(self.kind.as_str(), "child_page" | "child_database")
    }
}

/// A block with its fetched children (`children` sits next to the block's own keys).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BlockNode {
    #[serde(flatten)]
    pub block: Block,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BlockNode>,
}

/// A page's content fetched recursively.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockTree {
    pub blocks: Vec<BlockNode>,
    /// Number of blocks fetched (all levels).
    pub block_count: usize,
    /// True when the depth or block cap stopped the fetch early.
    pub truncated: bool,
}

/// Where a Notion-hosted file was found.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSource {
    Image,
    File,
    Pdf,
    Video,
    Audio,
    Cover,
    Icon,
}

/// A Notion-hosted file (`type: "file"`): the caller downloads it right after fetching (the
/// signed URL expires after one hour) and resolves it for the converter by `url`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotionFileRef {
    pub source: FileSource,
    /// The block (or page, for covers and icons) the file belongs to.
    pub owner_id: String,
    /// Signed download URL (valid for about one hour).
    pub url: String,
    /// Display name from Notion (file blocks), when present.
    pub name: Option<String>,
    pub expiry_time: Option<String>,
}

/// Normalizes a Notion id (dashed or 32 hex chars, any case) to the dashed lowercase form.
#[must_use]
pub fn normalize_id(raw: &str) -> Option<String> {
    let hex: String = raw.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let hex = hex.to_ascii_lowercase();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// Concatenates the `plain_text` of a rich text array.
#[must_use]
pub fn plain_text(rich_text: &[Value]) -> String {
    rich_text
        .iter()
        .filter_map(|item| item.get("plain_text").and_then(Value::as_str))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_ids() {
        assert_eq!(
            normalize_id("3C612F56FDD04A30A4D6BDA7D7426309").as_deref(),
            Some("3c612f56-fdd0-4a30-a4d6-bda7d7426309")
        );
        assert_eq!(
            normalize_id("3c612f56-fdd0-4a30-a4d6-bda7d7426309").as_deref(),
            Some("3c612f56-fdd0-4a30-a4d6-bda7d7426309")
        );
        assert_eq!(normalize_id("not-an-id"), None);
    }

    #[test]
    fn tolerates_unknown_parents_and_results() {
        let parent: Parent =
            serde_json::from_value(json!({"type": "agent_id", "agent_id": "x"})).unwrap();
        assert_eq!(parent, Parent::Unknown);
        let result: SearchResult =
            serde_json::from_value(json!({"object": "view", "id": "x"})).unwrap();
        assert_eq!(result, SearchResult::Other);
        let result: SearchResult =
            serde_json::from_value(json!({"object": "page", "id": "p"})).unwrap();
        assert!(matches!(result, SearchResult::Page(page) if page.id == "p"));
    }

    #[test]
    fn block_node_keeps_payload_and_children() {
        let node: BlockNode = serde_json::from_value(json!({
            "object": "block", "id": "b1", "type": "future_type", "has_children": true,
            "future_type": {"x": 1},
            "children": [{"id": "b2", "type": "paragraph", "paragraph": {"rich_text": []}}]
        }))
        .unwrap();
        assert_eq!(node.block.data(), &json!({"x": 1}));
        assert_eq!(node.children.len(), 1);
        assert!(!node.block.fields.contains_key("children"));
    }
}
