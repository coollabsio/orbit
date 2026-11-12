//! A database's views and its first schema for API tokens and MCP tools: filters, sorts and columns
//! go by property name or id and options by name ([`super::agent`]), for a database's own views
//! and for the views of a linked view block. A new database takes its properties and views in one
//! write.

use orbit_platform::{Id, TimestampMillis};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::agent::{Names, property_by_key};
use super::agent_schema::{check_position, parse_type, property_name, with_known_options};
use super::{
    DatabaseError, DatabaseRepository, DatabaseSchema, DatabaseView, FilterGroup, FilterNode,
    MAX_PROPERTIES, MAX_VIEWS, NewView, Property, PropertyKind, PropertyType, SchemaProperty,
    SchemaView, ViewChanges, ViewColumn, ViewConfig, ViewType, Views, links, open,
    properties_in_tx, views_in_tx,
};

/// A view as agents write it, for a new view and for a view of a new database.
#[derive(Debug, Default, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = PublicDatabaseViewSpec)]
pub struct AgentViewSpec {
    /// 1 to 200 characters.
    pub name: String,
    /// `table` (default), `board`, `list`, `calendar`, `gallery`, `timeline` or `form`.
    #[serde(rename = "type", default)]
    pub view_type: ViewType,
    /// An AND/OR group of rules, as in `query_database`; `property_id` takes a property name or
    /// id and `value` option names. Omitted: every row.
    #[schema(value_type = Option<Object>)]
    pub filter: Option<Value>,
    /// Up to 10 `{property_id, direction}` with names or ids.
    #[schema(value_type = Option<Vec<Object>>)]
    pub sorts: Option<Value>,
    /// The visible properties, in order, by name or id. The other properties are hidden. The title
    /// always shows, first unless listed. Omitted: every property.
    pub columns: Option<Vec<String>>,
    /// Board only: the `select`, `status`, `multi_select` or `person` property of the columns.
    pub group_by: Option<String>,
}

/// A change of a view; send at least one field. `filter`, `sorts`, `columns` and `group_by`
/// replace what the view has; fields left out stay.
#[derive(Debug, Default, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = PublicDatabaseViewChanges)]
pub struct AgentViewChanges {
    /// 1 to 200 characters.
    pub name: Option<String>,
    /// As in a new view. An empty group (`{"op": "and", "children": []}`) clears the filter.
    #[schema(value_type = Option<Object>)]
    pub filter: Option<Value>,
    /// As in a new view. An empty list clears the sorts.
    #[schema(value_type = Option<Vec<Object>>)]
    pub sorts: Option<Value>,
    /// As in a new view.
    pub columns: Option<Vec<String>>,
    /// As in a new view.
    pub group_by: Option<String>,
    /// The new index among the views.
    pub position: Option<i64>,
}

/// A view as agents read it: the same names that `AgentViewSpec` takes.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = PublicDatabaseViewDetail)]
pub struct AgentViewDetail {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub view_type: ViewType,
    /// Index among the views.
    pub position: i64,
    /// As `query_database` takes it; a property that two properties share is shown by id.
    #[schema(value_type = Object)]
    pub filter: Value,
    #[schema(value_type = Vec<Object>)]
    pub sorts: Value,
    /// The visible properties in order.
    pub columns: Vec<String>,
    /// Board only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
}

/// A property of a new database, besides its title property "Name".
#[derive(Debug, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = PublicDatabasePropertySpec)]
pub struct AgentPropertySpec {
    /// 1 to 200 characters.
    pub name: String,
    /// `text`, `number`, `select`, `multi_select`, `status`, `date`, `person`, `checkbox`, `url`,
    /// `email`, `phone`, `files`, `created_time`, `created_by`, `last_edited_time` or
    /// `last_edited_by`. Add relations, rollups, formulas, buttons and unique IDs afterwards.
    #[serde(rename = "type")]
    pub property_type: String,
    /// The type's config. Select, multi-select and status take `{"options": [...]}` with option
    /// names or `{name, color, group}` objects.
    #[schema(value_type = Option<Object>)]
    pub config: Option<Value>,
}

impl AgentViewSpec {
    /// This spec's filter, sorts, columns and group on `config`.
    fn apply(&self, names: &Names, config: &mut ViewConfig) -> Result<(), DatabaseError> {
        apply(
            names,
            config,
            Layout {
                filter: self.filter.as_ref(),
                sorts: self.sorts.as_ref(),
                columns: self.columns.as_deref(),
                group_by: self.group_by.as_deref(),
            },
        )
    }
}

impl Names {
    /// The property's name, or its id when another property has the same name.
    fn key(&self, id: Id) -> String {
        let Some(property) = self.properties.iter().find(|property| property.id == id) else {
            return id.to_string();
        };
        let shared = self
            .properties
            .iter()
            .filter(|other| other.name == property.name)
            .count()
            > 1;
        if shared {
            id.to_string()
        } else {
            property.name.clone()
        }
    }

    /// `value` with the option ids of a select, status or multi-select property as names.
    fn option_names(&self, id: Id, value: &Value) -> Value {
        let Some(property) = self.properties.iter().find(|property| property.id == id) else {
            return value.clone();
        };
        let options = property.kind.options();
        let name = |item: &Value| match item.as_str() {
            Some(text) => options
                .iter()
                .find(|(option, _)| *option == text)
                .map_or_else(|| item.clone(), |(_, name)| json!(name)),
            None => item.clone(),
        };
        match value {
            Value::Array(items) if !options.is_empty() => items.iter().map(name).collect(),
            Value::String(_) if !options.is_empty() => name(value),
            _ => value.clone(),
        }
    }

    fn filter_json(&self, group: &FilterGroup) -> Value {
        let children: Vec<Value> = group
            .children
            .iter()
            .map(|child| match child {
                FilterNode::Group(group) => self.filter_json(group),
                FilterNode::Rule(rule) => json!({
                    "property_id": self.key(rule.property_id),
                    "operator": rule.operator,
                    "value": self.option_names(rule.property_id, &rule.value),
                }),
            })
            .collect();
        json!({ "op": group.op, "children": children })
    }

    /// `view` with property and option names.
    pub(super) fn view_detail(&self, view: &DatabaseView) -> AgentViewDetail {
        let config = &view.config;
        AgentViewDetail {
            id: view.id,
            name: view.name.clone(),
            view_type: view.view_type,
            position: view.position,
            filter: self.filter_json(&config.filter),
            sorts: json!(
                config
                    .sorts
                    .iter()
                    .map(|sort| json!({
                        "property_id": self.key(sort.property_id),
                        "direction": sort.direction,
                    }))
                    .collect::<Vec<_>>()
            ),
            columns: config
                .columns
                .iter()
                .filter(|column| !column.hidden)
                .map(|column| self.key(column.property_id))
                .collect(),
            group_by: config.group_by.map(|id| self.key(id)),
        }
    }
}

struct Layout<'a> {
    filter: Option<&'a Value>,
    sorts: Option<&'a Value>,
    columns: Option<&'a [String]>,
    group_by: Option<&'a str>,
}

fn apply(names: &Names, config: &mut ViewConfig, layout: Layout<'_>) -> Result<(), DatabaseError> {
    if let Some(filter) = layout.filter {
        config.filter = names.filter(filter)?;
    }
    if let Some(sorts) = layout.sorts {
        config.sorts = names.sorts(sorts)?;
    }
    if let Some(columns) = layout.columns {
        config.columns = columns_of(names, columns, &config.columns)?;
    }
    if let Some(key) = layout.group_by {
        config.group_by = Some(names.property(key, "group_by")?.id);
    }
    Ok(())
}

/// The listed properties as visible columns in that order, then the others hidden; widths stay.
fn columns_of(
    names: &Names,
    keys: &[String],
    current: &[ViewColumn],
) -> Result<Vec<ViewColumn>, DatabaseError> {
    if keys.len() > MAX_PROPERTIES {
        return Err(DatabaseError::invalid("columns"));
    }
    let mut shown: Vec<Id> = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        let id = property_by_key(&names.properties, key, &format!("columns[{index}]"))?.id;
        if !shown.contains(&id) {
            shown.push(id);
        }
    }
    if let Some(title) = names
        .properties
        .iter()
        .find(|property| property.kind == PropertyKind::Title)
        && !shown.contains(&title.id)
    {
        shown.insert(0, title.id);
    }
    let column = |property: &Property, hidden: bool| {
        let width = current
            .iter()
            .find(|column| column.property_id == property.id)
            .map(|column| u64::from(column.width));
        ViewColumn::new(
            property.id,
            property.kind == PropertyKind::Title,
            width,
            hidden,
        )
    };
    let by_id = |id: &Id| names.properties.iter().find(|property| property.id == *id);
    Ok(shown
        .iter()
        .filter_map(by_id)
        .map(|property| column(property, false))
        .chain(
            names
                .properties
                .iter()
                .filter(|property| !shown.contains(&property.id))
                .map(|property| column(property, true)),
        )
        .collect())
}

fn view_name(name: &str) -> Result<String, DatabaseError> {
    property_name(name).map_err(|_| DatabaseError::invalid("name"))
}

/// `error` with its field path inside `prefix`, such as `views[1].`.
fn within(error: DatabaseError, prefix: &str) -> DatabaseError {
    match error {
        DatabaseError::Invalid { field } => DatabaseError::invalid(format!("{prefix}{field}")),
        error => error,
    }
}

/// Types a new database takes: the others need more than a name and a config (a relation needs
/// its other database), so they are added to the database afterwards.
fn starts_database(property_type: PropertyType) -> bool {
    !matches!(
        property_type,
        PropertyType::Title
            | PropertyType::Relation
            | PropertyType::Rollup
            | PropertyType::Formula
            | PropertyType::Button
            | PropertyType::UniqueId
    )
}

impl DatabaseRepository {
    /// The views of a database's own set, or of the linked view block `link`.
    pub async fn agent_views(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        link: Option<Id>,
    ) -> Result<Vec<AgentViewDetail>, DatabaseError> {
        let views = self
            .stored_views(workspace_id, database_id, actor_id, link)
            .await?;
        let names = self.names(workspace_id, database_id, actor_id).await?;
        Ok(views.iter().map(|view| names.view_detail(view)).collect())
    }

    async fn stored_views(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        link: Option<Id>,
    ) -> Result<Vec<DatabaseView>, DatabaseError> {
        let mut tx = self.database.transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let set = match link {
            Some(link_id) => {
                links::require_link(&mut tx, database_id, link_id).await?;
                Views::Link(link_id)
            }
            None => Views::Own,
        };
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let views = views_in_tx(&mut tx, database_id, set, &properties).await?;
        tx.rollback().await?;
        Ok(views)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn agent_create_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        spec: AgentViewSpec,
        link: Option<Id>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<AgentViewDetail, DatabaseError> {
        let names = self.names(workspace_id, database_id, actor_id).await?;
        let mut config = ViewConfig::for_properties(&names.properties);
        spec.apply(&names, &mut config)?;
        let input = NewView {
            name: view_name(&spec.name)?,
            view_type: spec.view_type,
            config: Some(config),
            duplicate_of: None,
            link,
        };
        let view = self
            .create_view(workspace_id, database_id, actor_id, input, request_id, now)
            .await?;
        Ok(names.view_detail(&view))
    }

    /// Changes the view `view_id` of a database's own set, or of the linked view block `link`.
    #[allow(clippy::too_many_arguments)]
    pub async fn agent_update_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        view_id: Id,
        link: Option<Id>,
        changes: AgentViewChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<AgentViewDetail, DatabaseError> {
        let layout = Layout {
            filter: changes.filter.as_ref(),
            sorts: changes.sorts.as_ref(),
            columns: changes.columns.as_deref(),
            group_by: changes.group_by.as_deref(),
        };
        let reshapes = layout.filter.is_some()
            || layout.sorts.is_some()
            || layout.columns.is_some()
            || layout.group_by.is_some();
        if !reshapes && changes.name.is_none() && changes.position.is_none() {
            return Err(DatabaseError::invalid("changes"));
        }
        let name = changes.name.as_deref().map(view_name).transpose()?;
        let position = changes.position.map(check_position).transpose()?;
        let names = self.names(workspace_id, database_id, actor_id).await?;
        let config = if reshapes {
            let mut config = self
                .stored_views(workspace_id, database_id, actor_id, link)
                .await?
                .into_iter()
                .find(|view| view.id == view_id)
                .ok_or(DatabaseError::ViewNotFound)?
                .config;
            apply(&names, &mut config, layout)?;
            Some(config)
        } else {
            None
        };
        let view = self
            .update_view(
                workspace_id,
                database_id,
                view_id,
                actor_id,
                ViewChanges {
                    name,
                    config,
                    position,
                    link,
                },
                request_id,
                now,
            )
            .await?;
        Ok(names.view_detail(&view))
    }

    /// The schema of a new database: a title property "Name", `properties` after it (default: a
    /// "Tags" multi-select) and `views` (default: one "Table" view). Views name properties as
    /// views of a database do.
    pub async fn agent_schema(
        &self,
        workspace_id: Id,
        actor_id: Id,
        properties: Option<Vec<AgentPropertySpec>>,
        views: Option<Vec<AgentViewSpec>>,
    ) -> Result<DatabaseSchema, DatabaseError> {
        let properties = properties.unwrap_or_else(|| {
            vec![AgentPropertySpec {
                name: "Tags".to_owned(),
                property_type: "multi_select".to_owned(),
                config: None,
            }]
        });
        let views = views.unwrap_or_else(|| {
            vec![AgentViewSpec {
                name: "Table".to_owned(),
                ..AgentViewSpec::default()
            }]
        });
        if properties.len() >= MAX_PROPERTIES {
            return Err(DatabaseError::invalid("properties"));
        }
        if views.is_empty() || views.len() > MAX_VIEWS {
            return Err(DatabaseError::invalid("views"));
        }
        let mut parsed = vec![Property {
            id: Id::new_v7(),
            name: "Name".to_owned(),
            kind: PropertyKind::Title,
            position: 0,
        }];
        for (index, spec) in properties.iter().enumerate() {
            let at = |field: &str| DatabaseError::invalid(format!("properties[{index}].{field}"));
            let property_type = parse_type(&spec.property_type)
                .ok()
                .filter(|property_type| starts_database(*property_type))
                .ok_or_else(|| at("type"))?;
            let config = spec
                .config
                .clone()
                .map(|config| with_known_options(config, &[]))
                .transpose()
                .map_err(|_| at("config"))?;
            parsed.push(Property {
                id: Id::new_v7(),
                name: property_name(&spec.name).map_err(|_| at("name"))?,
                kind: PropertyKind::parse(property_type, config.as_ref())
                    .map_err(|field| at(&field))?,
                position: index as i64 + 1,
            });
        }
        let members = self.members(workspace_id).await?;
        let names = Names::new(parsed, members, actor_id);
        let schema_views = views
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let prefix = format!("views[{index}].");
                let mut config = ViewConfig::for_properties(&names.properties);
                spec.apply(&names, &mut config)
                    .map_err(|error| within(error, &prefix))?;
                Ok(SchemaView {
                    name: view_name(&spec.name).map_err(|error| within(error, &prefix))?,
                    view_type: spec.view_type,
                    config,
                })
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        // The options keep the ids that the views' filters name.
        let schema_properties = names
            .properties
            .iter()
            .map(|property| SchemaProperty {
                id: property.id,
                name: property.name.clone(),
                property_type: property.kind.property_type(),
                config: property.kind.config_json(),
            })
            .collect();
        DatabaseSchema::new(schema_properties, schema_views).map(|(schema, _)| schema)
    }
}
