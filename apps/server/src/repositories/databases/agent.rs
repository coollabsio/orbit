//! Databases for API tokens and MCP tools (spec P3 14.3): keys are property names or ids, options
//! go by name, people by user id or email, and queries filter and sort on the server
//! ([`super::query`]). Results name options and show people as `{id, name}`.

use std::collections::{BTreeMap, HashMap, HashSet};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use orbit_platform::{Id, Row, TimestampMillis};
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use super::formula::FormulaType;
use super::query::{QueryContext, query_rows};
use super::{
    CellValue, ComputeScope, DatabaseError, DatabaseRepository, DatabaseRow, DateValue,
    FilterGroup, FilterNode, FilterOperator, FilterRule, GroupOp, MAX_SORTS, Property,
    PropertyKind, PropertyType, RelatedPage, SortDirection, StatusGroup, ViewSort, ViewType,
    computed, open, properties_in_tx, rows_in_tx, templates, unique_id,
};

/// Rows per query page.
const MAX_QUERY_ROWS: usize = 100;
const DEFAULT_QUERY_ROWS: usize = 50;
/// Values per row write.
const MAX_AGENT_VALUES: usize = 50;
/// Rows per trash request.
const MAX_AGENT_TRASH: usize = 100;

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabasePerson)]
pub struct AgentPerson {
    #[schema(value_type = String)]
    pub id: Id,
    /// Empty for someone who left the workspace.
    pub name: String,
}

/// A related page the reader can read.
#[derive(Serialize)]
struct AgentPage {
    id: Id,
    title: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabaseOption)]
pub struct AgentOption {
    pub id: String,
    pub name: String,
    /// Status options: `todo`, `in_progress` or `done`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabaseProperty)]
pub struct AgentProperty {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub property_type: PropertyType,
    /// The options of a `select`, `multi_select` or `status` property, in order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<AgentOption>>,
    /// The rest of the config, such as a number's `format` (a `percent` stores 0.5 for 50%).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub config: Option<Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabaseView)]
pub struct AgentView {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub view_type: ViewType,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabaseTemplate)]
pub struct AgentTemplate {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
}

/// A database's schema: what a query and a row write can name.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabase)]
pub struct AgentDatabase {
    #[schema(value_type = String)]
    pub id: Id,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    pub url: String,
    /// Properties and views are read-only; rows and values stay writable.
    pub locked: bool,
    pub row_count: u64,
    pub properties: Vec<AgentProperty>,
    pub views: Vec<AgentView>,
    pub templates: Vec<AgentTemplate>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabaseRow)]
pub struct AgentRow {
    #[schema(value_type = String)]
    pub id: Id,
    pub title: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    pub url: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    pub created_by: AgentPerson,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
    pub updated_by: AgentPerson,
    /// Every property but the title and the created and edited ones, by property name (by id
    /// when two properties share the name); `null` is empty. Options show by name, people (also
    /// of a formula) as `{id, name}`, related pages as `{id, title}`, dates as `{start, end}`,
    /// unique IDs as `"PREFIX-12"`.
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, Value>,
    /// Per relation key of `values`: links to pages the reader cannot read, which `values`
    /// leaves out. Absent: none.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub hidden_links: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabaseRows)]
pub struct AgentRowPage {
    pub items: Vec<AgentRow>,
    /// Send it as `cursor` with the same query for the next page; `null` on the last page.
    #[schema(required = true)]
    pub next_cursor: Option<String>,
    /// Rows that match the filter.
    pub total: u64,
}

#[derive(Debug, Default)]
pub struct AgentQuery {
    /// The view filter shape; `property_id` takes a property name or id, `value` option names.
    pub filter: Option<Value>,
    /// `[{property_id, direction}]`, with names or ids.
    pub sorts: Option<Value>,
    /// Starts from this view's filter and sorts: `filter` must match too, `sorts` replace its.
    pub view_id: Option<Id>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    /// IANA zone for relative dates and computed values; default `UTC`.
    pub tz: Option<String>,
}

struct Member {
    id: Id,
    name: String,
    email: String,
}

/// The schema and the members that turn names into ids, and ids into names.
struct Names {
    properties: Vec<Property>,
    members: Vec<Member>,
    actor_id: Id,
}

/// The property `key` names: its id, else its exact name, else its name in any letter case.
/// A name that two properties share is an error.
pub(super) fn property_by_key<'a>(
    properties: &'a [Property],
    key: &str,
    path: &str,
) -> Result<&'a Property, DatabaseError> {
    if let Ok(id) = key.parse::<Id>()
        && let Some(property) = properties.iter().find(|property| property.id == id)
    {
        return Ok(property);
    }
    let exact: Vec<&Property> = properties
        .iter()
        .filter(|property| property.name == key)
        .collect();
    let found = match exact.as_slice() {
        [] => {
            let lower = key.to_lowercase();
            let loose: Vec<&Property> = properties
                .iter()
                .filter(|property| property.name.to_lowercase() == lower)
                .collect();
            match loose.as_slice() {
                [property] => Some(*property),
                _ => None,
            }
        }
        [property] => Some(*property),
        _ => None,
    };
    found.ok_or_else(|| DatabaseError::invalid(path))
}

impl Names {
    fn property(&self, key: &str, path: &str) -> Result<&Property, DatabaseError> {
        property_by_key(&self.properties, key, path)
    }

    fn option_id(kind: &PropertyKind, text: &str) -> Option<String> {
        let options = kind.options();
        let lower = text.to_lowercase();
        options
            .iter()
            .find(|(id, _)| *id == text)
            .or_else(|| options.iter().find(|(_, name)| *name == text))
            .or_else(|| {
                options
                    .iter()
                    .find(|(_, name)| name.to_lowercase() == lower)
            })
            .map(|(id, _)| (*id).to_owned())
    }

    /// A user id, an email of a member, or `"me"` (kept as `"me"` when `keep_me`).
    fn person_id(&self, text: &str, keep_me: bool) -> Option<String> {
        if text == "me" {
            return Some(if keep_me {
                "me".to_owned()
            } else {
                self.actor_id.to_string()
            });
        }
        if let Ok(id) = text.parse::<Id>() {
            return Some(id.to_string());
        }
        let lower = text.to_lowercase();
        self.members
            .iter()
            .find(|member| member.email.to_lowercase() == lower)
            .map(|member| member.id.to_string())
    }

    /// Option or people references (one or a list) as ids; `None` when one is unknown.
    fn ids(&self, kind: &PropertyKind, value: &Value, keep_me: bool) -> Option<Value> {
        let items = match value {
            Value::Array(items) => items.as_slice(),
            Value::String(_) => std::slice::from_ref(value),
            other => return Some(other.clone()),
        };
        let people = matches!(
            kind,
            PropertyKind::Person | PropertyKind::CreatedBy | PropertyKind::LastEditedBy
        );
        items
            .iter()
            .map(|item| {
                let text = item.as_str()?;
                if people {
                    self.person_id(text, keep_me)
                } else {
                    Self::option_id(kind, text)
                }
                .map(Value::String)
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array)
    }

    /// Values as agents send them, as the internal row API takes them.
    fn input_values(&self, raw: &Map<String, Value>) -> Result<Vec<(Id, Value)>, DatabaseError> {
        if raw.len() > MAX_AGENT_VALUES {
            return Err(DatabaseError::invalid("values"));
        }
        raw.iter()
            .map(|(key, value)| {
                let path = format!("values.{key}");
                let property = self.property(key, &path)?;
                let invalid = || DatabaseError::invalid(path.clone());
                if page_field(&property.kind) {
                    return Err(invalid());
                }
                let value = match (&property.kind, value) {
                    (PropertyKind::Select(_) | PropertyKind::Status(_), Value::String(text)) => {
                        Value::String(Self::option_id(&property.kind, text).ok_or_else(invalid)?)
                    }
                    (PropertyKind::MultiSelect(_) | PropertyKind::Person, _) => {
                        self.ids(&property.kind, value, false).ok_or_else(invalid)?
                    }
                    (PropertyKind::Date(_), Value::String(text)) => DateValue::from_text(text)
                        .map(|date| CellValue::Date(date).to_json())
                        .ok_or_else(invalid)?,
                    (_, value) => value.clone(),
                };
                Ok((property.id, value))
            })
            .collect()
    }

    fn filter(&self, raw: &Value) -> Result<FilterGroup, DatabaseError> {
        let group = match self.filter_node(raw, "filter")? {
            FilterNode::Group(group) => group,
            rule @ FilterNode::Rule(_) => FilterGroup {
                op: GroupOp::And,
                children: vec![rule],
            },
        };
        let types: HashMap<Id, PropertyType> = self
            .properties
            .iter()
            .map(|property| (property.id, property.kind.filter_type()))
            .collect();
        group
            .check("filter", 1, &types, &mut 0)
            .map_err(DatabaseError::invalid)?;
        Ok(group)
    }

    fn filter_node(&self, raw: &Value, path: &str) -> Result<FilterNode, DatabaseError> {
        let invalid = |field: &str| DatabaseError::invalid(format!("{path}{field}"));
        let object = raw.as_object().ok_or_else(|| invalid(""))?;
        if let Some(children) = object.get("children") {
            let op = match object.get("op").and_then(Value::as_str) {
                None | Some("and") => GroupOp::And,
                Some("or") => GroupOp::Or,
                Some(_) => return Err(invalid(".op")),
            };
            let children = children
                .as_array()
                .ok_or_else(|| invalid(".children"))?
                .iter()
                .enumerate()
                .map(|(index, child)| self.filter_node(child, &format!("{path}.children[{index}]")))
                .collect::<Result<_, _>>()?;
            return Ok(FilterNode::Group(FilterGroup { op, children }));
        }
        let key = object
            .get("property_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(".property_id"))?;
        let property = self.property(key, &format!("{path}.property_id"))?;
        let operator: FilterOperator = object
            .get("operator")
            .cloned()
            .and_then(|operator| serde_json::from_value(operator).ok())
            .ok_or_else(|| invalid(".operator"))?;
        let value = object.get("value").cloned().unwrap_or(Value::Null);
        let value = match &property.kind {
            PropertyKind::Select(_)
            | PropertyKind::Status(_)
            | PropertyKind::MultiSelect(_)
            | PropertyKind::Person
            | PropertyKind::CreatedBy
            | PropertyKind::LastEditedBy
                if value.as_str() != Some("") =>
            {
                self.ids(&property.kind, &value, true)
                    .ok_or_else(|| invalid(".value"))?
            }
            PropertyKind::UniqueId(_) => match &value {
                Value::String(text) => unique_id_number(&property.kind, text)
                    .map(Value::from)
                    .ok_or_else(|| invalid(".value"))?,
                _ => value,
            },
            _ => value,
        };
        Ok(FilterNode::Rule(FilterRule {
            property_id: property.id,
            operator,
            value,
        }))
    }

    fn sorts(&self, raw: &Value) -> Result<Vec<ViewSort>, DatabaseError> {
        let items = raw
            .as_array()
            .filter(|items| items.len() <= MAX_SORTS)
            .ok_or_else(|| DatabaseError::invalid("sorts"))?;
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let path = format!("sorts[{index}]");
                let key = item["property_id"]
                    .as_str()
                    .ok_or_else(|| DatabaseError::invalid(format!("{path}.property_id")))?;
                let property = self.property(key, &format!("{path}.property_id"))?;
                let direction = match item.get("direction").and_then(Value::as_str) {
                    None | Some("asc") => SortDirection::Asc,
                    Some("desc") => SortDirection::Desc,
                    Some(_) => return Err(DatabaseError::invalid(format!("{path}.direction"))),
                };
                Ok(ViewSort {
                    property_id: property.id,
                    direction,
                })
            })
            .collect()
    }

    fn person(&self, id: Id) -> AgentPerson {
        AgentPerson {
            id,
            name: self
                .members
                .iter()
                .find(|member| member.id == id)
                .map(|member| member.name.clone())
                .unwrap_or_default(),
        }
    }

    fn row(
        &self,
        row: DatabaseRow,
        related: &BTreeMap<String, RelatedPage>,
        origin: &str,
        workspace_id: Id,
    ) -> AgentRow {
        let mut names: HashMap<&str, usize> = HashMap::new();
        for property in &self.properties {
            *names.entry(property.name.as_str()).or_default() += 1;
        }
        let mut values = BTreeMap::new();
        let mut hidden_links = BTreeMap::new();
        for property in self
            .properties
            .iter()
            .filter(|property| !page_field(&property.kind))
        {
            let id = property.id.to_string();
            let key = if names[property.name.as_str()] > 1 {
                id.clone()
            } else {
                property.name.clone()
            };
            if let Some(hidden) = row.hidden_links.get(&id) {
                hidden_links.insert(key.clone(), *hidden);
            }
            values.insert(
                key,
                self.output_value(property, row.values.get(&id), related),
            );
        }
        AgentRow {
            id: row.id,
            url: row_url(origin, row.id, workspace_id),
            title: row.title,
            icon: row.icon,
            created_at: row.created_at,
            created_by: self.person(row.created_by),
            updated_at: row.updated_at,
            updated_by: self.person(row.updated_by),
            values,
            hidden_links,
        }
    }

    fn output_value(
        &self,
        property: &Property,
        raw: Option<&Value>,
        related: &BTreeMap<String, RelatedPage>,
    ) -> Value {
        let parsed = || raw.and_then(|raw| super::parse_value(&property.kind, raw).ok().flatten());
        let name = |id: &str| {
            property
                .kind
                .options()
                .into_iter()
                .find(|(option, _)| *option == id)
                .map_or_else(String::new, |(_, name)| name.to_owned())
        };
        match &property.kind {
            PropertyKind::Status(options) => match parsed() {
                Some(CellValue::Choice(id)) => json!(name(&id)),
                _ => options
                    .iter()
                    .find(|option| option.group == StatusGroup::Todo)
                    .map_or(Value::Null, |option| json!(option.name)),
            },
            PropertyKind::Select(_) => match parsed() {
                Some(CellValue::Choice(id)) => json!(name(&id)),
                _ => Value::Null,
            },
            PropertyKind::MultiSelect(_) => match parsed() {
                Some(CellValue::Choices(ids)) => {
                    json!(ids.iter().map(|id| name(id)).collect::<Vec<_>>())
                }
                _ => Value::Null,
            },
            PropertyKind::Person => match parsed() {
                Some(CellValue::People(ids)) => {
                    json!(
                        ids.into_iter()
                            .map(|id| self.person(id))
                            .collect::<Vec<_>>()
                    )
                }
                _ => Value::Null,
            },
            PropertyKind::Checkbox => json!(parsed().is_some()),
            PropertyKind::Text
            | PropertyKind::Number(_)
            | PropertyKind::Date(_)
            | PropertyKind::Url => parsed().map_or(Value::Null, |value| value.to_json()),
            PropertyKind::UniqueId(_) => match parsed() {
                Some(CellValue::UniqueId(number)) => {
                    json!(unique_id::text(&property.kind, number))
                }
                _ => Value::Null,
            },
            PropertyKind::Relation(_) => match raw {
                Some(Value::Array(ids)) if !ids.is_empty() => Value::Array(
                    ids.iter()
                        .map(|id| self.formula_value(&FormulaType::Page, id, related))
                        .collect(),
                ),
                _ => Value::Null,
            },
            PropertyKind::Formula(config) => raw.map_or(Value::Null, |raw| {
                self.formula_value(&config.result_type, raw, related)
            }),
            // Rollups and the other later types: as the read path returned them.
            _ => raw.cloned().unwrap_or(Value::Null),
        }
    }

    /// A computed value of `result_type` with its people as `{id, name}` and its pages as
    /// `{id, title}`; an error (`{"error": …}`) and the other types stay as they are.
    fn formula_value(
        &self,
        result_type: &FormulaType,
        raw: &Value,
        related: &BTreeMap<String, RelatedPage>,
    ) -> Value {
        match (result_type, raw) {
            (FormulaType::List(item), Value::Array(items)) => Value::Array(
                items
                    .iter()
                    .map(|raw| self.formula_value(item, raw, related))
                    .collect(),
            ),
            (FormulaType::Person, Value::String(id)) => id
                .parse()
                .map_or_else(|_| raw.clone(), |id| json!(self.person(id))),
            (FormulaType::Page, Value::String(id)) => id.parse().map_or_else(
                |_| raw.clone(),
                |page_id| {
                    json!(AgentPage {
                        id: page_id,
                        title: related
                            .get(id)
                            .map(|page| page.title.clone())
                            .unwrap_or_default(),
                    })
                },
            ),
            _ => raw.clone(),
        }
    }
}

/// What an `Invalid { field }` of an agent request means, for its error message.
#[must_use]
pub fn invalid_detail(field: &str) -> &'static str {
    match field {
        "limit" => "Send a limit from 1 to 100.",
        "cursor" => {
            "The cursor is not a next_cursor of this query. Send the previous page's next_cursor with the same filter, sorts, view_id and tz."
        }
        "tz" => "Send an IANA time zone name, such as Europe/Budapest.",
        "row_ids" => "Send 1 to 100 row ids.",
        "values" => "Send at most 50 values.",
        "sorts" => "Send a list of at most 10 sorts.",
        "title" => "Send a title of at most 500 characters.",
        "template_id" => {
            "No template of this database has this id. Send the id of one of the database's templates (its templates list), or leave template_id out."
        }
        "property" => {
            "No property has this name or id, or two properties share the name: use the property id from get_database."
        }
        "name" => "Send a name of 1 to 200 characters.",
        "type" => {
            "Send one of these types: text, number, select, multi_select, status, date, person, checkbox, url, email, phone, files, unique_id, relation, rollup, created_time, created_by, last_edited_time, last_edited_by, button, formula. The title property cannot be added and cannot change its type."
        }
        "position" => "Send a position of 0 or more.",
        "changes" => "Send at least one of name, type, config, add_options or position.",
        "add_options" => {
            "add_options takes option names or {name, color, group} objects, only for a select, multi-select or status property, and not together with config."
        }
        _ if field.starts_with("config") => {
            "The config does not fit the property's type. Select, multi-select and status take {\"options\": [...]}: names or {id, name, color, group} objects, at most 200, with names of 1 to 200 characters; group (todo, in_progress, done) only for a status. Colors: default, gray, brown, orange, yellow, green, blue, purple, pink, red. For other types, send the shape that get_database shows in the property's config."
        }
        _ if field.ends_with(".property_id") => {
            "No property has this name or id, or two properties share the name: use the property id."
        }
        _ if field.ends_with(".operator") => "The operator does not fit the property's type.",
        _ => {
            "The value is invalid for this database: an unknown or shared property name, an unknown option or member, or a value of the wrong type."
        }
    }
}

/// Properties that read the row's page (its title, and who made and edited it when): rows show
/// them as their own fields, and only the title changes, through `title`.
fn page_field(kind: &PropertyKind) -> bool {
    matches!(
        kind,
        PropertyKind::Title
            | PropertyKind::CreatedTime
            | PropertyKind::CreatedBy
            | PropertyKind::LastEditedTime
            | PropertyKind::LastEditedBy
    )
}

/// The number of a unique ID as text: `"PREFIX-12"` (the prefix in any letter case) or `"12"`.
fn unique_id_number(kind: &PropertyKind, text: &str) -> Option<i64> {
    let number = match kind {
        PropertyKind::UniqueId(super::UniqueIdConfig {
            prefix: Some(prefix),
            ..
        }) => text
            .get(..prefix.len())
            .filter(|start| start.eq_ignore_ascii_case(prefix))
            .and_then(|_| text[prefix.len()..].strip_prefix('-'))
            .unwrap_or(text),
        _ => text,
    };
    number.parse().ok()
}

fn row_url(origin: &str, row_id: Id, workspace_id: Id) -> String {
    format!("{origin}/docs/{row_id}?workspace={workspace_id}")
}

/// The opaque cursor of a query: the next offset and a hash of what the query matches.
fn query_hash(filter: &FilterGroup, sorts: &[ViewSort], view_id: Option<Id>, tz: &str) -> String {
    let input = json!({ "filter": filter, "sorts": sorts, "view_id": view_id.map(|id| id.to_string()), "tz": tz });
    let digest = Sha256::digest(input.to_string().as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn cursor_offset(cursor: Option<&str>, hash: &str) -> Result<usize, DatabaseError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    cursor
        .split_once('.')
        .filter(|(_, rest)| *rest == hash)
        .and_then(|(offset, _)| offset.parse().ok())
        .ok_or_else(|| DatabaseError::invalid("cursor"))
}

impl DatabaseRepository {
    async fn names(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
    ) -> Result<Names, DatabaseError> {
        let properties = self.properties(workspace_id, database_id, actor_id).await?;
        let members = sqlx::query(
            "SELECT users.id, users.display_name, users.email FROM memberships \
             JOIN users ON users.id = memberships.user_id WHERE memberships.workspace_id = $1",
        )
        .bind(workspace_id.to_string())
        .fetch_all(self.database.pool())
        .await?
        .into_iter()
        .filter_map(|row| {
            Some(Member {
                id: row.get::<String, _>("id").parse().ok()?,
                name: row
                    .get::<Option<String>, _>("display_name")
                    .unwrap_or_default(),
                email: row.get::<Option<String>, _>("email").unwrap_or_default(),
            })
        })
        .collect();
        Ok(Names {
            properties,
            members,
            actor_id,
        })
    }

    /// The schema of a database the actor can read.
    pub async fn agent_database(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        origin: &str,
    ) -> Result<AgentDatabase, DatabaseError> {
        let detail = self.get(workspace_id, database_id, actor_id, None).await?;
        let properties = detail
            .properties
            .into_iter()
            .map(|property| {
                let mut config = property.config;
                let options = config
                    .as_object_mut()
                    .and_then(|config| config.remove("options"))
                    .and_then(|options| options.as_array().cloned())
                    .map(|options| {
                        options
                            .into_iter()
                            .map(|option| AgentOption {
                                id: option["id"].as_str().unwrap_or_default().to_owned(),
                                name: option["name"].as_str().unwrap_or_default().to_owned(),
                                group: option.get("group").cloned(),
                            })
                            .collect()
                    });
                let config = config
                    .as_object()
                    .is_some_and(|object| !object.is_empty())
                    .then_some(config);
                AgentProperty {
                    id: property.id,
                    name: property.name,
                    property_type: property.property_type,
                    options,
                    config,
                }
            })
            .collect();
        Ok(AgentDatabase {
            id: detail.database.id,
            url: row_url(origin, detail.database.id, workspace_id),
            title: detail.database.title,
            icon: detail.database.icon,
            locked: detail.database.locked,
            row_count: detail.database.row_count,
            properties,
            views: detail
                .views
                .into_iter()
                .map(|view| AgentView {
                    id: view.id,
                    name: view.name,
                    view_type: view.view_type,
                })
                .collect(),
            templates: detail
                .templates
                .into_iter()
                .map(|template| AgentTemplate {
                    id: template.row.id,
                    name: template.name,
                })
                .collect(),
        })
    }

    /// One page of the rows that match a query, filtered and sorted in memory.
    pub async fn agent_query(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: AgentQuery,
        origin: &str,
    ) -> Result<AgentRowPage, DatabaseError> {
        let limit = input.limit.unwrap_or(DEFAULT_QUERY_ROWS);
        if !(1..=MAX_QUERY_ROWS).contains(&limit) {
            return Err(DatabaseError::invalid("limit"));
        }
        let scope = ComputeScope::with_tz(workspace_id, actor_id, input.tz.as_deref())
            .ok_or_else(|| DatabaseError::invalid("tz"))?;
        let time_zone = TimeZone::get(&scope.tz).map_err(|_| DatabaseError::invalid("tz"))?;
        let names = self.names(workspace_id, database_id, actor_id).await?;
        let mut filter = match &input.filter {
            Some(raw) => names.filter(raw)?,
            None => FilterGroup::default(),
        };
        let mut sorts = match &input.sorts {
            Some(raw) => names.sorts(raw)?,
            None => Vec::new(),
        };
        let detail = self.get_with(&scope, database_id, None).await?;
        if let Some(view_id) = input.view_id {
            let view = detail
                .views
                .iter()
                .find(|view| view.id == view_id)
                .ok_or(DatabaseError::ViewNotFound)?;
            filter = if filter.children.is_empty() {
                view.config.filter.clone()
            } else {
                FilterGroup {
                    op: GroupOp::And,
                    children: vec![
                        FilterNode::Group(view.config.filter.clone()),
                        FilterNode::Group(filter),
                    ],
                }
            };
            if input.sorts.is_none() {
                sorts.clone_from(&view.config.sorts);
            }
        }
        let hash = query_hash(&filter, &sorts, input.view_id, &scope.tz);
        let offset = cursor_offset(input.cursor.as_deref(), &hash)?;
        let ctx = QueryContext {
            user_id: actor_id,
            now: Timestamp::now(),
            time_zone,
            members: names
                .members
                .iter()
                .map(|member| (member.id, member.name.clone()))
                .collect(),
            related: detail
                .related_pages
                .iter()
                .map(|(id, page)| (id.clone(), page.title.clone()))
                .collect(),
        };
        let related = detail.related_pages;
        let rows = query_rows(detail.rows, &names.properties, &filter, &sorts, &ctx);
        let total = rows.len();
        let end = offset.saturating_add(limit).min(total);
        let items = rows
            .into_iter()
            .skip(offset)
            .take(end.saturating_sub(offset))
            .map(|row| names.row(row, &related, origin, workspace_id))
            .collect();
        Ok(AgentRowPage {
            items,
            next_cursor: (end < total).then(|| format!("{end}.{hash}")),
            total: total as u64,
        })
    }

    /// A live row of the database, with its computed values.
    pub async fn agent_row(
        &self,
        workspace_id: Id,
        database_id: Id,
        row_id: Id,
        actor_id: Id,
        origin: &str,
    ) -> Result<AgentRow, DatabaseError> {
        let names = self.names(workspace_id, database_id, actor_id).await?;
        let mut tx = self.database.transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        let mut rows = rows_in_tx(&mut tx, database_id, Some(row_id)).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let computed = computed::fill(&mut tx, &scope, database_id, &properties, &mut rows).await?;
        tx.rollback().await?;
        let row = rows.pop().ok_or(DatabaseError::NotFound)?;
        Ok(names.row(row, &computed.related_pages, origin, workspace_id))
    }

    /// Values as agents send them (names or ids as keys, option names, emails), as
    /// [`Self::update_values`] and [`Self::create_row`] take them.
    pub async fn agent_values(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        values: &Map<String, Value>,
    ) -> Result<Vec<(Id, Value)>, DatabaseError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        self.names(workspace_id, database_id, actor_id)
            .await?
            .input_values(values)
    }

    /// `error` with a `values.<property id>` field named by the key the agent sent for that
    /// property, as the errors of [`Self::agent_values`] are.
    #[must_use]
    pub fn agent_value_error(
        error: DatabaseError,
        raw: &Map<String, Value>,
        ids: &[Id],
    ) -> DatabaseError {
        let DatabaseError::Invalid { field } = &error else {
            return error;
        };
        raw.keys()
            .zip(ids)
            .find(|(_, id)| field.strip_prefix("values.") == Some(id.to_string().as_str()))
            .map_or(error, |(key, _)| {
                DatabaseError::invalid(format!("values.{key}"))
            })
    }

    /// Trashes the live rows among `row_ids` (see [`Self::trash_rows`]) and returns how many.
    pub async fn agent_trash_rows(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        row_ids: &[Id],
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<u64, DatabaseError> {
        if row_ids.is_empty() || row_ids.len() > MAX_AGENT_TRASH {
            return Err(DatabaseError::invalid("row_ids"));
        }
        let mut tx = self.database.transaction().await?;
        let page = open(&mut tx, workspace_id, database_id, actor_id).await?;
        let live: HashSet<Id> = templates::row_ids(
            &mut tx,
            workspace_id,
            page.space(actor_id),
            database_id,
            database_id,
        )
        .await?
        .into_iter()
        .collect();
        tx.rollback().await?;
        let trashed: HashSet<&Id> = row_ids.iter().filter(|id| live.contains(id)).collect();
        self.trash_rows(
            workspace_id,
            database_id,
            actor_id,
            row_ids,
            request_id,
            now,
        )
        .await?;
        Ok(trashed.len() as u64)
    }
}
