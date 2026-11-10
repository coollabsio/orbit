//! CSV import (spec P2 9.3): the client parses the file and maps its columns; the server converts
//! each cell with the type-change rules of P1 4.2 and writes every row in one transaction.
//! Conversion runs before the write transaction and is checked again inside it.

use std::collections::{HashMap, HashSet};

use orbit_platform::{Id, SqlBuilder, TimestampMillis};
use serde::Serialize;
use serde_json::json;
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::search::{SearchRows, refresh_search_text, search_text};
use super::unique_id;
use super::{
    CellValue, DatabaseError, DatabaseRepository, DatabaseSchema, DatabaseView, MAX_CELL_ITEMS,
    MAX_NAME_CHARS, MAX_OPTIONS, MAX_PROPERTIES, MAX_ROWS, MAX_TEXT_CHARS, OptionColor, Property,
    PropertyKind, PropertyType, SelectOption, StatusGroup, StatusOption, ViewConfig, ViewType,
    Views, ensure_row_capacity, insert_property, open, properties_ids, properties_in_tx, raw_views,
    record_with, renumber_rows, scalar_from_text, unlocked, write_view_config,
};
use crate::billing::TrialLimit;
use crate::repositories::billing::ensure_trial_capacity;
use crate::repositories::pages::PageSpace;
use crate::repositories::tasks::require_access_tx;

/// Characters of a row title (the page title limit).
pub const MAX_TITLE_CHARS: usize = 500;
/// CSV columns per import.
pub const MAX_IMPORT_COLUMNS: usize = 100;
/// Rows per insert statement (13 bound values each).
const ROW_BATCH: usize = 200;
/// Values per insert statement (6 bound values each).
const VALUE_BATCH: usize = 500;

/// Where a CSV column goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportTarget {
    Property(Id),
    /// A new property of this type, named after the column. `Title` names the title column of a
    /// new database.
    New(PropertyType),
    Skip,
}

#[derive(Clone, Debug)]
pub struct ImportColumn {
    pub name: String,
    pub target: ImportTarget,
}

/// Parsed CSV rows and their column mapping. A row may be shorter than `columns` (missing cells
/// are empty), never longer.
#[derive(Clone, Debug, Default)]
pub struct CsvImport {
    pub columns: Vec<ImportColumn>,
    pub rows: Vec<Vec<String>>,
}

/// What an import did (or, as a dry run, would do).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, ToSchema)]
#[schema(as = DatabaseImportResult)]
pub struct ImportResult {
    pub created_rows: u64,
    /// Non-empty cells that became a value.
    pub converted: u64,
    /// Non-empty cells that do not fit their property and stay empty.
    pub cleared: u64,
    /// Options added to select, multi-select and status properties.
    pub created_options: u64,
    /// Titles over 500 and texts over 10,000 characters that were cut.
    pub values_truncated: u64,
    /// The same counts per CSV column, in column order (zero for skipped columns).
    pub columns: Vec<ImportColumnResult>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, ToSchema)]
#[schema(as = DatabaseImportColumnResult)]
pub struct ImportColumnResult {
    pub converted: u64,
    pub cleared: u64,
    pub created_options: u64,
    pub values_truncated: u64,
}

/// A workspace member a person cell can name.
#[derive(Clone, Debug)]
pub(super) struct Member {
    id: Id,
    name: String,
    email: String,
}

/// The rows to insert, converted.
#[derive(Clone, Debug, Default)]
pub(super) struct ImportRows {
    rows: Vec<PlannedRow>,
}

impl ImportRows {
    pub(super) fn len(&self) -> usize {
        self.rows.len()
    }
}

#[derive(Clone, Debug)]
struct PlannedRow {
    title: String,
    values: Vec<(Id, CellValue)>,
    search_text: String,
}

/// An import converted against one state of the database's properties.
#[derive(Debug)]
struct ImportPlan {
    /// The properties after the import, in order.
    properties: Vec<Property>,
    /// Properties the import adds.
    added: Vec<Id>,
    /// Existing properties that gain options.
    grown: Vec<Id>,
    rows: ImportRows,
    result: ImportResult,
}

impl ImportPlan {
    fn changes_schema(&self) -> bool {
        !self.added.is_empty() || !self.grown.is_empty()
    }
}

/// Converts `input` for a database with `existing` properties (none for a new database).
fn plan(
    existing: &[Property],
    members: &[Member],
    input: &CsvImport,
) -> Result<ImportPlan, DatabaseError> {
    let columns = &input.columns;
    if columns.is_empty() || columns.len() > MAX_IMPORT_COLUMNS {
        return Err(DatabaseError::invalid("columns"));
    }
    if input.rows.len() > MAX_ROWS as usize {
        return Err(DatabaseError::invalid("rows"));
    }
    if let Some(index) = input.rows.iter().position(|row| row.len() > columns.len()) {
        return Err(DatabaseError::invalid(format!("rows[{index}]")));
    }
    let new_database = existing.is_empty();
    let mut properties: Vec<Property> = existing.to_vec();
    let mut added = Vec::new();
    // Column index → index in `properties`.
    let mut targets: Vec<Option<usize>> = Vec::with_capacity(columns.len());
    let mut title_column = None;
    for (index, column) in columns.iter().enumerate() {
        let target_error = || DatabaseError::invalid(format!("columns[{index}].target"));
        let at = match column.target {
            ImportTarget::Skip => None,
            ImportTarget::Property(id) => {
                if new_database {
                    return Err(target_error());
                }
                let at = properties
                    .iter()
                    .position(|property| property.id == id)
                    .ok_or_else(target_error)?;
                if targets.contains(&Some(at)) {
                    return Err(target_error());
                }
                let kind = &properties[at].kind;
                if (kind.property_type().computed() && *kind != PropertyKind::Title)
                    || !text_fills(kind.property_type())
                {
                    return Err(target_error());
                }
                Some(at)
            }
            ImportTarget::New(property_type) => {
                let title = property_type == PropertyType::Title;
                if (title && !new_database)
                    || (property_type.computed() && !title)
                    || !text_fills(property_type)
                {
                    return Err(target_error());
                }
                let name = column.name.trim();
                if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
                    return Err(DatabaseError::invalid(format!("columns[{index}].name")));
                }
                let property = Property {
                    id: Id::new_v7(),
                    name: name.to_owned(),
                    kind: PropertyKind::default_for(property_type),
                    position: 0,
                };
                added.push(property.id);
                // A new database lists its title first.
                if title {
                    properties.insert(0, property);
                    for target in targets.iter_mut().flatten() {
                        *target += 1;
                    }
                    Some(0)
                } else {
                    properties.push(property);
                    Some(properties.len() - 1)
                }
            }
        };
        if at.is_some_and(|at| properties[at].kind == PropertyKind::Title) {
            if title_column.is_some() {
                return Err(DatabaseError::invalid("columns"));
            }
            title_column = Some(index);
        }
        targets.push(at);
    }
    if title_column.is_none() {
        return Err(DatabaseError::invalid("columns"));
    }
    if properties.len() > MAX_PROPERTIES {
        return Err(DatabaseError::PropertyLimit);
    }
    for (position, property) in properties.iter_mut().enumerate() {
        property.position = position as i64;
    }

    let people = People::new(members);
    let row_count = input.rows.len();
    let mut titles = vec![String::new(); row_count];
    let mut values: Vec<Vec<(Id, CellValue)>> = vec![Vec::new(); row_count];
    let mut result = ImportResult {
        created_rows: row_count as u64,
        columns: vec![ImportColumnResult::default(); columns.len()],
        ..ImportResult::default()
    };
    let mut grown = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        let Some(at) = *target else { continue };
        let cells = input
            .rows
            .iter()
            .map(|row| row.get(index).map_or("", String::as_str));
        let counts = &mut result.columns[index];
        let property = &mut properties[at];
        if property.kind == PropertyKind::Title {
            for (row, cell) in cells.enumerate() {
                let text = cell.trim();
                let cut: String = text.chars().take(MAX_TITLE_CHARS).collect();
                if cut.len() < text.len() {
                    counts.values_truncated += 1;
                }
                if !cut.is_empty() {
                    counts.converted += 1;
                }
                titles[row] = cut;
            }
            continue;
        }
        let options_before = option_count(&property.kind);
        for (row, cell) in cells.enumerate() {
            let text = cell.trim();
            if text.is_empty() {
                continue;
            }
            let value = convert_cell(&mut property.kind, text, &people, counts);
            match value {
                Some(value) => {
                    counts.converted += 1;
                    values[row].push((property.id, value));
                }
                None if property.kind == PropertyKind::Checkbox && unticked(text) => {}
                None => counts.cleared += 1,
            }
        }
        let created = option_count(&property.kind) - options_before;
        counts.created_options = created as u64;
        if created > 0 && !added.contains(&property.id) {
            grown.push(property.id);
        }
    }
    for counts in &result.columns {
        result.converted += counts.converted;
        result.cleared += counts.cleared;
        result.created_options += counts.created_options;
        result.values_truncated += counts.values_truncated;
    }
    let rows = titles
        .into_iter()
        .zip(values)
        .map(|(title, values)| {
            let by_id: HashMap<Id, CellValue> = values.iter().cloned().collect();
            PlannedRow {
                title,
                search_text: search_text(&properties, &by_id),
                values,
            }
        })
        .collect();
    Ok(ImportPlan {
        properties,
        added,
        grown,
        rows: ImportRows { rows },
        result,
    })
}

/// Whether CSV text can fill a property of this type: files are uploads and unique IDs are
/// numbered by the server.
fn text_fills(property_type: PropertyType) -> bool {
    !matches!(property_type, PropertyType::Files | PropertyType::UniqueId)
}

/// "false", "no", "0" and friends: an unticked box, not a value that failed to convert.
fn unticked(text: &str) -> bool {
    ["false", "no", "0", "-"]
        .iter()
        .any(|word| text.eq_ignore_ascii_case(word))
}

fn option_count(kind: &PropertyKind) -> usize {
    kind.options().len()
}

/// One non-empty, trimmed cell under `kind`. Selects match option names (case ignored) and add
/// the missing ones up to [`MAX_OPTIONS`]; multi-selects and people split on ",".
fn convert_cell(
    kind: &mut PropertyKind,
    text: &str,
    people: &People,
    counts: &mut ImportColumnResult,
) -> Option<CellValue> {
    match kind {
        PropertyKind::Text => {
            if text.chars().count() > MAX_TEXT_CHARS {
                counts.values_truncated += 1;
            }
            scalar_from_text(PropertyType::Text, text)
        }
        PropertyKind::Number(_)
        | PropertyKind::Url
        | PropertyKind::Email
        | PropertyKind::Phone
        | PropertyKind::Checkbox
        | PropertyKind::Date(_) => scalar_from_text(kind.property_type(), text),
        PropertyKind::Files | PropertyKind::UniqueId(_) => None,
        PropertyKind::Select(_) | PropertyKind::Status(_) => {
            choose_option(kind, text).map(CellValue::Choice)
        }
        PropertyKind::MultiSelect(_) => {
            let mut ids: Vec<String> = Vec::new();
            for name in text.split(',') {
                if let Some(id) = choose_option(kind, name)
                    && !ids.contains(&id)
                    && ids.len() < MAX_CELL_ITEMS
                {
                    ids.push(id);
                }
            }
            (!ids.is_empty()).then_some(CellValue::Choices(ids))
        }
        PropertyKind::Person => {
            let mut ids: Vec<Id> = Vec::new();
            for name in text.split(',') {
                if let Some(id) = people.find(name)
                    && !ids.contains(&id)
                    && ids.len() < MAX_CELL_ITEMS
                {
                    ids.push(id);
                }
            }
            (!ids.is_empty()).then_some(CellValue::People(ids))
        }
        // A CSV cell holds titles, not page ids: relations import from Notion only.
        PropertyKind::Relation(_) | PropertyKind::Rollup(_) => None,
        PropertyKind::Title
        | PropertyKind::CreatedTime
        | PropertyKind::CreatedBy
        | PropertyKind::LastEditedTime
        | PropertyKind::LastEditedBy
        | PropertyKind::Button(_) => None,
    }
}

/// The id of the option named `name` (case ignored), added when missing and there is room.
fn choose_option(kind: &mut PropertyKind, name: &str) -> Option<String> {
    let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    if name.is_empty() {
        return None;
    }
    let lower = name.to_lowercase();
    if let Some((id, _)) = kind
        .options()
        .into_iter()
        .find(|(_, option)| option.to_lowercase() == lower)
    {
        return Some(id.to_owned());
    }
    if option_count(kind) >= MAX_OPTIONS {
        return None;
    }
    let id = Id::new_v7().to_string();
    match kind {
        PropertyKind::Select(list) | PropertyKind::MultiSelect(list) => list.push(SelectOption {
            id: id.clone(),
            name,
            color: OptionColor::Default,
        }),
        PropertyKind::Status(list) => list.push(StatusOption {
            id: id.clone(),
            name,
            color: OptionColor::Default,
            group: StatusGroup::Todo,
        }),
        _ => return None,
    }
    Some(id)
}

/// Members by lowercased display name and email.
struct People(HashMap<String, Id>);

impl People {
    fn new(members: &[Member]) -> Self {
        let mut by_key = HashMap::new();
        for member in members {
            by_key
                .entry(member.email.to_lowercase())
                .or_insert(member.id);
        }
        for member in members {
            by_key
                .entry(member.name.to_lowercase())
                .or_insert(member.id);
        }
        Self(by_key)
    }

    fn find(&self, text: &str) -> Option<Id> {
        let key = text.trim().to_lowercase();
        if key.is_empty() {
            return None;
        }
        self.0.get(&key).copied()
    }
}

async fn members_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<Vec<Member>, DatabaseError> {
    use orbit_platform::Row;
    let rows = sqlx::query(
        "SELECT users.id AS id, users.display_name AS display_name, users.email AS email \
         FROM memberships JOIN users ON users.id = memberships.user_id \
         WHERE memberships.workspace_id = $1 AND memberships.role <> 'guest' ORDER BY users.id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(Member {
                id: super::parse_id(&row.get::<String, _>("id"))?,
                name: row.get("display_name"),
                email: row.get("email"),
            })
        })
        .collect()
}

/// Runs [`plan`] off the async workers: a full import converts up to 500,000 cells.
async fn plan_blocking(
    existing: Vec<Property>,
    members: Vec<Member>,
    input: CsvImport,
) -> Result<(ImportPlan, CsvImport, Vec<Member>), DatabaseError> {
    tokio::task::spawn_blocking(move || {
        plan(&existing, &members, &input).map(|plan| (plan, input, members))
    })
    .await
    .map_err(|_| DatabaseError::Corrupt)?
}

/// Whether two property lists have the same ids, kinds and order (a plan made for one fits the
/// other).
fn same_schema(left: &[Property], right: &[Property]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.id == right.id && left.kind == right.kind)
}

/// Inserts converted rows at the end of `database_id`, inside the caller's write transaction.
/// Person values keep only current members.
pub(super) async fn insert_rows(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    space: PageSpace,
    actor_id: Id,
    rows: &ImportRows,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    if rows.rows.is_empty() {
        return Ok(());
    }
    let members: HashSet<String> = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role <> 'guest'",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .collect();
    let start: i64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(position), -1) + 1 AS BIGINT) FROM pages WHERE parent_id = $1",
    )
    .bind(database_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    let ids: Vec<String> = rows.rows.iter().map(|_| Id::new_v7().to_string()).collect();
    let workspace = workspace_id.to_string();
    let database = database_id.to_string();
    let actor = actor_id.to_string();
    for (chunk_index, chunk) in rows.rows.chunks(ROW_BATCH).enumerate() {
        let mut query = SqlBuilder::new(
            "INSERT INTO pages (id, workspace_id, parent_id, teamspace_id, owner_id, title, position, \
             creator_id, updated_by, version, created_at, updated_at, content_json, content_text, property_text) VALUES ",
        );
        for (offset, row) in chunk.iter().enumerate() {
            let index = chunk_index * ROW_BATCH + offset;
            if offset > 0 {
                query.push(", ");
            }
            query.push("(");
            query.push_bind(ids[index].clone());
            query.push(", ").push_bind(workspace.clone());
            query.push(", ").push_bind(database.clone());
            query.push(", ").push_bind(space.teamspace_column());
            query.push(", ").push_bind(space.owner_column());
            query.push(", ").push_bind(row.title.clone());
            query.push(", ").push_bind(start + index as i64);
            query.push(", ").push_bind(actor.clone());
            query.push(", ").push_bind(actor.clone());
            query.push(", 0, ").push_bind(now.as_millis());
            query.push(", ").push_bind(now.as_millis());
            query
                .push(", '[]', '', ")
                .push_bind(row.search_text.clone());
            query.push(")");
        }
        query.build().execute(&mut **tx).await?;
    }
    let values: Vec<(usize, Id, CellValue)> = rows
        .rows
        .iter()
        .enumerate()
        .flat_map(|(index, row)| {
            row.values
                .iter()
                .filter_map(|(property, value)| {
                    let value = match value {
                        CellValue::People(people) => {
                            let kept: Vec<Id> = people
                                .iter()
                                .copied()
                                .filter(|id| members.contains(&id.to_string()))
                                .collect();
                            if kept.is_empty() {
                                return None;
                            }
                            CellValue::People(kept)
                        }
                        other => other.clone(),
                    };
                    Some((index, *property, value))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for chunk in values.chunks(VALUE_BATCH) {
        let mut query = SqlBuilder::new(
            "INSERT INTO page_property_values (page_id, property_id, workspace_id, value_json, updated_by, updated_at) VALUES ",
        );
        for (offset, (row, property, value)) in chunk.iter().enumerate() {
            if offset > 0 {
                query.push(", ");
            }
            query.push("(").push_bind(ids[*row].clone());
            query.push(", ").push_bind(property.to_string());
            query.push(", ").push_bind(workspace.clone());
            query.push(", ").push_bind(value.to_json().to_string());
            query.push(", ").push_bind(actor.clone());
            query.push(", ").push_bind(now.as_millis());
            query.push(")");
        }
        query.build().execute(&mut **tx).await?;
    }
    Ok(())
}

impl DatabaseRepository {
    /// Adds the CSV rows to the database (a merge adds rows, it never changes existing ones), with
    /// the new properties and options the mapping asks for. All or nothing. `dry_run` converts
    /// and counts without writing.
    #[allow(clippy::too_many_arguments)]
    pub async fn import_csv(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: CsvImport,
        dry_run: bool,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ImportResult, DatabaseError> {
        let adding = input.rows.len() as i64;
        let mut read = self.database.transaction().await?;
        open(&mut read, workspace_id, database_id, actor_id).await?;
        let snapshot = properties_in_tx(&mut read, database_id).await?;
        let members = members_in_tx(&mut read, workspace_id).await?;
        if dry_run {
            ensure_row_capacity(&mut read, database_id, adding).await?;
        }
        read.rollback().await?;
        let (planned, input, members) = plan_blocking(snapshot.clone(), members, input).await?;
        if dry_run {
            return Ok(planned.result);
        }

        let mut tx = self.database.immediate_transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let current = properties_in_tx(&mut tx, database_id).await?;
        // The schema changed since the snapshot: convert again against the current one.
        let mut plan = if same_schema(&snapshot, &current) {
            planned
        } else {
            plan(&current, &members, &input)?
        };
        if plan.changes_schema() {
            unlocked(&page)?;
        }
        ensure_row_capacity(&mut tx, database_id, adding).await?;
        ensure_trial_capacity(
            &self.database,
            &mut tx,
            workspace_id,
            TrialLimit::Pages,
            adding,
        )
        .await?;
        for property in &plan.properties {
            if plan.added.contains(&property.id) {
                insert_property(&mut tx, workspace_id, database_id, property, now).await?;
            } else if plan.grown.contains(&property.id) {
                sqlx::query(
                    "UPDATE database_properties SET config_json = $1, updated_at = $2 WHERE id = $3",
                )
                .bind(property.kind.config_json().to_string())
                .bind(now.as_millis())
                .bind(property.id.to_string())
                .execute(&mut *tx)
                .await?;
            }
        }
        if !plan.added.is_empty() {
            renumber_rows(
                &mut tx,
                "database_properties",
                &properties_ids(&plan.properties),
            )
            .await?;
            for mut view in raw_views(&mut tx, database_id, Views::All).await? {
                for property in &plan.properties {
                    if plan.added.contains(&property.id) {
                        view.config.columns.push(property.column());
                    }
                }
                write_view_config(&mut tx, view.id, &view.config, now).await?;
            }
        }
        insert_rows(
            &mut tx,
            workspace_id,
            database_id,
            page.space(actor_id),
            actor_id,
            &plan.rows,
            now,
        )
        .await?;
        let numbered = unique_id::number_rows(
            &mut tx,
            workspace_id,
            database_id,
            &mut plan.properties,
            actor_id,
            now,
        )
        .await?;
        // A new status property shows its default option on the rows that were already there.
        if plan.changes_schema() || !numbered.is_empty() {
            refresh_search_text(
                &mut tx,
                workspace_id,
                database_id,
                &plan.properties,
                SearchRows::All,
            )
            .await?;
        }
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.rows_imported",
            database_id,
            json!({
                "rows": plan.result.created_rows,
                "property_ids": plan.added.iter().map(Id::to_string).collect::<Vec<_>>(),
            }),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(plan.result)
    }

    /// A new database's schema from a CSV, for `PageRepository::create_database_with_id`: the
    /// mapped columns as properties (the title first) and one table view showing them, with the
    /// converted rows. One column must target `New(PropertyType::Title)`, the others `New(_)` or
    /// `Skip`.
    pub async fn csv_schema(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: CsvImport,
    ) -> Result<(DatabaseSchema, ImportResult), DatabaseError> {
        let mut tx = self.database.transaction().await?;
        require_access_tx(&mut tx, workspace_id, actor_id).await?;
        let members = members_in_tx(&mut tx, workspace_id).await?;
        tx.rollback().await?;
        let (plan, _, _) = plan_blocking(Vec::new(), members, input).await?;
        let views = vec![DatabaseView {
            id: Id::new_v7(),
            name: "Table".to_owned(),
            view_type: ViewType::Table,
            config: ViewConfig::for_properties(&plan.properties),
            position: 0,
        }];
        let schema = DatabaseSchema {
            properties: plan.properties,
            views,
            rows: Some(plan.rows),
        };
        Ok((schema, plan.result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, target: ImportTarget) -> ImportColumn {
        ImportColumn {
            name: name.to_owned(),
            target,
        }
    }

    fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
            .collect()
    }

    #[test]
    fn new_database_converts_each_type_and_counts() {
        let member = Member {
            id: Id::new_v7(),
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
        };
        let input = CsvImport {
            columns: vec![
                column("Amount", ImportTarget::New(PropertyType::Number)),
                column("Name", ImportTarget::New(PropertyType::Title)),
                column("Tags", ImportTarget::New(PropertyType::MultiSelect)),
                column("Done", ImportTarget::New(PropertyType::Checkbox)),
                column("Owner", ImportTarget::New(PropertyType::Person)),
                column("Notes", ImportTarget::Skip),
            ],
            rows: rows(&[
                &["12.5", "First", "a, b", "yes", "ADA@example.com", "x"],
                &["lots", "Second", "A", "no", "Nobody"],
            ]),
        };
        let plan = plan(&[], std::slice::from_ref(&member), &input).unwrap();
        let names: Vec<&str> = plan.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Name", "Amount", "Tags", "Done", "Owner"]);
        assert_eq!(plan.rows.rows[0].title, "First");
        let tags = &plan.properties[2];
        assert_eq!(
            tags.kind
                .options()
                .iter()
                .map(|(_, name)| *name)
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(
            plan.rows.rows[1].values,
            vec![(
                tags.id,
                CellValue::Choices(vec![tags.kind.options()[0].0.to_owned()])
            )]
        );
        assert_eq!(
            plan.rows.rows[0].values.last(),
            Some(&(plan.properties[4].id, CellValue::People(vec![member.id])))
        );
        assert_eq!(
            plan.result,
            ImportResult {
                created_rows: 2,
                converted: 7,
                cleared: 2,
                created_options: 2,
                values_truncated: 0,
                columns: vec![
                    ImportColumnResult {
                        converted: 1,
                        cleared: 1,
                        ..Default::default()
                    },
                    ImportColumnResult {
                        converted: 2,
                        ..Default::default()
                    },
                    ImportColumnResult {
                        converted: 2,
                        created_options: 2,
                        ..Default::default()
                    },
                    ImportColumnResult {
                        converted: 1,
                        ..Default::default()
                    },
                    ImportColumnResult {
                        converted: 1,
                        cleared: 1,
                        ..Default::default()
                    },
                    ImportColumnResult::default(),
                ],
            }
        );
    }

    #[test]
    fn needs_one_title_column_and_rows_that_fit() {
        let no_title = CsvImport {
            columns: vec![column("A", ImportTarget::New(PropertyType::Text))],
            rows: Vec::new(),
        };
        assert!(matches!(
            plan(&[], &[], &no_title),
            Err(DatabaseError::Invalid { field }) if field == "columns"
        ));
        let long_row = CsvImport {
            columns: vec![column("A", ImportTarget::New(PropertyType::Title))],
            rows: rows(&[&["ok"], &["one", "two"]]),
        };
        assert!(matches!(
            plan(&[], &[], &long_row),
            Err(DatabaseError::Invalid { field }) if field == "rows[1]"
        ));
    }

    #[test]
    fn cuts_long_titles_and_texts() {
        let input = CsvImport {
            columns: vec![
                column("Name", ImportTarget::New(PropertyType::Title)),
                column("Body", ImportTarget::New(PropertyType::Text)),
            ],
            rows: vec![vec!["t".repeat(600), "b".repeat(MAX_TEXT_CHARS + 1)]],
        };
        let plan = plan(&[], &[], &input).unwrap();
        assert_eq!(plan.rows.rows[0].title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(plan.result.values_truncated, 2);
        assert_eq!(
            plan.rows.rows[0].values,
            vec![(
                plan.properties[1].id,
                CellValue::Text("b".repeat(MAX_TEXT_CHARS))
            )]
        );
    }
}
