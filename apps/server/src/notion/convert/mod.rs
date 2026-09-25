//! Notion block JSON → BlockNote block JSON for Orbit pages. Pure functions, no IO.
//!
//! The caller fetches a page's block tree, downloads its Notion-hosted files
//! ([`collect_file_refs`]), creates Orbit pages, and then converts with a [`Resolver`] that
//! answers "which Orbit page is this Notion page?" and "which Orbit URL has this file?".
//!
//! Output follows Orbit's editor schema (`apps/web/src/features/docs/editor/schema.ts`,
//! BlockNote 0.55 default blocks + the custom `page` and `callout` blocks): every block is
//! `{ id, type, props, content?, children }` with all props spelled out.
//!
//! Lossy mappings are counted in [`ConvertReport`] (see the module docs of each block type in
//! `convert_node`).

mod properties;
mod rich_text;

use std::collections::{BTreeMap, HashMap};

use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use properties::database_row_properties_table;
use rich_text::{plain, resolve_href, rich_text_to_inline, split_color, str_at, text};

use super::files::is_allowed_file_url;
use super::model::{
    BlockNode, FileSource, NotionDataSource, NotionDatabase, NotionFileRef, NotionPage, Parent,
    normalize_id, plain_text,
};

/// Answers the converter's questions about the import. Ids passed in are normalized
/// (dashed, lowercase) Notion ids.
pub trait Resolver {
    /// The Orbit page id for an imported Notion page or database, or `None` when it is not part
    /// of the import (links then point to Notion).
    fn page_link(&self, notion_id: &str) -> Option<String>;
    /// The uploaded Orbit copy of a Notion-hosted file, or `None` when it was not downloaded.
    fn file_url(&self, file: &NotionFileRef) -> Option<ResolvedFile>;
    /// A title for a Notion page when the block itself has none (`link_to_page`, relations).
    fn page_title(&self, _notion_id: &str) -> Option<String> {
        None
    }
    /// A display name for a Notion user id (people properties and mentions without a name).
    fn user_name(&self, _user_id: &str) -> Option<String> {
        None
    }
}

/// An uploaded file: the URL the editor loads and the display name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolvedFile {
    pub url: String,
    pub name: String,
}

/// A [`Resolver`] backed by maps. `files` is keyed by [`NotionFileRef::url`].
#[derive(Clone, Debug, Default)]
pub struct MapResolver {
    pub pages: HashMap<String, String>,
    pub files: HashMap<String, ResolvedFile>,
    pub titles: HashMap<String, String>,
    pub users: HashMap<String, String>,
}

impl Resolver for MapResolver {
    fn page_link(&self, notion_id: &str) -> Option<String> {
        self.pages.get(notion_id).cloned()
    }

    fn file_url(&self, file: &NotionFileRef) -> Option<ResolvedFile> {
        self.files.get(&file.url).cloned()
    }

    fn page_title(&self, notion_id: &str) -> Option<String> {
        self.titles.get(notion_id).cloned()
    }

    fn user_name(&self, user_id: &str) -> Option<String> {
        self.users.get(user_id).cloned()
    }
}

/// What the conversion could not carry over. Merge the reports of all pages for the import
/// summary.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConvertReport {
    /// Blocks dropped, by type (`table_of_contents`, `breadcrumb`, `unsupported:form`, …).
    pub skipped: BTreeMap<String, u32>,
    /// Blocks or values converted with information loss, by kind (`column_list`, `equation`, …).
    pub lossy: BTreeMap<String, u32>,
    /// Page links, mentions, child pages and relations to Notion pages outside the import.
    pub unresolved_page_links: u32,
    /// Notion-hosted files the resolver had no upload for.
    pub missing_files: u32,
    /// Top-level blocks cut because the page exceeded `max_content_bytes`.
    pub truncated_blocks: u32,
}

impl ConvertReport {
    /// Adds another page's report to this one.
    pub fn merge(&mut self, other: &ConvertReport) {
        for (key, count) in &other.skipped {
            *self.skipped.entry(key.clone()).or_default() += count;
        }
        for (key, count) in &other.lossy {
            *self.lossy.entry(key.clone()).or_default() += count;
        }
        self.unresolved_page_links += other.unresolved_page_links;
        self.missing_files += other.missing_files;
        self.truncated_blocks += other.truncated_blocks;
    }

    /// True when nothing was skipped, lost, unresolved, missing or cut.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self == &Self::default()
    }
}

/// Conversion limits.
#[derive(Clone, Debug)]
pub struct ConvertOptions {
    /// Cut top-level blocks (and add a note) once the serialized content would exceed this.
    /// Default 900 KB, below the 1 MiB page body limit.
    pub max_content_bytes: Option<usize>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            max_content_bytes: Some(900 * 1024),
        }
    }
}

/// State for converting one page: resolver, block id generator, options and report.
pub struct ConvertContext<'a> {
    resolver: &'a dyn Resolver,
    next_id: Box<dyn FnMut() -> String + 'a>,
    options: ConvertOptions,
    report: ConvertReport,
}

impl<'a> ConvertContext<'a> {
    /// A context that generates UUIDv7 block ids.
    pub fn new(resolver: &'a dyn Resolver) -> Self {
        Self {
            resolver,
            next_id: Box::new(|| Id::new_v7().to_string()),
            options: ConvertOptions::default(),
            report: ConvertReport::default(),
        }
    }

    /// Replaces the block id generator (tests use a counter for stable output).
    #[must_use]
    pub fn with_id_generator(mut self, next_id: impl FnMut() -> String + 'a) -> Self {
        self.next_id = Box::new(next_id);
        self
    }

    #[must_use]
    pub fn with_options(mut self, options: ConvertOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn report(&self) -> &ConvertReport {
        &self.report
    }

    #[must_use]
    pub fn into_report(self) -> ConvertReport {
        self.report
    }

    fn id(&mut self) -> String {
        (self.next_id)()
    }

    fn skip(&mut self, kind: &str) {
        *self.report.skipped.entry(kind.to_owned()).or_default() += 1;
    }

    fn lossy(&mut self, kind: &str) {
        *self.report.lossy.entry(kind.to_owned()).or_default() += 1;
    }

    /// `/docs/<orbit id>` for an imported page, else the Notion URL (counted as unresolved).
    fn page_href(&mut self, notion_id: &str, fallback: Option<&str>) -> String {
        if let Some(orbit_id) = self.resolver.page_link(notion_id) {
            return format!("/docs/{orbit_id}");
        }
        self.report.unresolved_page_links += 1;
        match fallback {
            Some(url) if url.starts_with("https://") => url.to_owned(),
            _ => notion_url(notion_id),
        }
    }
}

/// A converted page: Orbit page fields plus BlockNote content.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ConvertedPage {
    pub title: String,
    /// Emoji icon, when the Notion icon is an emoji.
    pub icon: Option<String>,
    pub cover: Option<PageCover>,
    pub content: Vec<Value>,
}

/// A page cover: external URLs are kept as `cover_url`; Notion-hosted covers are uploaded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PageCover {
    External { url: String },
    File(NotionFileRef),
}

/// Converts a page. Database rows (data source parent) get their properties as a table first.
/// Content larger than `max_content_bytes` is cut at a top-level block with a note.
pub fn convert_page(
    page: &NotionPage,
    blocks: &[BlockNode],
    ctx: &mut ConvertContext<'_>,
) -> ConvertedPage {
    let mut content = Vec::new();
    if matches!(
        page.parent,
        Some(Parent::DataSourceId { .. } | Parent::DatabaseId { .. })
    ) && let Some(table) = database_row_properties_table(page, ctx)
    {
        content.push(table);
    }
    convert_nodes(blocks, ctx, &mut content);
    truncate(&mut content, ctx);
    ConvertedPage {
        title: page_title(page),
        icon: page_icon(page),
        cover: page_cover(page),
        content,
    }
}

/// Converts blocks (no properties table, no truncation).
pub fn convert_blocks(blocks: &[BlockNode], ctx: &mut ConvertContext<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    convert_nodes(blocks, ctx, &mut out);
    out
}

/// The page title (plain text of the `title` property), or `Untitled`.
#[must_use]
pub fn page_title(page: &NotionPage) -> String {
    let title = page
        .properties
        .values()
        .find(|value| str_at(value, &["type"]) == Some("title"))
        .and_then(|value| value.get("title"))
        .and_then(Value::as_array)
        .map(|items| plain_text(items))
        .unwrap_or_default();
    non_empty_title(&title)
}

/// The database title, or `Untitled`.
#[must_use]
pub fn database_title(database: &NotionDatabase) -> String {
    non_empty_title(&plain_text(&database.title))
}

/// The data source title, or `Untitled`.
#[must_use]
pub fn data_source_title(data_source: &NotionDataSource) -> String {
    non_empty_title(&plain_text(&data_source.title))
}

/// The page icon when it is an emoji; custom emojis, native icons and image icons are `None`.
#[must_use]
pub fn page_icon(page: &NotionPage) -> Option<String> {
    emoji_icon(page.icon.as_ref())
}

/// The page cover: an external URL, or a Notion-hosted file to download.
#[must_use]
pub fn page_cover(page: &NotionPage) -> Option<PageCover> {
    let cover = page.cover.as_ref()?;
    match str_at(cover, &["type"])? {
        "external" => str_at(cover, &["external", "url"]).map(|url| PageCover::External {
            url: url.to_owned(),
        }),
        "file" => file_ref(cover, FileSource::Cover, &page.id, None).map(PageCover::File),
        _ => None,
    }
}

/// Every Notion-hosted file of a page (cover and blocks) that the caller should download
/// before converting. External files are not included (they stay links).
#[must_use]
pub fn collect_file_refs(page: Option<&NotionPage>, blocks: &[BlockNode]) -> Vec<NotionFileRef> {
    let mut refs = Vec::new();
    if let Some(PageCover::File(file)) = page.and_then(page_cover) {
        refs.push(file);
    }
    collect_block_files(blocks, &mut refs);
    refs
}

fn collect_block_files(nodes: &[BlockNode], refs: &mut Vec<NotionFileRef>) {
    for node in nodes {
        let block = &node.block;
        if let Some(source) = media_source(&block.kind)
            && let Some(file) = file_ref(block.data(), source, &block.id, media_name(block.data()))
        {
            refs.push(file);
        }
        if block.kind == "embed"
            && let Some(url) = str_at(block.data(), &["url"])
            && is_allowed_file_url(url)
        {
            refs.push(NotionFileRef {
                source: FileSource::File,
                owner_id: block.id.clone(),
                url: url.to_owned(),
                name: None,
                expiry_time: None,
            });
        }
        collect_block_files(&node.children, refs);
    }
}

fn media_source(kind: &str) -> Option<FileSource> {
    Some(match kind {
        "image" => FileSource::Image,
        "file" => FileSource::File,
        "pdf" => FileSource::Pdf,
        "video" => FileSource::Video,
        "audio" => FileSource::Audio,
        _ => return None,
    })
}

fn media_name(data: &Value) -> Option<String> {
    str_at(data, &["name"])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// A `type: "file"` file object → [`NotionFileRef`].
fn file_ref(
    file: &Value,
    source: FileSource,
    owner_id: &str,
    name: Option<String>,
) -> Option<NotionFileRef> {
    if str_at(file, &["type"]) != Some("file") {
        return None;
    }
    Some(NotionFileRef {
        source,
        owner_id: owner_id.to_owned(),
        url: str_at(file, &["file", "url"])?.to_owned(),
        name,
        expiry_time: str_at(file, &["file", "expiry_time"]).map(str::to_owned),
    })
}

fn convert_nodes(nodes: &[BlockNode], ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    for node in nodes {
        convert_node(node, ctx, out);
    }
}

/// One Notion block → zero or more BlockNote blocks appended to `out`.
fn convert_node(node: &BlockNode, ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    let block = &node.block;
    if block.in_trash {
        return;
    }
    let data = block.data();
    match block.kind.as_str() {
        "paragraph" => {
            let content = inline(data, ctx);
            let children = convert_blocks(&node.children, ctx);
            let props = default_props(data);
            out.push(make_block(
                ctx,
                "paragraph",
                props,
                items(content),
                children,
            ));
        }
        "heading_1" | "heading_2" | "heading_3" | "heading_4" => {
            let level = block.kind[8..].parse::<u8>().unwrap_or(1);
            let toggleable = data.get("is_toggleable").and_then(Value::as_bool) == Some(true);
            let content = inline(data, ctx);
            let children = convert_blocks(&node.children, ctx);
            let mut props = default_props(data);
            props.insert("level".into(), json!(level));
            props.insert("isToggleable".into(), json!(toggleable));
            out.push(make_block(ctx, "heading", props, items(content), children));
        }
        "bulleted_list_item" => simple(node, "bulletListItem", default_props(data), ctx, out),
        "numbered_list_item" => {
            let mut props = default_props(data);
            if let Some(start) = data.get("list_start_index").and_then(Value::as_u64) {
                props.insert("start".into(), json!(start));
            }
            if str_at(data, &["list_format"]).is_some_and(|format| format != "numbers") {
                ctx.lossy("numbered_list_format");
            }
            simple(node, "numberedListItem", props, ctx, out);
        }
        "to_do" => {
            let mut props = default_props(data);
            let checked = data.get("checked").and_then(Value::as_bool) == Some(true);
            props.insert("checked".into(), json!(checked));
            simple(node, "checkListItem", props, ctx, out);
        }
        "toggle" => simple(node, "toggleListItem", default_props(data), ctx, out),
        "quote" => simple(node, "quote", quote_props(data), ctx, out),
        "callout" => callout(node, ctx, out),
        "code" => code(block.data(), ctx, out),
        "divider" => out.push(make_block(ctx, "divider", Map::new(), None, Vec::new())),
        "table" => table(node, ctx, out),
        "equation" => {
            ctx.lossy("equation");
            let expression = str_at(data, &["expression"]).unwrap_or_default();
            let content = if expression.is_empty() {
                Vec::new()
            } else {
                vec![text(expression, code_style())]
            };
            let props = default_props(&Value::Null);
            out.push(make_block(
                ctx,
                "paragraph",
                props,
                items(content),
                Vec::new(),
            ));
        }
        "child_page" => {
            let title = str_at(data, &["title"]).unwrap_or_default().to_owned();
            page_reference(&block.id, &title, ctx, out);
        }
        "child_database" => {
            let title = str_at(data, &["title"]).unwrap_or_default().to_owned();
            page_reference(&block.id, &title, ctx, out);
        }
        "link_to_page" => {
            let target = str_at(data, &["type"])
                .and_then(|kind| str_at(data, &[kind]))
                .unwrap_or_default();
            let title = normalize_id(target)
                .and_then(|id| ctx.resolver.page_title(&id))
                .unwrap_or_default();
            page_reference(target, &title, ctx, out);
        }
        "image" | "file" | "pdf" | "video" | "audio" => media(node, ctx, out),
        "bookmark" | "link_preview" => {
            let url = str_at(data, &["url"]).unwrap_or_default().to_owned();
            link_paragraph(&url, data, ctx, out);
        }
        "embed" => embed(node, ctx, out),
        "column_list" => {
            ctx.lossy("column_list");
            for column in &node.children {
                convert_nodes(&column.children, ctx, out);
            }
        }
        "column" => convert_nodes(&node.children, ctx, out),
        "synced_block" => {
            if node.children.is_empty()
                && data.get("synced_from").is_some_and(|from| !from.is_null())
            {
                ctx.skip("synced_block_unavailable");
            }
            convert_nodes(&node.children, ctx, out);
        }
        "tab" => {
            ctx.lossy("tab");
            for tab in &node.children {
                let tab_data = tab.block.data();
                let label = inline(tab_data, ctx);
                if !label.is_empty() {
                    let mut props = default_props(&Value::Null);
                    props.insert("level".into(), json!(3));
                    props.insert("isToggleable".into(), json!(false));
                    out.push(make_block(ctx, "heading", props, items(label), Vec::new()));
                }
                convert_nodes(&tab.children, ctx, out);
            }
        }
        "meeting_notes" | "transcription" => {
            ctx.lossy("meeting_notes");
            let title = data
                .get("title")
                .and_then(Value::as_array)
                .map(|items| plain_text(items))
                .unwrap_or_default();
            if !title.is_empty() {
                let mut styles = Map::new();
                styles.insert("bold".into(), Value::Bool(true));
                let props = default_props(&Value::Null);
                out.push(make_block(
                    ctx,
                    "paragraph",
                    props,
                    items(vec![text(title, styles)]),
                    Vec::new(),
                ));
            }
            convert_nodes(&node.children, ctx, out);
        }
        "table_of_contents" | "breadcrumb" | "template" | "table_row" => ctx.skip(&block.kind),
        "unsupported" => {
            let inner = str_at(data, &["block_type"]).unwrap_or("unknown");
            ctx.skip(&format!("unsupported:{inner}"));
        }
        other => {
            // A block type newer than this code: keep any nested content.
            ctx.skip(other);
            convert_nodes(&node.children, ctx, out);
        }
    }
}

/// Inline-content block whose children nest.
fn simple(
    node: &BlockNode,
    kind: &str,
    props: Map<String, Value>,
    ctx: &mut ConvertContext<'_>,
    out: &mut Vec<Value>,
) {
    let content = inline(node.block.data(), ctx);
    let children = convert_blocks(&node.children, ctx);
    out.push(make_block(ctx, kind, props, items(content), children));
}

/// Default callout emoji (same as the editor's `callout` block).
const DEFAULT_CALLOUT_EMOJI: &str = "💡";

/// Callout → Orbit's `callout` block (emoji, colors, rich text, nested children). A Notion icon
/// that is not an emoji (native icon, custom emoji, image) becomes the default emoji and is
/// counted as `callout_icon`.
fn callout(node: &BlockNode, ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    let data = node.block.data();
    let icon = data.get("icon").filter(|icon| !icon.is_null());
    let emoji = emoji_icon(icon).unwrap_or_else(|| {
        if icon.is_some() {
            ctx.lossy("callout_icon");
        }
        DEFAULT_CALLOUT_EMOJI.to_owned()
    });
    let mut props = Map::new();
    props.insert("emoji".into(), json!(emoji));
    props.extend(quote_props(data));
    let content = inline(data, ctx);
    let children = convert_blocks(&node.children, ctx);
    out.push(make_block(ctx, "callout", props, items(content), children));
}

fn code(data: &Value, ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    let source = data
        .get("rich_text")
        .and_then(Value::as_array)
        .map(|items| plain_text(items))
        .unwrap_or_default();
    let language = code_language(str_at(data, &["language"]).unwrap_or("plain text"));
    let content = if source.is_empty() {
        Vec::new()
    } else {
        vec![plain(source)]
    };
    let mut props = Map::new();
    props.insert("language".into(), json!(language));
    out.push(make_block(
        ctx,
        "codeBlock",
        props,
        items(content),
        Vec::new(),
    ));
    let caption = rich_array(data, "caption");
    if !caption.is_empty() {
        ctx.lossy("code_caption");
        let content = rich_text_to_inline(caption, ctx);
        let props = default_props(&Value::Null);
        out.push(make_block(
            ctx,
            "paragraph",
            props,
            items(content),
            Vec::new(),
        ));
    }
}

fn table(node: &BlockNode, ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    let data = node.block.data();
    let rows: Vec<&Vec<Value>> = node
        .children
        .iter()
        .filter(|child| child.block.kind == "table_row" && !child.block.in_trash)
        .filter_map(|child| child.block.data().get("cells").and_then(Value::as_array))
        .collect();
    if rows.is_empty() {
        ctx.skip("table_empty");
        return;
    }
    let width = data
        .get("table_width")
        .and_then(Value::as_u64)
        .map(|width| width as usize)
        .filter(|width| *width > 0)
        .unwrap_or_else(|| rows.iter().map(|cells| cells.len()).max().unwrap_or(1));
    let mut table_rows = Vec::with_capacity(rows.len());
    for cells in rows {
        let mut row = Vec::with_capacity(width);
        for index in 0..width {
            let content = cells
                .get(index)
                .and_then(Value::as_array)
                .map(|items| rich_text_to_inline(items, ctx))
                .unwrap_or_default();
            row.push(table_cell(content));
        }
        table_rows.push(json!({"cells": row}));
    }
    let content = table_content(
        width,
        data.get("has_column_header").and_then(Value::as_bool) == Some(true),
        data.get("has_row_header").and_then(Value::as_bool) == Some(true),
        table_rows,
    );
    let children = convert_blocks(
        &node
            .children
            .iter()
            .filter(|child| child.block.kind != "table_row")
            .cloned()
            .collect::<Vec<_>>(),
        ctx,
    );
    out.push(make_block(
        ctx,
        "table",
        table_props(),
        Some(content),
        children,
    ));
}

pub(crate) fn table_cell(content: Vec<Value>) -> Value {
    json!({
        "type": "tableCell",
        "props": {
            "backgroundColor": "default",
            "textColor": "default",
            "textAlignment": "left",
            "colspan": 1,
            "rowspan": 1
        },
        "content": content
    })
}

pub(crate) fn table_content(
    width: usize,
    header_row: bool,
    header_column: bool,
    rows: Vec<Value>,
) -> Value {
    let mut content = Map::new();
    content.insert("type".into(), json!("tableContent"));
    content.insert(
        "columnWidths".into(),
        Value::Array(vec![Value::Null; width]),
    );
    if header_row {
        content.insert("headerRows".into(), json!(1));
    }
    if header_column {
        content.insert("headerCols".into(), json!(1));
    }
    content.insert("rows".into(), Value::Array(rows));
    Value::Object(content)
}

pub(crate) fn table_props() -> Map<String, Value> {
    let mut props = Map::new();
    props.insert("textColor".into(), json!("default"));
    props
}

/// Image/file/pdf/video/audio. Notion-hosted files become image/file blocks with the uploaded
/// URL; external files stay links (the server never fetches them).
fn media(node: &BlockNode, ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    let block = &node.block;
    let data = block.data();
    let kind = block.kind.as_str();
    let caption = plain_text(rich_array(data, "caption"));
    let name = media_name(data);
    match str_at(data, &["type"]) {
        Some("file") => {
            let source = media_source(kind).unwrap_or(FileSource::File);
            let resolved = file_ref(data, source, &block.id, name.clone())
                .and_then(|file| ctx.resolver.file_url(&file));
            let Some(resolved) = resolved else {
                ctx.report.missing_files += 1;
                missing_file_note(kind, name.as_deref(), ctx, out);
                return;
            };
            let mut props = Map::new();
            props.insert("backgroundColor".into(), json!("default"));
            props.insert("name".into(), json!(resolved.name));
            props.insert("url".into(), json!(resolved.url));
            props.insert("caption".into(), json!(caption));
            if kind == "image" {
                props.insert("textAlignment".into(), json!("left"));
                props.insert("showPreview".into(), json!(true));
                out.push(make_block(ctx, "image", props, None, Vec::new()));
            } else {
                if kind != "file" {
                    ctx.lossy(&format!("{kind}_as_file"));
                }
                out.push(make_block(ctx, "file", props, None, Vec::new()));
            }
        }
        Some("external") => {
            ctx.lossy(&format!("external_{kind}_as_link"));
            let url = str_at(data, &["external", "url"])
                .unwrap_or_default()
                .to_owned();
            let label = name
                .or_else(|| (!caption.is_empty()).then(|| caption.clone()))
                .unwrap_or_else(|| url.clone());
            match resolve_href(&url, ctx) {
                Some(href) => {
                    let props = default_props(&Value::Null);
                    let content = vec![rich_text::link(href, vec![plain(label)])];
                    out.push(make_block(
                        ctx,
                        "paragraph",
                        props,
                        items(content),
                        Vec::new(),
                    ));
                }
                None => {
                    let props = default_props(&Value::Null);
                    out.push(make_block(
                        ctx,
                        "paragraph",
                        props,
                        items(vec![plain(label)]),
                        Vec::new(),
                    ));
                }
            }
        }
        _ => {
            ctx.report.missing_files += 1;
            missing_file_note(kind, name.as_deref(), ctx, out);
        }
    }
}

fn missing_file_note(
    kind: &str,
    name: Option<&str>,
    ctx: &mut ConvertContext<'_>,
    out: &mut Vec<Value>,
) {
    let label = match kind {
        "image" => "Image",
        "pdf" => "PDF",
        "video" => "Video",
        "audio" => "Audio",
        _ => "File",
    };
    let message = match name {
        Some(name) => format!("{label} not imported from Notion: {name}"),
        None => format!("{label} not imported from Notion"),
    };
    let mut styles = Map::new();
    styles.insert("italic".into(), Value::Bool(true));
    let props = default_props(&Value::Null);
    out.push(make_block(
        ctx,
        "paragraph",
        props,
        items(vec![text(message, styles)]),
        Vec::new(),
    ));
}

/// Embeds: links, except embeds of Notion-hosted uploads, which become file blocks.
fn embed(node: &BlockNode, ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    let block = &node.block;
    let data = block.data();
    let url = str_at(data, &["url"]).unwrap_or_default().to_owned();
    if is_allowed_file_url(&url) {
        let file = NotionFileRef {
            source: FileSource::File,
            owner_id: block.id.clone(),
            url,
            name: None,
            expiry_time: None,
        };
        ctx.lossy("embed_as_file");
        match ctx.resolver.file_url(&file) {
            Some(resolved) => {
                let mut props = Map::new();
                props.insert("backgroundColor".into(), json!("default"));
                props.insert("name".into(), json!(resolved.name));
                props.insert("url".into(), json!(resolved.url));
                props.insert(
                    "caption".into(),
                    json!(plain_text(rich_array(data, "caption"))),
                );
                out.push(make_block(ctx, "file", props, None, Vec::new()));
            }
            None => {
                ctx.report.missing_files += 1;
                missing_file_note("file", None, ctx, out);
            }
        }
        return;
    }
    link_paragraph(&url, data, ctx, out);
}

/// Bookmark/embed/link preview → paragraph with a link (caption as text when present).
fn link_paragraph(url: &str, data: &Value, ctx: &mut ConvertContext<'_>, out: &mut Vec<Value>) {
    let caption = plain_text(rich_array(data, "caption"));
    let label = if caption.is_empty() {
        url.to_owned()
    } else {
        caption
    };
    if label.is_empty() {
        ctx.skip("empty_link");
        return;
    }
    let content = match resolve_href(url, ctx) {
        Some(href) => vec![rich_text::link(href, vec![plain(label)])],
        None => vec![plain(label)],
    };
    let props = default_props(&Value::Null);
    out.push(make_block(
        ctx,
        "paragraph",
        props,
        items(content),
        Vec::new(),
    ));
}

/// child_page/child_database/link_to_page → `page` block when imported, else a link paragraph.
fn page_reference(
    notion_id: &str,
    title: &str,
    ctx: &mut ConvertContext<'_>,
    out: &mut Vec<Value>,
) {
    let Some(id) = normalize_id(notion_id) else {
        ctx.skip("link_to_page_invalid");
        return;
    };
    if let Some(orbit_id) = ctx.resolver.page_link(&id) {
        let mut props = Map::new();
        props.insert("pageId".into(), json!(orbit_id));
        out.push(make_block(ctx, "page", props, None, Vec::new()));
        return;
    }
    ctx.report.unresolved_page_links += 1;
    let label = non_empty_title(title);
    let content = vec![rich_text::link(notion_url(&id), vec![plain(label)])];
    let props = default_props(&Value::Null);
    out.push(make_block(
        ctx,
        "paragraph",
        props,
        items(content),
        Vec::new(),
    ));
}

/// `{ id, type, props, content?, children }`.
pub(crate) fn make_block(
    ctx: &mut ConvertContext<'_>,
    kind: &str,
    props: Map<String, Value>,
    content: Option<Value>,
    children: Vec<Value>,
) -> Value {
    let mut block = Map::new();
    block.insert("id".into(), json!(ctx.id()));
    block.insert("type".into(), json!(kind));
    block.insert("props".into(), Value::Object(props));
    if let Some(content) = content {
        block.insert("content".into(), content);
    }
    block.insert("children".into(), Value::Array(children));
    Value::Object(block)
}

fn items(content: Vec<Value>) -> Option<Value> {
    Some(Value::Array(content))
}

/// Default props (`textColor`, `backgroundColor`, `textAlignment`) from a block's `color`.
pub(crate) fn default_props(data: &Value) -> Map<String, Value> {
    let mut props = quote_props(data);
    props.insert("textAlignment".into(), json!("left"));
    props
}

/// `textColor` + `backgroundColor` (quotes have no alignment).
fn quote_props(data: &Value) -> Map<String, Value> {
    let (text_color, background) = split_color(str_at(data, &["color"]).unwrap_or("default"));
    let mut props = Map::new();
    props.insert("textColor".into(), json!(text_color.unwrap_or("default")));
    props.insert(
        "backgroundColor".into(),
        json!(background.unwrap_or("default")),
    );
    props
}

fn inline(data: &Value, ctx: &mut ConvertContext<'_>) -> Vec<Value> {
    rich_text_to_inline(rich_array(data, "rich_text"), ctx)
}

fn rich_array<'v>(data: &'v Value, key: &str) -> &'v [Value] {
    data.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn code_style() -> Map<String, Value> {
    let mut styles = Map::new();
    styles.insert("code".into(), Value::Bool(true));
    styles
}

fn emoji_icon(icon: Option<&Value>) -> Option<String> {
    let icon = icon?;
    let emoji = str_at(icon, &["emoji"])?;
    (str_at(icon, &["type"]).is_none_or(|kind| kind == "emoji") && !emoji.is_empty())
        .then(|| emoji.to_owned())
}

/// Notion code languages → BlockNote/Shiki language ids.
fn code_language(language: &str) -> String {
    let language = language.trim().to_ascii_lowercase();
    match language.as_str() {
        "plain text" | "" => "text",
        "c++" => "cpp",
        "c#" => "csharp",
        "f#" => "fsharp",
        "java/c/c++/c#" => "java",
        "vb.net" | "visual basic" => "vb",
        "docker" => "dockerfile",
        "markup" => "html",
        "webassembly" => "wasm",
        "objective-c" => "objective-c",
        other => return other.replace(' ', "-"),
    }
    .to_owned()
}

fn notion_url(notion_id: &str) -> String {
    format!("https://www.notion.so/{}", notion_id.replace('-', ""))
}

fn non_empty_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "Untitled".to_owned()
    } else {
        title.to_owned()
    }
}

const TRUNCATION_NOTE: &str =
    "The rest of this page was not imported because it is larger than Orbit's page size limit.";

/// Keeps whole top-level blocks while the serialized content fits `max_content_bytes`.
fn truncate(content: &mut Vec<Value>, ctx: &mut ConvertContext<'_>) {
    let Some(max) = ctx.options.max_content_bytes else {
        return;
    };
    let sizes: Vec<usize> = content
        .iter()
        .map(|block| serde_json::to_vec(block).map_or(0, |bytes| bytes.len() + 1))
        .collect();
    if sizes.iter().sum::<usize>() + 2 <= max {
        return;
    }
    let budget = max.saturating_sub(1024);
    let mut total = 2;
    let mut keep = 0;
    for size in &sizes {
        if total + size > budget {
            break;
        }
        total += size;
        keep += 1;
    }
    ctx.report.truncated_blocks += u32::try_from(content.len() - keep).unwrap_or(u32::MAX);
    content.truncate(keep);
    let mut styles = Map::new();
    styles.insert("italic".into(), Value::Bool(true));
    let note = items(vec![text(TRUNCATION_NOTE, styles)]);
    let props = default_props(&Value::Null);
    content.push(make_block(ctx, "paragraph", props, note, Vec::new()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(value: Value) -> BlockNode {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn truncates_large_pages_at_block_boundaries() {
        let resolver = MapResolver::default();
        let mut counter = 0;
        let mut ctx = ConvertContext::new(&resolver)
            .with_id_generator(move || {
                counter += 1;
                format!("id-{counter}")
            })
            .with_options(ConvertOptions {
                max_content_bytes: Some(2_000),
            });
        let long = "x".repeat(300);
        let blocks: Vec<BlockNode> = (0..20)
            .map(|index| {
                node(json!({
                    "id": format!("b{index}"), "type": "paragraph",
                    "paragraph": {"rich_text": [{"type": "text", "plain_text": long, "text": {"content": long}}]}
                }))
            })
            .collect();
        let page = NotionPage {
            id: "p".into(),
            ..NotionPage::default()
        };
        let converted = convert_page(&page, &blocks, &mut ctx);
        let size = serde_json::to_vec(&converted.content).unwrap().len();
        assert!(size <= 2_000, "{size}");
        assert!(converted.content.len() < 20);
        assert_eq!(
            converted.content.last().unwrap()["content"][0]["text"],
            json!(TRUNCATION_NOTE)
        );
        assert_eq!(
            ctx.report().truncated_blocks as usize,
            20 - (converted.content.len() - 1)
        );
    }

    #[test]
    fn maps_code_languages() {
        assert_eq!(code_language("plain text"), "text");
        assert_eq!(code_language("C++"), "cpp");
        assert_eq!(code_language("rust"), "rust");
        assert_eq!(code_language("visual basic"), "vb");
    }

    #[test]
    fn merges_reports() {
        let mut total = ConvertReport::default();
        let mut one = ConvertReport::default();
        one.skipped.insert("breadcrumb".into(), 2);
        one.missing_files = 1;
        total.merge(&one);
        total.merge(&one);
        assert_eq!(total.skipped["breadcrumb"], 4);
        assert_eq!(total.missing_files, 2);
        assert!(!total.is_clean());
        assert!(ConvertReport::default().is_clean());
    }
}
