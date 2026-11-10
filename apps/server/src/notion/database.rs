//! A Notion data source as an Orbit database (pure): the schema with its views, and the values
//! of each row. The import fetches the data source, its views and rows first and writes the
//! result through `repositories::databases`, which checks every config and value again.
//!
//! Orbit ids are derived from the database page id and the Notion ids (property id, option id),
//! so a resumed import finds the properties and options it created before without storing a map.

use std::collections::{HashMap, HashSet};

use orbit_platform::Id;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::convert::{ConvertReport, page_icon, page_title};
use super::model::{NotionDataSource, NotionPage, NotionView, normalize_id, plain_text};
use crate::repositories::databases::{
    CalendarMode, CardPreview, CardSize, FilterGroup, FilterNode, FilterOperator, FilterRule,
    GroupOp, MAX_CELL_ITEMS, MAX_FILES, MAX_FILTER_LEVELS, MAX_GROUP_CHILDREN, MAX_NAME_CHARS, MAX_OPTIONS,
    MAX_PROPERTIES, MAX_RULES, MAX_SORTS, MAX_TEXT_CHARS, MAX_VIEWS, Property, PropertyType,
    SchemaProperty, SchemaView, SortDirection, TimelineZoom, ViewColumn, ViewConfig, ViewSort,
    ViewType, accepts_value,
};

/// Orbit members by lowercase email: `(user id, display name)`.
pub type Members = HashMap<String, (String, String)>;

/// Nesting followed in Notion filters and rollup values (untrusted input).
const MAX_DEPTH: usize = 8;

/// A data source as the import fetched it.
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub data_source: NotionDataSource,
    /// The views of this data source, in Notion's order; `None` when Notion did not return them.
    pub views: Option<Vec<NotionView>>,
    /// The rows in Notion order.
    pub rows: Vec<NotionPage>,
}

/// The schema of a new Orbit database, and what it could not carry over.
pub struct DatabasePlan {
    pub properties: Vec<SchemaProperty>,
    pub views: Vec<SchemaView>,
    pub report: ConvertReport,
}

/// A row's title, icon and values for `POST …/rows`, and what they could not carry over.
pub struct RowPlan {
    pub title: String,
    pub icon: Option<String>,
    pub values: Vec<(Id, Value)>,
    pub report: ConvertReport,
}

/// The Orbit property id for a Notion property of the database page `database`.
#[must_use]
pub fn property_id(database: Id, notion_property_id: &str) -> Id {
    let digest = Sha256::digest(format!("{database}/{}", decode(notion_property_id)));
    let hex = hex(&digest);
    let base = database.to_string();
    // The database id's time prefix, version 7 and the RFC 4122 variant, so the id parses as
    // a UUIDv7.
    format!(
        "{}-{}-7{}-{:x}{}-{}",
        &base[0..8],
        &base[9..13],
        &hex[0..3],
        8 | (digest[2] & 3),
        &hex[5..8],
        &hex[8..20]
    )
    .parse()
    .expect("a well-formed UUIDv7")
}

/// The Orbit option id for a Notion select, multi-select or status option.
#[must_use]
pub fn option_id(property: Id, notion_option_id: &str) -> String {
    let digest = Sha256::digest(format!("{property}/{notion_option_id}"));
    format!("n{}", &hex(&digest)[..24])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Percent-decodes a Notion property id (`%3B%60IZ` → ``;`IZ``); ids come encoded in some
/// responses and raw in others.
fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            let digit = |byte: u8| char::from(byte).to_digit(16).unwrap_or_default() as u8;
            out.push(digit(bytes[index + 1]) * 16 + digit(bytes[index + 2]));
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A select, multi-select or status option: its Notion id and its name.
struct PlannedOption {
    notion_id: String,
    name: String,
}

/// One Notion property and the Orbit property it becomes.
struct Planned {
    notion_id: String,
    notion_type: String,
    name: String,
    id: Id,
    property_type: PropertyType,
    config: Value,
    options: Vec<PlannedOption>,
    /// Status groups: `(group name, notion option ids)`.
    groups: Vec<(String, Vec<String>)>,
}

/// Plans the Orbit database for a data source: one property per Notion property type Orbit can
/// hold (static values for computed Notion types), and one table view per Notion view.
#[must_use]
pub fn plan_database(database: Id, source: &Source, members: &Members) -> DatabasePlan {
    let mut report = ConvertReport::default();
    let rows = &source.rows;
    let mut planned = Vec::new();
    for (name, schema) in &source.data_source.properties {
        let Some(notion_type) = schema.get("type").and_then(Value::as_str) else {
            continue;
        };
        let notion_id = schema
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_owned();
        let id = property_id(database, &notion_id);
        let config = schema.get(notion_type).unwrap_or(&Value::Null);
        let values = || {
            rows.iter()
                .filter_map(|row| row.properties.get(name))
                .filter_map(|value| value.get(notion_type))
        };
        let mut options = Vec::new();
        let mut groups = Vec::new();
        let (property_type, orbit_config) = match notion_type {
            "title" => (PropertyType::Title, json!({})),
            "rich_text" => (PropertyType::Text, json!({})),
            "email" => (PropertyType::Email, json!({})),
            "phone_number" => (PropertyType::Phone, json!({})),
            "unique_id" => (PropertyType::UniqueId, unique_id_config(config)),
            "number" => {
                let format = config.get("format").and_then(Value::as_str);
                (PropertyType::Number, number_config(format, &mut report))
            }
            "select" | "multi_select" | "status" => {
                let status = notion_type == "status";
                if status {
                    groups = status_groups(config);
                }
                let (list, json) = plan_options(id, config, &groups, status, &mut report);
                options = list;
                let property_type = match notion_type {
                    "select" => PropertyType::Select,
                    "multi_select" => PropertyType::MultiSelect,
                    _ => PropertyType::Status,
                };
                (property_type, json!({ "options": json }))
            }
            "date" | "created_time" | "last_edited_time" => (PropertyType::Date, json!({})),
            "people" => (PropertyType::Person, json!({})),
            "checkbox" => (PropertyType::Checkbox, json!({})),
            "url" => (PropertyType::Url, json!({})),
            // Uploaded files are downloaded with the row's body (`import.rs`).
            "files" => (PropertyType::Files, json!({})),
            "relation" => {
                lossy(&mut report, "relation_titles");
                (PropertyType::Text, json!({}))
            }
            "created_by" | "last_edited_by" => {
                if values().all(|user| user.is_null() || member_id(user, members).is_some()) {
                    (PropertyType::Person, json!({}))
                } else {
                    lossy(&mut report, "people_as_text");
                    (PropertyType::Text, json!({}))
                }
            }
            "formula" => {
                lossy(&mut report, "formula_values");
                let result = values().find_map(|value| {
                    let kind = value.get("type")?.as_str()?;
                    (!value.get(kind)?.is_null()).then_some(kind)
                });
                match result {
                    Some("number") => (PropertyType::Number, json!({"format": "number"})),
                    Some("boolean") => (PropertyType::Checkbox, json!({})),
                    Some("date") => (PropertyType::Date, json!({})),
                    _ => (PropertyType::Text, json!({})),
                }
            }
            "rollup" => {
                lossy(&mut report, "rollup_values");
                let result = values().find_map(|value| {
                    let kind = value.get("type")?.as_str()?;
                    (!value.get(kind)?.is_null()).then_some(kind)
                });
                match result {
                    Some("number") => (PropertyType::Number, json!({"format": "number"})),
                    Some("date") => (PropertyType::Date, json!({})),
                    _ => (PropertyType::Text, json!({})),
                }
            }
            other => {
                *report
                    .skipped
                    .entry(format!("property:{other}"))
                    .or_default() += 1;
                continue;
            }
        };
        planned.push(Planned {
            notion_id,
            notion_type: notion_type.to_owned(),
            name: clean_name(name, "Untitled"),
            id,
            property_type,
            config: orbit_config,
            options,
            groups,
        });
    }

    let views: Vec<&NotionView> = source
        .views
        .iter()
        .flatten()
        .filter(|view| {
            view.data_source_id.as_deref().and_then(normalize_id)
                == normalize_id(&source.data_source.id)
        })
        .collect();
    // The first view's column order is the property order; the title leads otherwise.
    let order: HashMap<Id, usize> = views
        .first()
        .and_then(|view| view_columns(view))
        .into_iter()
        .flatten()
        .filter_map(|(key, _)| lookup(&planned, key).map(|property| property.id))
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    planned.sort_by(|a, b| {
        let rank = |property: &Planned| {
            (
                order.get(&property.id).copied().unwrap_or(usize::MAX),
                property.property_type != PropertyType::Title,
            )
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    if planned.len() > MAX_PROPERTIES {
        let title = planned
            .iter()
            .position(|property| property.property_type == PropertyType::Title);
        if let Some(title) = title.filter(|title| *title >= MAX_PROPERTIES) {
            planned.swap(title, MAX_PROPERTIES - 1);
        }
        *report
            .lossy
            .entry("properties_truncated".to_owned())
            .or_default() += u32::try_from(planned.len() - MAX_PROPERTIES).unwrap_or(u32::MAX);
        planned.truncate(MAX_PROPERTIES);
    }

    let users = notion_users(rows, members);
    let mut schema_views: Vec<SchemaView> = views
        .iter()
        .take(MAX_VIEWS)
        .map(|view| plan_view(view, &planned, &users, &mut report))
        .collect();
    if source.views.is_none() {
        lossy(&mut report, "views_unavailable");
    }
    if schema_views.is_empty() {
        schema_views.push(SchemaView {
            name: "Table".to_owned(),
            view_type: ViewType::Table,
            config: ViewConfig::default(),
        });
    }
    DatabasePlan {
        properties: planned
            .into_iter()
            .map(|property| SchemaProperty {
                id: property.id,
                name: property.name,
                property_type: property.property_type,
                config: property.config,
            })
            .collect(),
        views: schema_views,
        report,
    }
}

/// A Notion unique ID's prefix, when Orbit can hold it (1 to 10 letters, digits or `_`).
fn unique_id_config(config: &Value) -> Value {
    let prefix = config
        .get("prefix")
        .and_then(Value::as_str)
        .filter(|prefix| {
            (1..=10).contains(&prefix.chars().count())
                && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    json!({ "prefix": prefix })
}

/// The Notion-hosted files of each `files` property of a row: `(Notion property id, [(url,
/// name)])`. External links are not files Orbit holds; they are counted as dropped.
#[must_use]
pub fn row_files(
    row: &NotionPage,
    report: &mut ConvertReport,
) -> Vec<(String, Vec<(String, Option<String>)>)> {
    let mut properties = Vec::new();
    for (name, value) in &row.properties {
        if value.get("type").and_then(Value::as_str) != Some("files") {
            continue;
        }
        let notion_id = value.get("id").and_then(Value::as_str).unwrap_or(name);
        let mut files = Vec::new();
        for file in value.get("files").and_then(Value::as_array).into_iter().flatten() {
            let file_name = file.get("name").and_then(Value::as_str).map(str::to_owned);
            match file.get("file").and_then(|file| file.get("url")).and_then(Value::as_str) {
                Some(url) if files.len() < MAX_FILES => files.push((url.to_owned(), file_name)),
                _ => lossy(report, "files_dropped"),
            }
        }
        properties.push((notion_id.to_owned(), files));
    }
    properties
}

fn lossy(report: &mut ConvertReport, kind: &str) {
    *report.lossy.entry(kind.to_owned()).or_default() += 1;
}

/// A name trimmed to Orbit's 200 characters; `fallback` when empty.
fn clean_name(name: &str, fallback: &str) -> String {
    let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim();
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name.to_owned()
    }
}

/// Notion number formats: currencies become `currency` with their ISO 4217 code; a format
/// Orbit has no equivalent for (Bitcoin) shows plain numbers.
fn number_config(format: Option<&str>, report: &mut ConvertReport) -> Value {
    let currency = match format.unwrap_or("number") {
        "number" => return json!({"format": "number"}),
        "number_with_commas" => return json!({"format": "number_with_commas"}),
        "percent" => return json!({"format": "percent"}),
        "dollar" => "USD",
        "australian_dollar" => "AUD",
        "canadian_dollar" => "CAD",
        "singapore_dollar" => "SGD",
        "euro" => "EUR",
        "pound" => "GBP",
        "yen" => "JPY",
        "ruble" => "RUB",
        "rupee" => "INR",
        "won" => "KRW",
        "yuan" => "CNY",
        "real" => "BRL",
        "lira" => "TRY",
        "rupiah" => "IDR",
        "franc" => "CHF",
        "hong_kong_dollar" => "HKD",
        "new_zealand_dollar" => "NZD",
        "krona" => "SEK",
        "norwegian_krone" => "NOK",
        "mexican_peso" => "MXN",
        "rand" => "ZAR",
        "new_taiwan_dollar" => "TWD",
        "danish_krone" => "DKK",
        "zloty" => "PLN",
        "baht" => "THB",
        "forint" => "HUF",
        "koruna" => "CZK",
        "shekel" => "ILS",
        "chilean_peso" => "CLP",
        "philippine_peso" => "PHP",
        "dirham" => "AED",
        "colombian_peso" => "COP",
        "riyal" => "SAR",
        "ringgit" => "MYR",
        "leu" => "RON",
        "argentine_peso" => "ARS",
        "uruguayan_peso" => "UYU",
        "peruvian_sol" => "PEN",
        "vietnamese_dong" => "VND",
        "pakistani_rupee" => "PKR",
        "nigerian_naira" => "NGN",
        _ => {
            lossy(report, "number_format");
            return json!({"format": "number"});
        }
    };
    json!({"format": "currency", "currency": currency})
}

/// Status groups in order: `(name, option ids)`.
fn status_groups(config: &Value) -> Vec<(String, Vec<String>)> {
    config
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|group| {
            let name = group
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let ids = group
                .get("option_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            (name, ids)
        })
        .collect()
}

/// Notion's three status groups by name (To-do, In progress, Complete), else by position.
fn orbit_group(groups: &[(String, Vec<String>)], option: &str) -> &'static str {
    let Some(index) = groups
        .iter()
        .position(|(_, ids)| ids.iter().any(|id| id == option))
    else {
        return "todo";
    };
    match groups[index].0.to_lowercase().as_str() {
        "to-do" | "to do" | "todo" => "todo",
        "in progress" => "in_progress",
        "complete" | "completed" | "done" => "done",
        _ => match index {
            0 => "todo",
            1 => "in_progress",
            _ => "done",
        },
    }
}

/// Options with derived ids, names and colors (Notion's ten colors are Orbit's); at most 200.
fn plan_options(
    property: Id,
    config: &Value,
    groups: &[(String, Vec<String>)],
    status: bool,
    report: &mut ConvertReport,
) -> (Vec<PlannedOption>, Vec<Value>) {
    let options: Vec<&Value> = config
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    if options.len() > MAX_OPTIONS {
        lossy(report, "options_truncated");
    }
    let mut seen = HashSet::new();
    let mut list = Vec::new();
    let mut out = Vec::new();
    for option in options.into_iter().take(MAX_OPTIONS) {
        let Some(notion_id) = option.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !seen.insert(notion_id) {
            continue;
        }
        let name = clean_name(
            option
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "Untitled",
        );
        let color = option
            .get("color")
            .and_then(Value::as_str)
            .filter(|color| {
                matches!(
                    *color,
                    "gray"
                        | "brown"
                        | "orange"
                        | "yellow"
                        | "green"
                        | "blue"
                        | "purple"
                        | "pink"
                        | "red"
                )
            })
            .unwrap_or("default");
        let group = status.then(|| orbit_group(groups, notion_id));
        let mut json = json!({"id": option_id(property, notion_id), "name": name, "color": color});
        if let Some(group) = group {
            json["group"] = json!(group);
        }
        list.push(PlannedOption {
            notion_id: notion_id.to_owned(),
            name,
        });
        out.push(json);
    }
    // Orbit needs a `todo` option (the value of a row without one).
    if status && !out.is_empty() && !out.iter().any(|option| option["group"] == "todo") {
        out[0]["group"] = json!("todo");
    }
    if status && out.is_empty() {
        let id = option_id(property, "orbit-default");
        out.push(json!({"id": id, "name": "Not started", "color": "default", "group": "todo"}));
    }
    (list, out)
}

/// The Orbit member of a Notion user object, matched by email (case-insensitive).
fn member_id(user: &Value, members: &Members) -> Option<String> {
    let email = user
        .get("person")?
        .get("email")?
        .as_str()?
        .trim()
        .to_lowercase();
    members.get(&email).map(|(id, _)| id.clone())
}

/// Notion user id → Orbit member id, for every person seen in the rows.
fn notion_users(rows: &[NotionPage], members: &Members) -> HashMap<String, String> {
    let mut users = HashMap::new();
    for value in rows.iter().flat_map(|row| row.properties.values()) {
        let list: Vec<&Value> = match value.get("type").and_then(Value::as_str) {
            Some("people") => value
                .get("people")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .collect(),
            Some(kind @ ("created_by" | "last_edited_by")) => value.get(kind).into_iter().collect(),
            _ => continue,
        };
        for user in list {
            if let (Some(id), Some(member)) = (
                user.get("id").and_then(Value::as_str),
                member_id(user, members),
            ) {
                users.insert(id.to_owned(), member);
            }
        }
    }
    users
}

/// A property by Notion id (encoded or not) or name.
fn lookup<'a>(planned: &'a [Planned], key: &str) -> Option<&'a Planned> {
    let decoded = decode(key);
    planned
        .iter()
        .find(|property| decode(&property.notion_id) == decoded)
        .or_else(|| planned.iter().find(|property| property.name == key.trim()))
}

/// `(property id or name, settings)` of a view's column list, when Notion has one.
fn view_columns(view: &NotionView) -> Option<Vec<(&str, &Value)>> {
    let list = view.configuration.as_ref()?.get("properties")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|entry| Some((entry.get("property_id")?.as_str()?, entry)))
            .collect(),
    )
}

/// One Notion view as an Orbit table, board, list or calendar view: name, columns, sorts and the
/// flat AND part of the filter; a board also keeps its group and lane properties (when they
/// became a type that can group) and "hide empty groups"; a board and a calendar keep their
/// visible card properties, and a calendar its date property (when it became a date) and range.
/// A gallery keeps its card preview and size, a timeline its date properties, zoom and table
/// pane. Other layouts (chart, form, …) become tables.
fn plan_view(
    view: &NotionView,
    planned: &[Planned],
    users: &HashMap<String, String>,
    report: &mut ConvertReport,
) -> SchemaView {
    let kind = view.kind.as_deref().unwrap_or("table");
    let view_type = match kind {
        "board" => ViewType::Board,
        "table" => ViewType::Table,
        "list" => ViewType::List,
        "calendar" => ViewType::Calendar,
        "gallery" => ViewType::Gallery,
        "timeline" => ViewType::Timeline,
        _ => {
            lossy(report, &format!("view_type:{kind}"));
            ViewType::Table
        }
    };
    let mut columns = Vec::new();
    let mut card_properties = Vec::new();
    if let Some(list) = view_columns(view) {
        let mut seen = HashSet::new();
        for (key, entry) in list {
            let Some(property) = lookup(planned, key) else {
                continue;
            };
            if seen.insert(property.id) {
                if matches!(
                    view_type,
                    ViewType::Board | ViewType::Calendar | ViewType::Gallery
                ) && property.property_type != PropertyType::Title
                    && entry.get("visible").and_then(Value::as_bool) != Some(false)
                {
                    card_properties.push(property.id);
                }
                columns.push(ViewColumn::new(
                    property.id,
                    property.property_type == PropertyType::Title,
                    entry.get("width").and_then(Value::as_u64),
                    entry.get("visible").and_then(Value::as_bool) == Some(false),
                ));
            }
        }
        for property in planned
            .iter()
            .filter(|property| !seen.contains(&property.id))
        {
            columns.push(ViewColumn::new(
                property.id,
                property.property_type == PropertyType::Title,
                None,
                false,
            ));
        }
    }

    let mut sorts = Vec::new();
    let mut sorts_lost = false;
    for sort in view.sorts.iter().flatten() {
        let property = sort
            .get("property")
            .and_then(Value::as_str)
            .and_then(|key| lookup(planned, key));
        match property {
            Some(property)
                if !sorts
                    .iter()
                    .any(|sort: &ViewSort| sort.property_id == property.id) =>
            {
                sorts.push(ViewSort {
                    property_id: property.id,
                    direction: if sort.get("direction").and_then(Value::as_str)
                        == Some("descending")
                    {
                        SortDirection::Desc
                    } else {
                        SortDirection::Asc
                    },
                });
            }
            Some(_) => {}
            None => sorts_lost = true,
        }
    }
    if sorts.len() > MAX_SORTS {
        sorts.truncate(MAX_SORTS);
        sorts_lost = true;
    }
    if sorts_lost {
        lossy(report, "view_sort");
    }

    let mut filters = FilterMapping {
        planned,
        users,
        rules: 0,
        complete: true,
    };
    let mut root = FilterGroup::default();
    match view.filter.as_ref().map(|filter| filters.node(filter, 1)) {
        None | Some(Ok(None)) => {}
        Some(Ok(Some(FilterNode::Group(group)))) => root = group,
        Some(Ok(Some(rule @ FilterNode::Rule(_)))) => root.children.push(rule),
        Some(Err(TooDeep)) => filters.complete = false,
    }
    let quick: Vec<FilterNode> = view
        .quick_filters
        .iter()
        .flatten()
        .filter_map(|(key, condition)| {
            let mut leaf = condition.as_object()?.clone();
            if leaf.is_empty() {
                return None;
            }
            leaf.insert("property".to_owned(), json!(key));
            filters.leaf(&Value::Object(leaf))
        })
        .collect();
    if !quick.is_empty() {
        // Quick filters must all match: an `or` root goes one level down under a new `and` root.
        if root.op == GroupOp::Or && !root.children.is_empty() {
            if levels(&root) < MAX_FILTER_LEVELS {
                root = FilterGroup {
                    op: GroupOp::And,
                    children: vec![FilterNode::Group(root)],
                };
            } else {
                filters.complete = false;
            }
        }
        if root.op == GroupOp::And || root.children.is_empty() {
            root.op = GroupOp::And;
            root.children.extend(quick);
        }
    }
    if root.children.len() > MAX_GROUP_CHILDREN {
        root.children.truncate(MAX_GROUP_CHILDREN);
        filters.complete = false;
    }
    if !filters.complete {
        lossy(report, "view_filter");
    }
    let grouping = |field: &str| {
        (view_type == ViewType::Board)
            .then(|| view.configuration.as_ref()?.get(field))
            .flatten()
            .filter(|group| !group.is_null())
    };
    let group_property = |group: &Value, lanes: bool| {
        group
            .get("property_id")
            .or(Some(group))
            .and_then(Value::as_str)
            .and_then(|key| lookup(planned, key))
            .filter(|property| {
                matches!(
                    property.property_type,
                    PropertyType::Select
                        | PropertyType::Status
                        | PropertyType::MultiSelect
                        | PropertyType::Person
                ) || (lanes && property.property_type == PropertyType::Checkbox)
            })
            .map(|property| property.id)
    };
    let group_by = grouping("group_by").and_then(|group| group_property(group, false));
    let sub_group_by = grouping("sub_group_by")
        .and_then(|group| group_property(group, true))
        .filter(|id| Some(*id) != group_by);
    let hide_empty_groups = grouping("group_by")
        .and_then(|group| group.get("hide_empty_groups"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let calendar = |field: &str| {
        (view_type == ViewType::Calendar)
            .then(|| view.configuration.as_ref()?.get(field))
            .flatten()
    };
    let date_property_id = calendar("date_property_id")
        .and_then(Value::as_str)
        .and_then(|key| lookup(planned, key))
        .filter(|property| {
            matches!(
                property.property_type,
                PropertyType::Date | PropertyType::CreatedTime | PropertyType::LastEditedTime
            )
        })
        .map(|property| property.id);
    let calendar_mode = match calendar("view_range").and_then(Value::as_str) {
        Some("week") => CalendarMode::Week,
        _ => CalendarMode::Month,
    };
    let layout = layout_config(view, view_type, planned);
    SchemaView {
        name: clean_name(view.name.as_deref().unwrap_or_default(), "Table"),
        view_type,
        config: ViewConfig {
            card_preview: layout.card_preview,
            card_size: layout.card_size,
            fit_image: layout.fit_image,
            start_property_id: layout.start_property_id,
            end_property_id: layout.end_property_id,
            zoom: layout.zoom,
            show_table: layout.show_table,
            table_columns: layout.table_columns,
            group_by,
            sub_group_by,
            hide_empty_groups,
            card_properties,
            date_property_id,
            calendar_mode,
            columns,
            sorts,
            filter: root,
            wrap_cells: view
                .configuration
                .as_ref()
                .and_then(|config| config.get("wrap_cells"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ..ViewConfig::default()
        },
    }
}

/// The gallery and timeline fields of an imported view; defaults for other layouts.
fn layout_config(view: &NotionView, view_type: ViewType, planned: &[Planned]) -> ViewConfig {
    let field = |name: &str| view.configuration.as_ref()?.get(name);
    let property = |key: Option<&Value>| {
        key.and_then(Value::as_str)
            .and_then(|key| lookup(planned, key))
    };
    let date = |key: Option<&Value>| {
        property(key)
            .filter(|property| property.property_type == PropertyType::Date)
            .map(|property| property.id)
    };
    match view_type {
        ViewType::Gallery => ViewConfig {
            card_preview: match field("cover") {
                None => CardPreview::Content,
                Some(Value::Null) => CardPreview::None,
                Some(cover) => match cover.get("type").and_then(Value::as_str) {
                    Some("page_cover") => CardPreview::Cover,
                    Some("property") => property(cover.get("property_id"))
                        .filter(|property| property.property_type == PropertyType::Files)
                        .map_or(CardPreview::Content, |property| {
                            CardPreview::Files(property.id.to_string())
                        }),
                    _ => CardPreview::Content,
                },
            },
            card_size: match field("cover_size").and_then(Value::as_str) {
                Some("small") => CardSize::Small,
                Some("large") => CardSize::Large,
                _ => CardSize::Medium,
            },
            fit_image: field("cover_aspect").and_then(Value::as_str) == Some("contain"),
            ..ViewConfig::default()
        },
        ViewType::Timeline => ViewConfig {
            start_property_id: date(field("date_property_id")),
            end_property_id: date(field("end_date_property_id")),
            zoom: match field("preference")
                .and_then(|preference| preference.get("zoom_level"))
                .and_then(Value::as_str)
            {
                Some("hours" | "day") => TimelineZoom::Day,
                Some("week" | "bi_week") => TimelineZoom::Week,
                Some("quarter") => TimelineZoom::Quarter,
                Some("year" | "5_years") => TimelineZoom::Year,
                _ => TimelineZoom::Month,
            },
            show_table: (field("show_table").and_then(Value::as_bool) == Some(false))
                .then_some(false),
            table_columns: field("table_properties")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|entry| entry.get("visible").and_then(Value::as_bool) != Some(false))
                .filter_map(|entry| property(entry.get("property_id")))
                .filter(|property| property.property_type != PropertyType::Title)
                .map(|property| property.id)
                .collect(),
            ..ViewConfig::default()
        },
        _ => ViewConfig::default(),
    }
}

/// A Notion filter nests deeper than Orbit's 3 group levels.
struct TooDeep;

/// Maps Notion filters to Orbit filter groups and notes whether a part was dropped.
struct FilterMapping<'a> {
    planned: &'a [Planned],
    users: &'a HashMap<String, String>,
    /// Rules kept so far; past `MAX_RULES` the rest go.
    rules: usize,
    complete: bool,
}

impl FilterMapping<'_> {
    /// A Notion compound filter as a group at `level` (the root is 1), or a condition as a rule.
    /// A condition without an Orbit operator goes, a group left empty too, and a group of more
    /// than 10 keeps its first 10; a group deeper than 3 levels drops the whole filter.
    fn node(&mut self, filter: &Value, level: usize) -> Result<Option<FilterNode>, TooDeep> {
        let compound = [("and", GroupOp::And), ("or", GroupOp::Or)]
            .into_iter()
            .find_map(|(key, op)| Some((op, filter.get(key)?.as_array()?)));
        let Some((op, items)) = compound else {
            return Ok(self.leaf(filter));
        };
        if level > MAX_FILTER_LEVELS {
            return Err(TooDeep);
        }
        let mut children = Vec::new();
        for item in items {
            children.extend(self.node(item, level + 1)?);
        }
        if children.len() > MAX_GROUP_CHILDREN {
            children.truncate(MAX_GROUP_CHILDREN);
            self.complete = false;
        }
        Ok((!children.is_empty()).then_some(FilterNode::Group(FilterGroup { op, children })))
    }

    fn leaf(&mut self, leaf: &Value) -> Option<FilterNode> {
        let rule = (self.rules < MAX_RULES)
            .then(|| filter_rule(leaf, self.planned, self.users))
            .flatten();
        match rule {
            Some(rule) => {
                self.rules += 1;
                Some(FilterNode::Rule(rule))
            }
            None => {
                self.complete = false;
                None
            }
        }
    }
}

/// The group levels of a filter, the root included.
fn levels(group: &FilterGroup) -> usize {
    1 + group
        .children
        .iter()
        .map(|child| match child {
            FilterNode::Group(group) => levels(group),
            FilterNode::Rule(_) => 0,
        })
        .max()
        .unwrap_or(0)
}

/// A Notion property filter condition as an Orbit rule, when Orbit has the operator.
fn filter_rule(
    leaf: &Value,
    planned: &[Planned],
    users: &HashMap<String, String>,
) -> Option<FilterRule> {
    use FilterOperator as Op;
    let property = lookup(planned, leaf.get("property")?.as_str()?)?;
    let (mut key, mut condition) = leaf
        .as_object()?
        .iter()
        .find(|(key, _)| !matches!(key.as_str(), "property" | "type"))?;
    if matches!(key.as_str(), "formula" | "rollup") {
        (key, condition) = condition.as_object()?.iter().next()?;
        if matches!(key.as_str(), "any" | "every" | "none") {
            return None;
        }
    }
    let (operator, value) = condition.as_object()?.iter().next()?;
    let rule = |operator, value| {
        Some(FilterRule {
            property_id: property.id,
            operator,
            value,
        })
    };
    match operator.as_str() {
        "is_empty" => return rule(Op::IsEmpty, Value::Null),
        "is_not_empty" => return rule(Op::IsNotEmpty, Value::Null),
        _ => {}
    }
    let text_source = matches!(
        property.notion_type.as_str(),
        "title" | "rich_text" | "email" | "phone_number" | "url" | "formula"
    );
    match property.property_type {
        PropertyType::Title
        | PropertyType::Text
        | PropertyType::Url
        | PropertyType::Email
        | PropertyType::Phone
            if text_source =>
        {
            let operator = match operator.as_str() {
                "equals" => Op::Equals,
                "does_not_equal" => Op::NotEquals,
                "contains" => Op::Contains,
                "does_not_contain" => Op::NotContains,
                "starts_with" => Op::StartsWith,
                "ends_with" => Op::EndsWith,
                _ => return None,
            };
            rule(operator, json!(value.as_str()?))
        }
        PropertyType::Number | PropertyType::UniqueId => {
            let operator = match operator.as_str() {
                "equals" => Op::Eq,
                "does_not_equal" => Op::Ne,
                "greater_than" => Op::Gt,
                "less_than" => Op::Lt,
                "greater_than_or_equal_to" => Op::Ge,
                "less_than_or_equal_to" => Op::Le,
                _ => return None,
            };
            rule(operator, json!(value.as_f64()?))
        }
        PropertyType::Select | PropertyType::Status => {
            let (operator, groups) = match operator.as_str() {
                "equals" => (Op::Is, false),
                "does_not_equal" => (Op::IsNot, false),
                "group_equals" => (Op::Is, true),
                "group_does_not_equal" => (Op::IsNot, true),
                _ => return None,
            };
            rule(operator, option_ids(property, value, groups)?)
        }
        PropertyType::MultiSelect => {
            let operator = match operator.as_str() {
                "contains" => Op::Contains,
                "does_not_contain" => Op::NotContains,
                _ => return None,
            };
            rule(operator, option_ids(property, value, false)?)
        }
        PropertyType::Person if property.notion_type == "people" => {
            let operator = match operator.as_str() {
                "contains" => Op::Contains,
                "does_not_contain" => Op::NotContains,
                _ => return None,
            };
            let user = match value.as_str()? {
                "me" => "me".to_owned(),
                id => users.get(id)?.clone(),
            };
            rule(operator, json!([user]))
        }
        PropertyType::Checkbox => {
            let checked = value.as_bool()?;
            match operator.as_str() {
                "equals" => rule(Op::Is, json!(checked)),
                "does_not_equal" => rule(Op::Is, json!(!checked)),
                _ => None,
            }
        }
        PropertyType::Date => {
            let operator = match operator.as_str() {
                "equals" => Op::Is,
                "before" => Op::Before,
                "after" => Op::After,
                "on_or_before" => Op::OnOrBefore,
                "on_or_after" => Op::OnOrAfter,
                _ => return None,
            };
            let raw = value.as_str()?;
            let date = match raw {
                "today" | "tomorrow" | "yesterday" | "one_week_ago" | "one_week_from_now"
                | "one_month_ago" | "one_month_from_now" => raw.to_owned(),
                _ => date_point(raw)?,
            };
            rule(operator, json!(date))
        }
        _ => None,
    }
}

/// Option names (one or a list) as Orbit option ids; a status group name stands for its
/// options. `None` when a name matches nothing.
fn option_ids(property: &Planned, value: &Value, groups: bool) -> Option<Value> {
    let names: Vec<&str> = match value {
        Value::String(name) => vec![name.as_str()],
        Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
        _ => return None,
    };
    let mut ids = Vec::new();
    for name in names {
        let options: Vec<&str> = property
            .options
            .iter()
            .filter(|option| !groups && option.name == name.trim())
            .map(|option| option.notion_id.as_str())
            .collect();
        let options = if options.is_empty() {
            property
                .groups
                .iter()
                .filter(|(group, _)| group == name)
                .flat_map(|(_, ids)| ids.iter().map(String::as_str))
                .collect()
        } else {
            options
        };
        if options.is_empty() {
            return None;
        }
        ids.extend(options.into_iter().map(|id| option_id(property.id, id)));
    }
    Some(json!(ids))
}

/// The values of one row for the database's `properties` (as stored): every Notion value whose
/// property was imported, converted to the property's Orbit type. Values the property does not
/// accept are left out and counted.
#[must_use]
pub fn plan_row(
    database: Id,
    row: &NotionPage,
    properties: &[Property],
    members: &Members,
    titles: &HashMap<String, String>,
) -> RowPlan {
    let mut report = ConvertReport::default();
    let by_id: HashMap<Id, &Property> = properties
        .iter()
        .map(|property| (property.id, property))
        .collect();
    let mut values = Vec::new();
    for (name, value) in &row.properties {
        let notion_id = value.get("id").and_then(Value::as_str).unwrap_or(name);
        let Some(property) = by_id.get(&property_id(database, notion_id)) else {
            continue;
        };
        let Some(cell) = cell(value, property, members, titles, &mut report) else {
            continue;
        };
        if accepts_value(&property.kind, &cell) {
            values.push((property.id, cell));
        } else {
            lossy(&mut report, "invalid_values");
        }
    }
    RowPlan {
        title: page_title(row),
        icon: page_icon(row),
        values,
        report,
    }
}

/// A Notion property value as an Orbit value of `property`'s type; `None` when empty or of
/// another type (a formula whose result type changed between rows).
fn cell(
    value: &Value,
    property: &Property,
    members: &Members,
    titles: &HashMap<String, String>,
    report: &mut ConvertReport,
) -> Option<Value> {
    let kind = value.get("type")?.as_str()?;
    let data = value.get(kind)?;
    // A formula or rollup result: `{type, <type>: …}`.
    let result = |data: &Value| -> Option<(String, Value)> {
        let kind = data.get("type")?.as_str()?;
        Some((kind.to_owned(), data.get(kind)?.clone()))
    };
    let computed = match kind {
        "formula" | "rollup" => result(data),
        _ => None,
    };
    match property.kind.property_type() {
        PropertyType::Text => {
            let text = notion_text(value, titles, 0)?;
            if text.chars().count() > MAX_TEXT_CHARS {
                lossy(report, "values_truncated");
                return Some(json!(text.chars().take(MAX_TEXT_CHARS).collect::<String>()));
            }
            Some(json!(text))
        }
        PropertyType::Number => match (kind, &computed) {
            ("number", _) => data.as_f64().map(|number| json!(number)),
            (_, Some((result, number))) if result == "number" => {
                number.as_f64().map(|number| json!(number))
            }
            _ => None,
        },
        PropertyType::Select | PropertyType::Status => {
            let id = data.get("id")?.as_str()?;
            Some(json!(option_id(property.id, id)))
        }
        PropertyType::MultiSelect => {
            let ids: Vec<String> = data
                .as_array()?
                .iter()
                .filter_map(|option| option.get("id")?.as_str())
                .map(|id| option_id(property.id, id))
                .collect();
            Some(json!(limit(ids, report)))
        }
        PropertyType::Date => match (kind, &computed) {
            ("date", _) => date_value(data),
            ("created_time" | "last_edited_time", _) => {
                Some(json!({"start": date_point(data.as_str()?)?, "end": null}))
            }
            (_, Some((result, date))) if result == "date" => date_value(date),
            _ => None,
        },
        PropertyType::Person => {
            let users: Vec<&Value> = match kind {
                "people" => data.as_array()?.iter().collect(),
                "created_by" | "last_edited_by" => vec![data],
                _ => return None,
            };
            let mut ids = Vec::new();
            for user in users {
                match member_id(user, members) {
                    Some(id) if !ids.contains(&id) => ids.push(id),
                    Some(_) => {}
                    None => lossy(report, "unmatched_people"),
                }
            }
            Some(json!(limit(ids, report)))
        }
        PropertyType::Checkbox => match (kind, &computed) {
            ("checkbox", _) => data.as_bool().map(|checked| json!(checked)),
            (_, Some((result, checked))) if result == "boolean" => {
                checked.as_bool().map(|checked| json!(checked))
            }
            _ => None,
        },
        PropertyType::Url | PropertyType::Email | PropertyType::Phone => {
            data.as_str().map(|text| json!(text.trim()))
        }
        PropertyType::UniqueId => data.get("number")?.as_i64().map(|number| json!(number)),
        _ => None,
    }
}

fn limit(mut ids: Vec<String>, report: &mut ConvertReport) -> Vec<String> {
    if ids.len() > MAX_CELL_ITEMS {
        ids.truncate(MAX_CELL_ITEMS);
        lossy(report, "values_truncated");
    }
    ids
}

/// A Notion date (`{start, end, time_zone}`) as an Orbit date value.
fn date_value(data: &Value) -> Option<Value> {
    let start = date_point(data.get("start")?.as_str()?)?;
    let end = data.get("end").and_then(Value::as_str).and_then(date_point);
    Some(json!({"start": start, "end": end}))
}

/// `2026-11-09` stays; a time with an offset (`2026-11-09T14:00:00.000+01:00`) becomes UTC
/// (`2026-11-09T13:00:00Z`). A time without an offset counts as UTC.
fn date_point(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let Some((day, time)) = raw.split_once('T') else {
        return (raw.len() == 10).then(|| raw.to_owned());
    };
    let (clock, offset_minutes) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0)
    } else if let Some(at) = time.rfind(['+', '-']) {
        let (clock, offset) = time.split_at(at);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = offset[1..].split_once(':').unwrap_or((&offset[1..], "0"));
        (
            clock,
            sign * (hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?),
        )
    } else {
        (time, 0)
    };
    let clock = clock.split('.').next()?;
    let mut parts = clock.split(':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    let second: i64 = parts.next().map_or(Some(0), |part| part.parse().ok())?;
    let mut fields = day.split('-');
    let year: i64 = fields.next()?.parse().ok()?;
    let month: i64 = fields.next()?.parse().ok()?;
    let date: i64 = fields.next()?.parse().ok()?;
    let seconds = days_from_civil(year, month, date) * 86_400 + hour * 3_600 + minute * 60 + second
        - offset_minutes * 60;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, date) = civil_from_days(days);
    Some(format!(
        "{year:04}-{month:02}-{date:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    ))
}

/// Days since 1970-01-01 (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A Notion property value as display text: names for options and people, `PREFIX-N` for ids,
/// related page titles, formula and rollup results. `None` when empty.
fn notion_text(value: &Value, titles: &HashMap<String, String>, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    let kind = value.get("type")?.as_str()?;
    let data = value.get(kind)?;
    let join = |items: Vec<String>| {
        items
            .into_iter()
            .filter(|item| !item.trim().is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let user_name = |user: &Value| {
        user.get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let text = match kind {
        "title" | "rich_text" => plain_text(data.as_array()?),
        "number" => number_text(data)?,
        "select" | "status" => data.get("name")?.as_str()?.to_owned(),
        "multi_select" => join(
            data.as_array()?
                .iter()
                .filter_map(|option| Some(option.get("name")?.as_str()?.to_owned()))
                .collect(),
        ),
        "date" => date_text(data)?,
        "people" => join(data.as_array()?.iter().map(user_name).collect()),
        "created_by" | "last_edited_by" => user_name(data),
        "checkbox" => if data.as_bool()? { "Yes" } else { "No" }.to_owned(),
        "url" | "email" | "phone_number" | "created_time" | "last_edited_time" | "string" => {
            data.as_str()?.to_owned()
        }
        "boolean" => if data.as_bool()? { "Yes" } else { "No" }.to_owned(),
        "files" => join(
            data.as_array()?
                .iter()
                .filter_map(|file| {
                    file.get("name")
                        .and_then(Value::as_str)
                        .or_else(|| file.get("external")?.get("url")?.as_str())
                        .map(str::to_owned)
                })
                .collect(),
        ),
        "unique_id" => {
            let number = number_text(data.get("number")?)?;
            match data.get("prefix").and_then(Value::as_str) {
                Some(prefix) if !prefix.is_empty() => format!("{prefix}-{number}"),
                _ => number,
            }
        }
        "relation" => join(
            data.as_array()?
                .iter()
                .filter_map(|related| normalize_id(related.get("id")?.as_str()?))
                .map(|id| {
                    titles
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| "Untitled".to_owned())
                })
                .collect(),
        ),
        "formula" | "rollup" => {
            let result = data.get("type")?.as_str()?;
            match result {
                "array" => join(
                    data.get("array")?
                        .as_array()?
                        .iter()
                        .filter_map(|item| notion_text(item, titles, depth + 1))
                        .collect(),
                ),
                _ => notion_text(data, titles, depth + 1)?,
            }
        }
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}

fn number_text(number: &Value) -> Option<String> {
    if let Some(value) = number.as_i64() {
        return Some(value.to_string());
    }
    let value = number.as_f64()?;
    Some(if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    })
}

fn date_text(date: &Value) -> Option<String> {
    let start = date.get("start")?.as_str()?;
    Some(match date.get("end").and_then(Value::as_str) {
        Some(end) => format!("{start} → {end}"),
        None => start.to_owned(),
    })
}

/// `(name, property id, type)` of the row's values that the page object may cut: relations with
/// `has_more` and people lists of 25 or more (the page object holds at most 25). Read them in
/// full from the property-item endpoint and [`set_paginated`].
#[must_use]
pub fn paginated_properties(row: &NotionPage) -> Vec<(String, String, String)> {
    row.properties
        .iter()
        .filter_map(|(name, value)| {
            let kind = value.get("type")?.as_str()?;
            let more = match kind {
                "relation" => value.get("has_more").and_then(Value::as_bool) == Some(true),
                "people" => value.get("people")?.as_array()?.len() >= 25,
                _ => false,
            };
            let id = value.get("id")?.as_str()?;
            more.then(|| (name.clone(), id.to_owned(), kind.to_owned()))
        })
        .collect()
}

/// Replaces a paginated property's value with the full list from the property-item endpoint.
pub fn set_paginated(row: &mut NotionPage, name: &str, kind: &str, items: &[Value]) {
    let list: Vec<Value> = items
        .iter()
        .filter_map(|item| item.get(kind).cloned())
        .collect();
    if let Some(Value::Object(value)) = row.properties.get_mut(name) {
        value.insert(kind.to_owned(), Value::Array(list));
        value.insert("has_more".to_owned(), json!(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_ids_are_stable_v7_ids() {
        let database: Id = "01900000-0000-7000-8000-000000000001".parse().unwrap();
        let id = property_id(database, "%3B%60IZ");
        assert_eq!(id, property_id(database, ";`IZ"), "encoded or not");
        assert_ne!(id, property_id(database, "title"));
        assert!(id.to_string().starts_with("01900000-0000-7"));
        let option = option_id(id, "abc");
        assert_eq!(option.len(), 25);
        assert!(option.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    }

    #[test]
    fn dates_become_utc() {
        assert_eq!(date_point("2026-11-09").as_deref(), Some("2026-11-09"));
        assert_eq!(
            date_point("2026-11-09T00:30:00.000+01:00").as_deref(),
            Some("2026-11-08T23:30:00Z")
        );
        assert_eq!(
            date_point("2026-12-31T23:00:00-02:00").as_deref(),
            Some("2027-01-01T01:00:00Z")
        );
        assert_eq!(
            date_point("2026-11-06T20:17:00.000Z").as_deref(),
            Some("2026-11-06T20:17:00Z")
        );
        assert_eq!(date_point("soon"), None);
    }

    #[test]
    fn filters_keep_and_or_groups_up_to_three_levels() {
        let planned = [Planned {
            notion_id: "a".to_owned(),
            notion_type: "checkbox".to_owned(),
            name: "A".to_owned(),
            id: Id::new_v7(),
            property_type: PropertyType::Checkbox,
            config: Value::Null,
            options: Vec::new(),
            groups: Vec::new(),
        }];
        let users = HashMap::new();
        let mut mapping = FilterMapping {
            planned: &planned,
            users: &users,
            rules: 0,
            complete: true,
        };
        let leaf = json!({"property": "A", "checkbox": {"equals": true}});
        let unknown = json!({"property": "B", "checkbox": {"equals": true}});
        let Ok(Some(FilterNode::Group(group))) = mapping.node(
            &json!({"or": [leaf, {"and": [leaf, unknown]}, {"and": [unknown]}]}),
            1,
        ) else {
            panic!("an or group maps to a group");
        };
        assert_eq!(group.op, GroupOp::Or);
        assert_eq!(group.children.len(), 2, "a group left empty goes");
        assert!(
            matches!(&group.children[1], FilterNode::Group(inner) if inner.children.len() == 1)
        );
        assert!(!mapping.complete, "the unknown property is reported");

        let mut deep = leaf.clone();
        for _ in 0..4 {
            deep = json!({"and": [deep]});
        }
        assert!(
            mapping.node(&deep, 1).is_err(),
            "4 levels are dropped whole"
        );
    }

    #[test]
    fn notion_ids_decode() {
        assert_eq!(decode("%3B%60IZ"), ";`IZ");
        assert_eq!(decode("title"), "title");
        assert_eq!(decode("100%"), "100%");
    }
}
