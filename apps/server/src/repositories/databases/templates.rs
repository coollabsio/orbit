//! Row templates (spec P2 item 5, D-P2-5). A template is a hidden child page of its database,
//! listed in `database_templates`: its body is edited with the normal editor and its values are
//! normal `page_property_values`, so it is a row in every way but one: every query that reads the
//! rows of a database leaves it out with [`NOT_TEMPLATE`]. The placeholders "me" (a person value
//! adds whoever makes the row) and "today" (a date) are kept in `placeholders_json`, so stored
//! values stay valid row values; reads put them back into `values` in the shape clients send.

use std::collections::{BTreeMap, HashMap, HashSet};

use orbit_platform::{Database, Id, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::super::billing::ensure_trial_capacity;
use super::super::pages::{PageSpace, Remap, content_text, copy_pages, live_subtree};
use super::computed::{self, ComputeScope, Computed};
use super::files::{self, FileCopies};
use super::{
    DatabaseError, DatabaseRepository, DatabaseRow, DatabaseView, DatePoint, NewRow, Property,
    PropertyKind, ViewConfig, Views, block_links, children_in_tx, copy_links, linked_databases,
    open, parse_id, parse_value, properties_in_tx, raw_views, record_with, write_view_config,
};
use super::{relations, unique_id};
use crate::billing::TrialLimit;
use crate::collab::CollabHub;

const MAX_TEMPLATES: usize = 20;

/// `pages` (the table or its alias) is not a template page.
pub(in crate::repositories) const NOT_TEMPLATE: &str =
    "NOT EXISTS (SELECT 1 FROM database_templates WHERE database_templates.page_id = pages.id)";
const IS_TEMPLATE: &str =
    "EXISTS (SELECT 1 FROM database_templates WHERE database_templates.page_id = pages.id)";

const ME: &str = "me";
const TODAY: &str = "today";

/// Which child pages of a database a query reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Children {
    Rows,
    Templates,
}

impl Children {
    pub(super) const fn filter(self) -> &'static str {
        match self {
            Self::Rows => NOT_TEMPLATE,
            Self::Templates => IS_TEMPLATE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Placeholder {
    /// A person value also gets the member who makes the row.
    Me,
    /// A date value is the day the row is made.
    Today,
}

type Placeholders = BTreeMap<Id, Placeholder>;
/// A property's new placeholder after a write (`None` clears it).
pub(super) type PlaceholderChange = (Id, Option<Placeholder>);

/// A row template: a hidden page of the database whose title, icon, values and body fill a new
/// row.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabaseTemplate)]
pub struct DatabaseTemplate {
    /// What "New" lists, 1 to 200 characters.
    pub name: String,
    /// The template page as a row: `position` orders the templates, and `values` hold `"me"` in
    /// a person value and `{"start": "today", "end": null}` as a date where the template uses
    /// them.
    #[serde(flatten)]
    pub row: DatabaseRow,
}

/// A template as `database_templates` stores it.
pub(super) struct StoredTemplate {
    pub page_id: Id,
    pub name: String,
    pub position: i64,
    placeholders: Placeholders,
}

/// A `database_templates` row to copy with its page (duplicate, backup restore).
pub(in crate::repositories) struct TemplateCopy {
    pub page_id: Id,
    pub database_id: Id,
    pub name: String,
    pub position: i64,
    pub placeholders_json: String,
}

/// What a template gives a new row.
pub(super) struct TemplateFill {
    pub page_id: Id,
    pub title: String,
    pub icon: Option<String>,
    pub content: Vec<Value>,
    /// Values that still fit their property, placeholders resolved.
    pub values: Vec<(Id, Value)>,
}

impl TemplateFill {
    /// The row input with the template under it: values sent in the request win, and an empty
    /// title or a missing icon takes the template's.
    pub(super) fn apply(&self, input: &mut NewRow) {
        let sent: HashSet<Id> = input.values.iter().map(|(id, _)| *id).collect();
        let mut values: Vec<(Id, Value)> = self
            .values
            .iter()
            .filter(|(id, _)| !sent.contains(id))
            .cloned()
            .collect();
        values.append(&mut input.values);
        input.values = values;
        if input.title.is_empty() {
            input.title.clone_from(&self.title);
        }
        if input.icon.is_none() {
            input.icon.clone_from(&self.icon);
        }
    }
}

fn parse_placeholders(json: &str) -> Result<Placeholders, DatabaseError> {
    serde_json::from_str(json).map_err(|_| DatabaseError::Corrupt)
}

/// The live templates of a database in order (one with `only`).
pub(super) async fn stored_templates(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    only: Option<Id>,
) -> Result<Vec<StoredTemplate>, DatabaseError> {
    let only = only.map(|id| id.to_string());
    let rows = sqlx::query(
        "SELECT database_templates.page_id AS page_id, database_templates.name AS name, \
         database_templates.position AS position, database_templates.placeholders_json AS placeholders_json \
         FROM database_templates JOIN pages ON pages.id = database_templates.page_id \
         WHERE database_templates.database_id = $1 AND pages.deleted_at IS NULL \
         AND ($2 IS NULL OR database_templates.page_id = $3) \
         ORDER BY database_templates.position, database_templates.page_id",
    )
    .bind(database_id.to_string())
    .bind(&only)
    .bind(&only)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredTemplate {
                page_id: parse_id(&row.get::<String, _>("page_id"))?,
                name: row.get("name"),
                position: row.get("position"),
                placeholders: parse_placeholders(&row.get::<String, _>("placeholders_json"))?,
            })
        })
        .collect()
}

/// The templates as clients see them (one with `only`), with their computed values, and the
/// related pages of their relation cells.
pub(super) async fn templates_in_tx(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    properties: &[Property],
    only: Option<Id>,
) -> Result<(Vec<DatabaseTemplate>, Computed), DatabaseError> {
    let stored = stored_templates(tx, database_id, only).await?;
    let mut rows = children_in_tx(tx, database_id, Children::Templates, only).await?;
    let computed = computed::fill(tx, scope, database_id, properties, &mut rows).await?;
    let mut pages: HashMap<Id, DatabaseRow> = rows.into_iter().map(|row| (row.id, row)).collect();
    let templates = stored
        .into_iter()
        .filter_map(|template| {
            let mut row = pages.remove(&template.page_id)?;
            row.position = template.position;
            with_placeholders(&mut row.values, &template.placeholders, properties);
            Some(DatabaseTemplate {
                name: template.name,
                row,
            })
        })
        .collect();
    Ok((templates, computed))
}

async fn single_template(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    template_id: Id,
) -> Result<DatabaseTemplate, DatabaseError> {
    let properties = properties_in_tx(tx, database_id).await?;
    templates_in_tx(tx, scope, database_id, &properties, Some(template_id))
        .await?
        .0
        .pop()
        .ok_or(DatabaseError::TemplateNotFound)
}

/// The template page as a row, for the value routes.
pub(super) async fn template_row(
    tx: &mut Transaction<'_, Any>,
    scope: &ComputeScope,
    database_id: Id,
    template_id: Id,
) -> Result<DatabaseRow, DatabaseError> {
    Ok(single_template(tx, scope, database_id, template_id)
        .await?
        .row)
}

/// Puts the placeholders into `values` as clients send them.
fn with_placeholders(
    values: &mut BTreeMap<String, Value>,
    placeholders: &Placeholders,
    properties: &[Property],
) {
    for (id, placeholder) in placeholders {
        let Some(property) = properties.iter().find(|property| property.id == *id) else {
            continue;
        };
        match (placeholder, &property.kind) {
            (Placeholder::Me, PropertyKind::Person) => {
                let entry = values.entry(id.to_string()).or_insert_with(|| json!([]));
                if let Some(ids) = entry.as_array_mut() {
                    ids.insert(0, json!(ME));
                }
            }
            (Placeholder::Today, PropertyKind::Date(_)) => {
                values.insert(id.to_string(), json!({ "start": TODAY, "end": null }));
            }
            _ => {}
        }
    }
}

/// Whether a live child page of the database is a row or a template (`None`: neither).
pub(super) async fn child_kind(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    page_id: Id,
) -> Result<Option<Children>, DatabaseError> {
    let found: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT CASE WHEN {IS_TEMPLATE} THEN 1 ELSE 0 END FROM pages \
         WHERE id = $1 AND parent_id = $2 AND deleted_at IS NULL"
    )))
    .bind(page_id.to_string())
    .bind(database_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    Ok(found.map(|template| {
        if template == 1 {
            Children::Templates
        } else {
            Children::Rows
        }
    }))
}

/// Whether `page_id` is a template of any database (a page move refuses it).
pub(in crate::repositories) async fn is_template(
    tx: &mut Transaction<'_, Any>,
    page_id: Id,
) -> Result<bool, sqlx::Error> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM database_templates WHERE page_id = $1")
            .bind(page_id.to_string())
            .fetch_optional(&mut **tx)
            .await?;
    Ok(found.is_some())
}

/// The ids of the database's live rows in manual order, without `exclude` (and without
/// templates).
pub(super) async fn row_ids(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    space: PageSpace,
    database_id: Id,
    exclude: Id,
) -> Result<Vec<Id>, DatabaseError> {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM pages WHERE workspace_id = $1 AND teamspace_id IS NOT DISTINCT FROM $2 \
         AND owner_id IS NOT DISTINCT FROM $3 AND parent_id = $4 AND deleted_at IS NULL AND id <> $5 \
         AND {NOT_TEMPLATE} ORDER BY position, id"
    )))
    .bind(workspace_id.to_string())
    .bind(space.teamspace_column())
    .bind(space.owner_column())
    .bind(database_id.to_string())
    .bind(exclude.to_string())
    .fetch_all(&mut **tx)
    .await?
    .iter()
    .map(|id| parse_id(id))
    .collect()
}

/// Takes the placeholders out of a template write: `"me"` in a person value and `"today"` (or
/// `{"start": "today"}`) as a date. Returns the values to store and, for each person and date
/// property written, its new placeholder (`None` clears it).
pub(super) fn split_placeholders(
    properties: &[Property],
    input: Vec<(Id, Value)>,
) -> (Vec<(Id, Value)>, Vec<PlaceholderChange>) {
    let mut changes = Vec::new();
    let values = input
        .into_iter()
        .map(|(id, raw)| {
            let kind = properties
                .iter()
                .find(|property| property.id == id)
                .map(|property| &property.kind);
            match kind {
                Some(PropertyKind::Person) => {
                    let me = raw
                        .as_array()
                        .is_some_and(|ids| ids.iter().any(|item| item == ME));
                    changes.push((id, me.then_some(Placeholder::Me)));
                    if me {
                        let rest: Vec<Value> = raw
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|item| *item != ME)
                            .cloned()
                            .collect();
                        return (id, Value::Array(rest));
                    }
                    (id, raw)
                }
                Some(PropertyKind::Date(_)) => {
                    let today = raw == TODAY
                        || (raw["start"] == TODAY
                            && raw.get("end").is_none_or(Value::is_null)
                            && raw.as_object().is_some_and(|map| {
                                map.keys().all(|key| key == "start" || key == "end")
                            }));
                    changes.push((id, today.then_some(Placeholder::Today)));
                    (id, if today { Value::Null } else { raw })
                }
                _ => (id, raw),
            }
        })
        .collect();
    (values, changes)
}

/// Applies placeholder changes from [`split_placeholders`].
pub(super) async fn write_placeholders(
    tx: &mut Transaction<'_, Any>,
    template_id: Id,
    changes: Vec<PlaceholderChange>,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    if changes.is_empty() {
        return Ok(());
    }
    let current: String =
        sqlx::query_scalar("SELECT placeholders_json FROM database_templates WHERE page_id = $1")
            .bind(template_id.to_string())
            .fetch_one(&mut **tx)
            .await?;
    let mut placeholders = parse_placeholders(&current)?;
    for (id, placeholder) in changes {
        match placeholder {
            Some(placeholder) => placeholders.insert(id, placeholder),
            None => placeholders.remove(&id),
        };
    }
    sqlx::query(
        "UPDATE database_templates SET placeholders_json = $1, updated_at = $2 WHERE page_id = $3",
    )
    .bind(serde_json::to_string(&placeholders).map_err(|_| DatabaseError::Corrupt)?)
    .bind(now.as_millis())
    .bind(template_id.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn member_ids(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<HashSet<String>, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role <> 'guest'",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .collect())
}

/// What the template `template_id` gives a row that `actor_id` makes on `today` (`YYYY-MM-DD`):
/// its title, icon and body, and the values that still fit their property (a removed option or
/// member drops out), with "me" and "today" resolved.
pub(super) async fn fill(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    template_id: Id,
    actor_id: Id,
    today: &str,
    properties: &[Property],
) -> Result<TemplateFill, DatabaseError> {
    let template = stored_templates(tx, database_id, Some(template_id))
        .await?
        .pop()
        .ok_or_else(|| DatabaseError::invalid("template_id"))?;
    let page = sqlx::query("SELECT title, icon, content_json FROM pages WHERE id = $1")
        .bind(template_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let content: Vec<Value> = serde_json::from_str(&page.get::<String, _>("content_json"))
        .map_err(|_| DatabaseError::Corrupt)?;
    let members = member_ids(tx, workspace_id).await?;
    let stored = sqlx::query(
        "SELECT property_id, value_json FROM page_property_values WHERE page_id = $1 ORDER BY property_id",
    )
    .bind(template_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let mut values: BTreeMap<Id, Value> = BTreeMap::new();
    for row in stored {
        let property_id = parse_id(&row.get::<String, _>("property_id"))?;
        let Some(property) = properties
            .iter()
            .find(|property| property.id == property_id)
        else {
            continue;
        };
        // A row gets its own number.
        if matches!(property.kind, PropertyKind::UniqueId(_)) {
            continue;
        }
        let raw: Value = serde_json::from_str(&row.get::<String, _>("value_json"))
            .map_err(|_| DatabaseError::Corrupt)?;
        let Ok(Some(value)) = parse_value(&property.kind, &raw) else {
            continue;
        };
        let mut value = value.to_json();
        if let Some(ids) = value.as_array_mut()
            && property.kind == PropertyKind::Person
        {
            ids.retain(|id| id.as_str().is_some_and(|id| members.contains(id)));
        }
        values.insert(property_id, value);
    }
    values.extend(relations::template_values(tx, actor_id, properties, template_id).await?);
    for (property_id, placeholder) in &template.placeholders {
        let Some(property) = properties
            .iter()
            .find(|property| property.id == *property_id)
        else {
            continue;
        };
        match (placeholder, &property.kind) {
            (Placeholder::Me, PropertyKind::Person) if members.contains(&actor_id.to_string()) => {
                let entry = values.entry(*property_id).or_insert_with(|| json!([]));
                if let Some(ids) = entry.as_array_mut()
                    && !ids.contains(&json!(actor_id.to_string()))
                {
                    ids.insert(0, json!(actor_id.to_string()));
                }
            }
            (Placeholder::Today, PropertyKind::Date(_)) => {
                values.insert(*property_id, json!({ "start": today, "end": null }));
            }
            _ => {}
        }
    }
    Ok(TemplateFill {
        page_id: template_id,
        title: page.get("title"),
        icon: page.get("icon"),
        content,
        values: values.into_iter().collect(),
    })
}

/// Writes the body and the live sub-pages of template `fill` into the new row `row_id` in
/// `space`, as a duplicate does ([`copy_pages`]); the sub-pages are pages of the row, not rows.
/// Returns the template's copied files.
#[allow(clippy::too_many_arguments)]
pub(super) async fn copy_into_row(
    database: &Database,
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    fill: &TemplateFill,
    row_id: Id,
    space: PageSpace,
    actor_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<FileCopies, DatabaseError> {
    let sub_pages = live_subtree(tx, fill.page_id).await?;
    let added = i64::try_from(sub_pages.len()).unwrap_or(i64::MAX);
    ensure_trial_capacity(database, tx, workspace_id, TrialLimit::Pages, added).await?;
    let mut pages: HashMap<Id, Id> = sub_pages
        .iter()
        .map(|page| (page.id, Id::new_v7()))
        .collect();
    pages.insert(fill.page_id, row_id);
    let copies = copy_body(
        tx,
        workspace_id,
        fill.page_id,
        &fill.content,
        &pages,
        actor_id,
        now,
    )
    .await?;
    copy_pages(
        tx,
        workspace_id,
        &sub_pages,
        &pages,
        space,
        actor_id,
        request_id,
        now,
    )
    .await?;
    Ok(copies)
}

/// Writes the body of `source_id` (`content`, flushed by the caller) into its copy in `pages`,
/// as a duplicate does: the page's files are copied, and their URLs, the links of linked view
/// blocks and references to pages of `pages` point at the copies. Returns the copied files.
pub(super) async fn copy_body(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    source_id: Id,
    content: &[Value],
    pages: &HashMap<Id, Id>,
    actor_id: Id,
    now: TimestampMillis,
) -> Result<FileCopies, DatabaseError> {
    let target_id = pages[&source_id];
    let file_rows = sqlx::query(
        "SELECT id, blob_id, file_name, mime_type, size_bytes FROM page_files \
         WHERE page_id = $1 AND workspace_id = $2 ORDER BY created_at, id",
    )
    .bind(source_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let mut files = HashMap::new();
    let mut copies = Vec::new();
    for row in file_rows {
        let copy = Id::new_v7();
        files.insert(
            (source_id, parse_id(&row.get::<String, _>("id"))?),
            (target_id, copy),
        );
        copies.push((copy, row));
    }
    let mut link_copies = Vec::new();
    for (link_id, database_id) in linked_databases(tx, workspace_id, &block_links(content)).await? {
        link_copies.push(super::LinkCopy {
            link_id,
            copy_id: Id::new_v7(),
            database_id,
            host_page_id: target_id,
        });
    }
    let links: HashMap<Id, Id> = link_copies
        .iter()
        .map(|copy| (copy.link_id, copy.copy_id))
        .collect();
    let remap = Remap {
        workspace_id,
        pages,
        files: &files,
        links: &links,
    };
    let mut body = Value::Array(content.to_vec());
    remap.value(&mut body, None);
    let Value::Array(body) = body else {
        return Err(DatabaseError::Corrupt);
    };
    sqlx::query("UPDATE pages SET content_json = $1, content_text = $2 WHERE id = $3")
        .bind(serde_json::to_string(&body).map_err(|_| DatabaseError::Corrupt)?)
        .bind(content_text(&body))
        .bind(target_id.to_string())
        .execute(&mut **tx)
        .await?;
    for (file_id, row) in copies {
        sqlx::query(
            "INSERT INTO page_files (id, workspace_id, page_id, blob_id, file_name, mime_type, \
             size_bytes, uploaded_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(file_id.to_string())
        .bind(workspace_id.to_string())
        .bind(target_id.to_string())
        .bind(row.get::<String, _>("blob_id"))
        .bind(row.get::<String, _>("file_name"))
        .bind(row.get::<String, _>("mime_type"))
        .bind(row.get::<i64, _>("size_bytes"))
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
    }
    copy_links(
        tx,
        workspace_id,
        &link_copies,
        pages,
        &HashMap::new(),
        actor_id,
        now,
    )
    .await?;
    Ok(files)
}

/// Drops a default template that is not a live template of the database.
pub(super) async fn retain_default(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    config: &mut ViewConfig,
) -> Result<(), DatabaseError> {
    if let Some(template_id) = config.default_template_id
        && stored_templates(tx, database_id, Some(template_id))
            .await?
            .is_empty()
    {
        config.default_template_id = None;
    }
    Ok(())
}

/// Read-time repair: a view's default template that is gone reads as `null`.
pub(super) fn repair_defaults(views: &mut [DatabaseView], templates: &[DatabaseTemplate]) {
    for view in views {
        if let Some(template_id) = view.config.default_template_id
            && !templates
                .iter()
                .any(|template| template.row.id == template_id)
        {
            view.config.default_template_id = None;
        }
    }
}

/// The templates of `databases`, for a copy (duplicate) of them.
pub(super) async fn load_copies(
    tx: &mut Transaction<'_, Any>,
    databases: &[Id],
) -> Result<Vec<TemplateCopy>, DatabaseError> {
    let mut copies = Vec::new();
    for database_id in databases {
        let rows = sqlx::query(
            "SELECT page_id, name, position, placeholders_json FROM database_templates \
             WHERE database_id = $1 ORDER BY position, page_id",
        )
        .bind(database_id.to_string())
        .fetch_all(&mut **tx)
        .await?;
        for row in rows {
            copies.push(TemplateCopy {
                page_id: parse_id(&row.get::<String, _>("page_id"))?,
                database_id: *database_id,
                name: row.get("name"),
                position: row.get("position"),
                placeholders_json: row.get("placeholders_json"),
            });
        }
    }
    Ok(copies)
}

/// Writes `templates` for copies (`pages`: original id → copy id): a template stays
/// a template only when its database was copied with it; on its own, its copy is a row. The
/// placeholders follow the copied properties.
pub(super) async fn write_copies(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    templates: &[TemplateCopy],
    pages: &HashMap<Id, Id>,
    property_ids: &HashMap<Id, Id>,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    for template in templates {
        let (Some(page), Some(database)) = (
            pages.get(&template.page_id),
            pages.get(&template.database_id),
        ) else {
            continue;
        };
        let placeholders: Placeholders = parse_placeholders(&template.placeholders_json)?
            .into_iter()
            .map(|(id, placeholder)| (property_ids.get(&id).copied().unwrap_or(id), placeholder))
            .collect();
        sqlx::query(
            "INSERT INTO database_templates (page_id, workspace_id, database_id, name, placeholders_json, \
             position, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(page.to_string())
        .bind(workspace_id.to_string())
        .bind(database.to_string())
        .bind(&template.name)
        .bind(serde_json::to_string(&placeholders).map_err(|_| DatabaseError::Corrupt)?)
        .bind(template.position)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// A view config's default template, moved to its copy.
pub(super) fn remap_default(config: &mut ViewConfig, pages: &HashMap<Id, Id>) {
    if let Some(template_id) = config.default_template_id {
        config.default_template_id = pages.get(&template_id).copied();
    }
}

pub struct NewTemplate {
    /// 1 to 200 characters.
    pub name: String,
    /// A live row or template of the database whose title, icon, values and body the template
    /// copies.
    pub from: Option<Id>,
}

#[derive(Default)]
pub struct TemplateChanges {
    pub name: Option<String>,
    pub position: Option<i64>,
}

impl DatabaseRepository {
    /// A new row filled from a template: its title, icon, values and body (see
    /// [`TemplateFill::apply`]). `today` (`YYYY-MM-DD`, the caller's local day) is what "today"
    /// means; the UTC day when absent.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_row_from_template(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewRow,
        template_id: Id,
        today: Option<String>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let today = match today {
            Some(day) => match DatePoint::parse(&day) {
                Some(DatePoint::Day(day)) => day,
                _ => return Err(DatabaseError::invalid("today")),
            },
            None => utc_today(now),
        };
        if let Some(hub) = CollabHub::existing(&self.database) {
            hub.flush_page(template_id).await;
        }
        self.insert_row(
            Id::new_v7(),
            workspace_id,
            database_id,
            actor_id,
            input,
            Some((template_id, today)),
            request_id,
            now,
        )
        .await
    }

    pub async fn create_template(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewTemplate,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseTemplate, DatabaseError> {
        if let Some(from) = input.from
            && let Some(hub) = CollabHub::existing(&self.database)
        {
            hub.flush_page(from).await;
        }
        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let existing = stored_templates(&mut tx, database_id, None).await?;
        if existing.len() >= MAX_TEMPLATES {
            return Err(DatabaseError::TemplateLimit);
        }
        ensure_trial_capacity(&self.database, &mut tx, workspace_id, TrialLimit::Pages, 1).await?;
        let source = match input.from {
            Some(from) => {
                let kind = child_kind(&mut tx, database_id, from)
                    .await?
                    .ok_or_else(|| DatabaseError::invalid("from_row_id"))?;
                let row = sqlx::query("SELECT title, icon, content_json FROM pages WHERE id = $1")
                    .bind(from.to_string())
                    .fetch_one(&mut *tx)
                    .await?;
                let content: Vec<Value> =
                    serde_json::from_str(&row.get::<String, _>("content_json"))
                        .map_err(|_| DatabaseError::Corrupt)?;
                Some((
                    from,
                    kind,
                    row.get::<String, _>("title"),
                    row.get::<Option<String>, _>("icon"),
                    content,
                ))
            }
            None => None,
        };
        let template_id = Id::new_v7();
        let space = page.space(actor_id);
        let (title, icon) = source
            .as_ref()
            .map_or((String::new(), None), |(_, _, title, icon, _)| {
                (title.clone(), icon.clone())
            });
        sqlx::query(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, icon, position, \
             creator_id, updated_by, version, created_at, updated_at, content_json, content_text, property_text) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9, 0, $10, $11, '[]', '', '')",
        )
        .bind(template_id.to_string())
        .bind(workspace_id.to_string())
        .bind(database_id.to_string())
        .bind(space.teamspace_column())
        .bind(space.owner_column())
        .bind(&title)
        .bind(&icon)
        .bind(actor_id.to_string())
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        let placeholders = match &source {
            Some((from, Children::Templates, ..)) => {
                sqlx::query_scalar::<_, String>(
                    "SELECT placeholders_json FROM database_templates WHERE page_id = $1",
                )
                .bind(from.to_string())
                .fetch_one(&mut *tx)
                .await?
            }
            _ => "{}".to_owned(),
        };
        sqlx::query(
            "INSERT INTO database_templates (page_id, workspace_id, database_id, name, placeholders_json, \
             position, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(template_id.to_string())
        .bind(workspace_id.to_string())
        .bind(database_id.to_string())
        .bind(&input.name)
        .bind(&placeholders)
        .bind(existing.len() as i64)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        if let Some((from, _, _, _, content)) = &source {
            sqlx::query(
                "INSERT INTO page_property_values (page_id, property_id, workspace_id, value_json, updated_by, updated_at) \
                 SELECT $1, property_id, workspace_id, value_json, $2, $3 FROM page_property_values WHERE page_id = $4",
            )
            .bind(template_id.to_string())
            .bind(actor_id.to_string())
            .bind(now.as_millis())
            .bind(from.to_string())
            .execute(&mut *tx)
            .await?;
            let copies = copy_body(
                &mut tx,
                workspace_id,
                *from,
                content,
                &HashMap::from([(*from, template_id)]),
                actor_id,
                now,
            )
            .await?;
            files::remap_page(&mut tx, template_id, *from, &copies).await?;
            unique_id::clear(&mut tx, database_id, template_id).await?;
            let properties = properties_in_tx(&mut tx, database_id).await?;
            relations::copy_page_links(&mut tx, &properties, *from, template_id, now).await?;
        }
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.template_created",
            database_id,
            json!({ "template_id": template_id.to_string() }),
            request_id,
            now,
        )
        .await?;
        let template = single_template(
            &mut tx,
            &ComputeScope::utc(workspace_id, actor_id),
            database_id,
            template_id,
        )
        .await?;
        tx.commit().await?;
        Ok(template)
    }

    /// Renames a template or moves it among the templates.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_template(
        &self,
        workspace_id: Id,
        database_id: Id,
        template_id: Id,
        actor_id: Id,
        changes: TemplateChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseTemplate, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let templates = stored_templates(&mut tx, database_id, None).await?;
        if !templates
            .iter()
            .any(|template| template.page_id == template_id)
        {
            return Err(DatabaseError::TemplateNotFound);
        }
        if let Some(name) = &changes.name {
            sqlx::query(
                "UPDATE database_templates SET name = $1, updated_at = $2 WHERE page_id = $3",
            )
            .bind(name)
            .bind(now.as_millis())
            .bind(template_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        if let Some(position) = changes.position {
            let mut ids: Vec<Id> = templates
                .iter()
                .map(|template| template.page_id)
                .filter(|id| *id != template_id)
                .collect();
            let index = usize::try_from(position.max(0))
                .unwrap_or(ids.len())
                .min(ids.len());
            ids.insert(index, template_id);
            renumber_templates(&mut tx, &ids).await?;
        }
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.template_updated",
            database_id,
            json!({ "template_id": template_id.to_string() }),
            request_id,
            now,
        )
        .await?;
        let template = single_template(
            &mut tx,
            &ComputeScope::utc(workspace_id, actor_id),
            database_id,
            template_id,
        )
        .await?;
        tx.commit().await?;
        Ok(template)
    }

    /// Deletes a template page with its sub-pages for good (no trash); views that used it as
    /// their default go back to an empty row.
    pub async fn delete_template(
        &self,
        workspace_id: Id,
        database_id: Id,
        template_id: Id,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let templates = stored_templates(&mut tx, database_id, None).await?;
        if !templates
            .iter()
            .any(|template| template.page_id == template_id)
        {
            return Err(DatabaseError::TemplateNotFound);
        }
        let subtree = sqlx::query_scalar::<_, String>(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT $1 \
                 UNION ALL \
                 SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
             ) \
             SELECT id FROM subtree",
        )
        .bind(template_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        // Children first, so no page is left pointing at a deleted parent mid-way.
        for id in subtree.iter().rev() {
            sqlx::query("DELETE FROM pages WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        let rest: Vec<Id> = templates
            .iter()
            .map(|template| template.page_id)
            .filter(|id| *id != template_id)
            .collect();
        renumber_templates(&mut tx, &rest).await?;
        for mut view in raw_views(&mut tx, database_id, Views::All).await? {
            if view.config.default_template_id == Some(template_id) {
                view.config.default_template_id = None;
                write_view_config(&mut tx, view.id, &view.config, now).await?;
            }
        }
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.template_deleted",
            database_id,
            json!({ "template_id": template_id.to_string() }),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        CollabHub::revalidate_database(&self.database, Some(workspace_id));
        Ok(())
    }
}

async fn renumber_templates(tx: &mut Transaction<'_, Any>, ids: &[Id]) -> Result<(), sqlx::Error> {
    for (position, id) in ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query(
            "UPDATE database_templates SET position = $1 WHERE page_id = $2 AND position <> $3",
        )
        .bind(position)
        .bind(id.to_string())
        .bind(position)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Today's UTC day, for rows made without the client's `today`.
pub(super) fn utc_today(now: TimestampMillis) -> String {
    let days = now.as_millis().div_euclid(86_400_000);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use orbit_platform::TimestampMillis;

    use super::utc_today;

    #[test]
    fn utc_today_is_the_civil_day() {
        assert_eq!(utc_today(TimestampMillis::from_millis(0)), "1970-01-01");
        // 2026-11-10T23:59:59.999Z and the next millisecond.
        assert_eq!(
            utc_today(TimestampMillis::from_millis(1_794_355_199_999)),
            "2026-11-10"
        );
        assert_eq!(
            utc_today(TimestampMillis::from_millis(1_794_355_200_000)),
            "2026-11-11"
        );
        assert_eq!(
            utc_today(TimestampMillis::from_millis(951_782_400_000)),
            "2000-02-29"
        );
    }
}
