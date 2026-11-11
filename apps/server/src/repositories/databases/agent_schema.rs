//! A database's schema for API tokens and MCP tools: add, change and delete properties by name or
//! id. Options go by name: an option sent without its id keeps the id, color and group of the
//! option with that name, so a config that an agent writes from `get_database` loses no values.

use orbit_platform::{Id, TimestampMillis};
use serde_json::{Map, Value, json};

use super::agent::property_by_key;
use super::{
    DatabaseError, DatabaseProperty, DatabaseRepository, MAX_NAME_CHARS, NewProperty, Property,
    PropertyChanges, PropertyType, PropertyUpdate,
};

/// A property change as agents send it; see [`DatabaseRepository::agent_update_property`].
#[derive(Debug, Default)]
pub struct AgentPropertyChanges {
    pub name: Option<String>,
    pub property_type: Option<String>,
    /// The whole config; options left out are removed from every row.
    pub config: Option<Value>,
    /// Options to add to the current ones (names, or `{name, color, group}`); names the property
    /// already has are skipped. Cannot be sent with `config`.
    pub add_options: Vec<Value>,
    pub position: Option<i64>,
    pub dry_run: bool,
}

impl DatabaseRepository {
    #[allow(clippy::too_many_arguments)]
    pub async fn agent_create_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        name: &str,
        property_type: &str,
        config: Option<Value>,
        position: Option<i64>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseProperty, DatabaseError> {
        let input = NewProperty::Fresh {
            name: property_name(name)?,
            property_type: parse_type(property_type)?,
            config: config
                .map(|config| with_known_options(config, &[]))
                .transpose()?,
            position: position.map(check_position).transpose()?,
        };
        self.create_property(workspace_id, database_id, actor_id, input, request_id, now)
            .await
    }

    /// Changes the property `key` names (its id or name). `add_options` appends to the current
    /// options; `config` replaces the config, with options matched to the current ones by id or
    /// name.
    #[allow(clippy::too_many_arguments)]
    pub async fn agent_update_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        key: &str,
        changes: AgentPropertyChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<PropertyUpdate, DatabaseError> {
        let properties = self.properties(workspace_id, database_id, actor_id).await?;
        let property = property_by_key(&properties, key, "property")?;
        // Options of another type are not the ones the config means.
        let current = match changes.property_type {
            Some(_) => Vec::new(),
            None => current_options(property),
        };
        let config = match (changes.config, changes.add_options.is_empty()) {
            (Some(_), false) => return Err(DatabaseError::invalid("add_options")),
            (Some(config), true) => Some(with_known_options(config, &current)?),
            (None, false) => Some(with_added_options(property, &changes.add_options)?),
            (None, true) => None,
        };
        let changes = PropertyChanges {
            name: changes.name.as_deref().map(property_name).transpose()?,
            property_type: changes
                .property_type
                .as_deref()
                .map(parse_type)
                .transpose()?,
            config,
            position: changes.position.map(check_position).transpose()?,
            dry_run: changes.dry_run,
        };
        if changes.name.is_none()
            && changes.property_type.is_none()
            && changes.config.is_none()
            && changes.position.is_none()
        {
            return Err(DatabaseError::invalid("changes"));
        }
        self.update_property(
            workspace_id,
            database_id,
            property.id,
            actor_id,
            changes,
            request_id,
            now,
        )
        .await
    }

    /// Deletes the property `key` names, with its values; returns its id.
    pub async fn agent_delete_property(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        key: &str,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<Id, DatabaseError> {
        let properties = self.properties(workspace_id, database_id, actor_id).await?;
        let property_id = property_by_key(&properties, key, "property")?.id;
        self.delete_property(
            workspace_id,
            database_id,
            property_id,
            actor_id,
            request_id,
            now,
        )
        .await?;
        Ok(property_id)
    }
}

fn property_name(name: &str) -> Result<String, DatabaseError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(DatabaseError::invalid("name"));
    }
    Ok(name.to_owned())
}

fn parse_type(raw: &str) -> Result<PropertyType, DatabaseError> {
    PropertyType::parse(raw).ok_or_else(|| DatabaseError::invalid("type"))
}

fn check_position(position: i64) -> Result<i64, DatabaseError> {
    if position < 0 {
        return Err(DatabaseError::invalid("position"));
    }
    Ok(position)
}

/// The property's options as stored (`{id, name, color, group?}`); empty for other types.
fn current_options(property: &Property) -> Vec<Map<String, Value>> {
    property.kind.config_json()["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| option.as_object().cloned())
        .collect()
}

/// `config` with each option completed from the current option of the same id or name: a name
/// alone keeps that option's id, color and group. A plain string is an option name.
fn with_known_options(
    mut config: Value,
    current: &[Map<String, Value>],
) -> Result<Value, DatabaseError> {
    let Some(options) = config.get_mut("options").and_then(Value::as_array_mut) else {
        return Ok(config);
    };
    for (index, option) in options.iter_mut().enumerate() {
        *option = Value::Object(known_option(option, current, index, "config.options")?);
    }
    Ok(config)
}

/// The current config with `added` options after the current ones; names the property already has
/// (in any letter case) are skipped.
fn with_added_options(property: &Property, added: &[Value]) -> Result<Value, DatabaseError> {
    let mut config = property.kind.config_json();
    let Some(options) = config.get_mut("options").and_then(Value::as_array_mut) else {
        return Err(DatabaseError::invalid("add_options"));
    };
    for (index, option) in added.iter().enumerate() {
        let option = known_option(option, &[], index, "add_options")?;
        let name = option_name(&option).to_lowercase();
        let known = options
            .iter()
            .any(|other| other["name"].as_str().unwrap_or_default().to_lowercase() == name);
        if !known {
            options.push(Value::Object(option));
        }
    }
    Ok(config)
}

fn known_option(
    option: &Value,
    current: &[Map<String, Value>],
    index: usize,
    path: &str,
) -> Result<Map<String, Value>, DatabaseError> {
    let mut option = match option {
        Value::String(name) => json!({ "name": name })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        Value::Object(option) => option.clone(),
        _ => return Err(DatabaseError::invalid(format!("{path}[{index}]"))),
    };
    let name = option_name(&option).trim().to_lowercase();
    let same = match option.get("id").and_then(Value::as_str) {
        Some(id) => current.iter().find(|other| other["id"] == id),
        None => current
            .iter()
            .find(|other| other["name"].as_str().unwrap_or_default().to_lowercase() == name),
    };
    if let Some(same) = same {
        for field in ["id", "color", "group"] {
            if let (false, Some(value)) = (option.contains_key(field), same.get(field)) {
                option.insert(field.to_owned(), value.clone());
            }
        }
    }
    Ok(option)
}

fn option_name(option: &Map<String, Value>) -> &str {
    option
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
}
