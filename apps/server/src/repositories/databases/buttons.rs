//! Button properties (spec P3 item 15, D-P3-6). A button stores no value: its config lists up to
//! [`MAX_ACTIONS`] data actions, and a press runs the server actions (`set_values` on the pressed
//! row, `add_row` to this or another database) in one transaction, so a failed action rolls back
//! every earlier one. `open_url` runs on the client after the server actions.
//!
//! Action values are sent as in `PATCH …/rows/{row_id}/values`, with placeholders resolved at the
//! press: `"me"` in a person value, `"today"` and `"now"` as a date (or as its `start`), and, in
//! `add_row` only, `"this_row"` (the pressed row's id, for a relation) and a string under the
//! title property (the new row's title). Values of properties that no longer exist are skipped,
//! so a deleted property does not break the button.

use std::collections::{BTreeMap, HashSet};

use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::search::{SearchRows, refresh_search_text};
use super::templates::utc_today;
use super::{
    ComputeScope, DatabaseError, DatabaseRepository, DatabaseRow, DatePoint, MAX_CELL_ITEMS,
    MAX_NAME_CHARS, NewRow, Property, PropertyKind, RowPlacement, open, parse_values,
    properties_in_tx, record_with, require_row, single_row, valid_url, write_values,
};

pub const MAX_ACTIONS: usize = 5;
const MAX_CONFIRM_CHARS: usize = 500;
/// A row title, as `POST …/rows` takes it.
const MAX_TITLE_CHARS: usize = 500;
const ME: &str = "me";
const TODAY: &str = "today";
const NOW: &str = "now";
const THIS_ROW: &str = "this_row";

/// A button property's config.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseButtonConfig)]
pub struct ButtonConfig {
    /// What the button shows, at most 200 characters; empty shows the property name.
    #[serde(default)]
    pub label: String,
    /// Run in order on a press, at most 5, with at most one `open_url`.
    #[serde(default)]
    pub actions: Vec<ButtonAction>,
    /// A question the client asks before the press runs (at most 500 characters).
    #[serde(default)]
    pub confirm: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[schema(as = DatabaseButtonAction)]
pub enum ButtonAction {
    /// Writes values on the pressed row.
    SetValues {
        #[schema(value_type = Object)]
        values: BTreeMap<Id, Value>,
    },
    /// Adds a row (last) to this or another database of the workspace.
    AddRow {
        #[schema(value_type = String)]
        database_id: Id,
        #[serde(default)]
        #[schema(value_type = Object)]
        values: BTreeMap<Id, Value>,
    },
    /// Opens an `http(s)` URL on the client after the server actions.
    OpenUrl { url: String },
}

impl ButtonConfig {
    /// Parses a stored or sent config; errors name the field path.
    pub(super) fn parse(config: &Value) -> Result<Self, String> {
        let mut config: Self =
            serde_json::from_value(config.clone()).map_err(|_| "config".to_owned())?;
        config.label = config.label.trim().to_owned();
        if config.label.chars().count() > MAX_NAME_CHARS {
            return Err("config.label".to_owned());
        }
        config.confirm = config
            .confirm
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty());
        if config
            .confirm
            .as_ref()
            .is_some_and(|text| text.chars().count() > MAX_CONFIRM_CHARS)
        {
            return Err("config.confirm".to_owned());
        }
        if config.actions.len() > MAX_ACTIONS {
            return Err("config.actions".to_owned());
        }
        let mut urls = 0;
        for (index, action) in config.actions.iter().enumerate() {
            let at = |field: &str| format!("config.actions[{index}].{field}");
            match action {
                ButtonAction::SetValues { values } | ButtonAction::AddRow { values, .. } => {
                    if values.len() > MAX_CELL_ITEMS {
                        return Err(at("values"));
                    }
                }
                ButtonAction::OpenUrl { url } => {
                    urls += 1;
                    if urls > 1 || !valid_url(url) {
                        return Err(at("url"));
                    }
                }
            }
        }
        Ok(config)
    }
}

/// What a press did.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabaseButtonPress)]
pub struct ButtonPress {
    /// The pressed row after the actions.
    pub row: DatabaseRow,
    /// The rows that `add_row` actions made, in action order.
    pub created_rows: Vec<CreatedRow>,
    /// The `open_url` action's URL, for the client to open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = DatabaseButtonCreatedRow)]
pub struct CreatedRow {
    #[schema(value_type = String)]
    pub database_id: Id,
    pub row: DatabaseRow,
}

/// What the placeholders of a press stand for.
struct Press {
    actor_id: Id,
    row_id: Id,
    today: String,
    now: String,
}

impl Press {
    /// `values` with placeholders resolved, for the properties that still exist. In `add_row`
    /// (`adding`), `"this_row"` is the pressed row and the title property's string is returned
    /// as the title.
    fn resolve(
        &self,
        properties: &[Property],
        values: &BTreeMap<Id, Value>,
        adding: bool,
    ) -> (Option<String>, Vec<(Id, Value)>) {
        let mut title = None;
        let mut resolved = Vec::with_capacity(values.len());
        for (id, raw) in values {
            let Some(property) = properties.iter().find(|property| property.id == *id) else {
                continue;
            };
            if adding && property.kind == PropertyKind::Title {
                match raw.as_str() {
                    Some(text) => title = Some(text.trim().chars().take(MAX_TITLE_CHARS).collect()),
                    None => resolved.push((*id, raw.clone())),
                }
                continue;
            }
            resolved.push((*id, self.value(&property.kind, raw, adding)));
        }
        (title, resolved)
    }

    fn value(&self, kind: &PropertyKind, raw: &Value, adding: bool) -> Value {
        if adding && raw == THIS_ROW {
            return json!([self.row_id.to_string()]);
        }
        match kind {
            PropertyKind::Person => {
                let me = json!(self.actor_id.to_string());
                match raw {
                    Value::String(text) if text == ME => json!([me]),
                    Value::Array(items) => {
                        let mut seen = HashSet::new();
                        Value::Array(
                            items
                                .iter()
                                .map(|item| if item == ME { me.clone() } else { item.clone() })
                                .filter(|item| seen.insert(item.to_string()))
                                .collect(),
                        )
                    }
                    other => other.clone(),
                }
            }
            PropertyKind::Date(_) => {
                let point = |value: &Value| match value.as_str() {
                    Some(TODAY) => Some(self.today.clone()),
                    Some(NOW) => Some(self.now.clone()),
                    _ => None,
                };
                if let Some(start) = point(raw) {
                    return json!({ "start": start, "end": null });
                }
                let mut raw = raw.clone();
                if let Some(map) = raw.as_object_mut() {
                    for key in ["start", "end"] {
                        if let Some(point) = map.get(key).and_then(point) {
                            map.insert(key.to_owned(), json!(point));
                        }
                    }
                }
                raw
            }
            _ => raw.clone(),
        }
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` of `now`.
fn utc_moment(now: TimestampMillis) -> String {
    let seconds = now.as_millis().div_euclid(1_000).rem_euclid(86_400);
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        utc_today(now),
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The error of action `index`, its field path prefixed with the action.
fn in_action(index: usize) -> impl Fn(DatabaseError) -> DatabaseError {
    move |error| match error {
        DatabaseError::Invalid { field } => DatabaseError::Invalid {
            field: format!("actions[{index}].{field}"),
        },
        other => other,
    }
}

impl DatabaseRepository {
    /// Runs the server actions of button `property_id` on row `row_id` in one transaction.
    /// `today` (`YYYY-MM-DD`, the caller's local day) is what `"today"` means; the UTC day when
    /// absent. Needs access to this database and to each `add_row` target (else 404, and
    /// nothing is written).
    #[allow(clippy::too_many_arguments)]
    pub async fn press_button(
        &self,
        workspace_id: Id,
        database_id: Id,
        row_id: Id,
        property_id: Id,
        actor_id: Id,
        today: Option<String>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<ButtonPress, DatabaseError> {
        let today = match today {
            Some(day) => match DatePoint::parse(&day) {
                Some(DatePoint::Day(day)) => day,
                _ => return Err(DatabaseError::invalid("today")),
            },
            None => utc_today(now),
        };
        let press = Press {
            actor_id,
            row_id,
            today,
            now: utc_moment(now),
        };
        let mut tx = self.database.immediate_transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        require_row(&mut tx, database_id, row_id).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let Some(PropertyKind::Button(config)) = properties
            .iter()
            .find(|property| property.id == property_id)
            .map(|property| property.kind.clone())
        else {
            return Err(DatabaseError::PropertyNotFound);
        };
        let mut changed: Vec<Id> = Vec::new();
        let mut created_rows = Vec::new();
        let mut open_url = None;
        for (index, action) in config.actions.iter().enumerate() {
            let in_action = in_action(index);
            match action {
                ButtonAction::SetValues { values } => {
                    let (_, input) = press.resolve(&properties, values, false);
                    let parsed = parse_values(&mut tx, workspace_id, &properties, &input)
                        .await
                        .map_err(&in_action)?;
                    write_values(&mut tx, workspace_id, row_id, &parsed, actor_id, now).await?;
                    changed.extend(parsed.iter().map(|(id, _)| *id));
                }
                ButtonAction::AddRow {
                    database_id: target,
                    values,
                } => {
                    let target_properties = if *target == database_id {
                        properties.clone()
                    } else {
                        open(&mut tx, workspace_id, *target, actor_id).await?;
                        properties_in_tx(&mut tx, *target).await?
                    };
                    let (title, input) = press.resolve(&target_properties, values, true);
                    let row = Self::insert_row_in_tx(
                        &self.database,
                        &mut tx,
                        Id::new_v7(),
                        workspace_id,
                        *target,
                        actor_id,
                        NewRow {
                            title: title.unwrap_or_default(),
                            icon: None,
                            values: input,
                            placement: RowPlacement::Last,
                        },
                        None,
                        request_id,
                        now,
                    )
                    .await
                    .map_err(&in_action)?;
                    created_rows.push(CreatedRow {
                        database_id: *target,
                        row,
                    });
                }
                ButtonAction::OpenUrl { url } => open_url = Some(url.clone()),
            }
        }
        if !changed.is_empty() {
            refresh_search_text(
                &mut tx,
                workspace_id,
                database_id,
                &properties,
                SearchRows::One(row_id),
            )
            .await?;
        }
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            "database.button_pressed",
            database_id,
            json!({
                "row_id": row_id.to_string(),
                "property_id": property_id.to_string(),
                "property_ids": changed.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "created_row_ids": created_rows
                    .iter()
                    .map(|created| created.row.id.to_string())
                    .collect::<Vec<_>>(),
            }),
            request_id,
            now,
        )
        .await?;
        let scope = ComputeScope::utc(workspace_id, actor_id);
        let row = single_row(&mut tx, &scope, database_id, row_id).await?;
        tx.commit().await?;
        Ok(ButtonPress {
            row,
            created_rows,
            open_url,
        })
    }
}
