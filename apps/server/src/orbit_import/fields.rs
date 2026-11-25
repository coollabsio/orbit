//! Custom task fields of a full import: the gallery (before the projects), each project's field
//! list, and every task's values.
//!
//! A source field becomes the field an earlier import of it made, else this workspace's field
//! with the same name (case-insensitive) and kind, whose missing options are appended, else a
//! new field; a name that a field of another kind holds gets a suffix (`Name (2)`). Options map
//! by name (case-insensitive), so values keep pointing at the same choices whatever ids the
//! archive used. The archive is untrusted: at most [`MAX_FIELDS`] fields and [`MAX_OPTIONS`]
//! options each, names trimmed and capped, unknown kinds skipped, and every value checked
//! against its field as the API checks it (a value that fails is left out).

use std::collections::{HashMap, HashSet};

use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;

use super::model;
use super::{OrbitImportService, Run};
use crate::notion::import::clamp;
use crate::repositories::tasks::{
    ImportedOption, MAX_FIELDS, MAX_OPTIONS, TaskError, TaskFieldKind, TaskFieldOption,
    checked_value,
};
use crate::task_import::{Failure, LABEL_COLORS};
use crate::workspace_export::ExportMode;

/// This workspace's side of one source field.
pub(super) struct FieldTarget {
    pub(super) id: Id,
    kind: TaskFieldKind,
    options: Vec<TaskFieldOption>,
    /// Source option id → option id here.
    option_ids: HashMap<String, String>,
}

/// Source field id → its field here.
pub(super) type FieldMap = HashMap<String, FieldTarget>;

fn kind_of(field: &model::TaskField) -> Option<TaskFieldKind> {
    serde_json::from_value(Value::String(field.kind.clone())).ok()
}

fn field_name(field: &model::TaskField) -> String {
    clamp(field.name.trim(), 100, 400).trim().to_owned()
}

/// The source options as the gallery accepts them: trimmed names of 1 to 100 characters,
/// unique without case, `#rrggbb` colors (else a palette color), at most [`MAX_OPTIONS`].
/// Returns them with the source id of each.
fn checked_options(field: &model::TaskField) -> Vec<(String, ImportedOption)> {
    let mut names = HashSet::new();
    let mut options = Vec::new();
    for option in &field.options {
        if options.len() >= MAX_OPTIONS {
            break;
        }
        let name = clamp(option.name.trim(), 100, 400).trim().to_owned();
        if name.is_empty() || !names.insert(name.to_lowercase()) {
            continue;
        }
        let color = option.color.to_ascii_lowercase();
        let color = if color.len() == 7
            && color.starts_with('#')
            && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            color
        } else {
            LABEL_COLORS[options.len() % LABEL_COLORS.len()].to_owned()
        };
        options.push((option.id.clone(), ImportedOption { name, color }));
    }
    options
}

/// Imports the gallery of a full import that selected a project. Runs on every attempt; each
/// field is linked, so a later attempt finds it again.
pub(super) async fn import_fields(
    service: &OrbitImportService,
    run: &Run,
    workspace: &model::Workspace,
) -> Result<(), Failure> {
    if run.mode != ExportMode::Full || run.selection.project_ids.is_empty() {
        return Ok(());
    }
    let mut seen = HashSet::new();
    for field in workspace.task_fields.iter().take(MAX_FIELDS as usize) {
        if field.id.is_empty() || !seen.insert(field.id.as_str()) {
            continue;
        }
        let (Some(kind), name) = (kind_of(field), field_name(field)) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let options: Vec<ImportedOption> = checked_options(field)
            .into_iter()
            .map(|(_, option)| option)
            .collect();
        let linked = service.linked(run, "task_field", &field.id).await?;
        let imported = service
            .inner
            .tasks
            .import_task_field(
                run.workspace_id,
                run.user_id,
                linked,
                &name,
                kind,
                &options,
                &run.request_id,
                TimestampMillis::now(),
            )
            .await;
        match imported {
            Ok(Some(record)) => {
                service
                    .record_link(run, "task_field", &field.id, record.id)
                    .await?;
            }
            Ok(None) => {
                tracing::warn!(import_id = %run.import_id, "Orbit import: task field left out (gallery full or name taken)");
            }
            Err(TaskError::Unavailable(error)) => return Err(Failure::from(error)),
            Err(error) => {
                tracing::warn!(import_id = %run.import_id, error = %error, "Orbit import: task field refused");
            }
        }
    }
    Ok(())
}

/// The imported fields, from the links and this workspace's gallery.
pub(super) async fn field_map(
    service: &OrbitImportService,
    run: &Run,
    workspace: &model::Workspace,
) -> Result<FieldMap, Failure> {
    let mut map = FieldMap::new();
    if run.mode != ExportMode::Full {
        return Ok(map);
    }
    let gallery: HashMap<Id, (TaskFieldKind, Vec<TaskFieldOption>)> = service
        .inner
        .tasks
        .task_fields(run.workspace_id, run.user_id)
        .await?
        .into_iter()
        .map(|field| (field.id, (field.kind, field.options)))
        .collect();
    for field in workspace.task_fields.iter().take(MAX_FIELDS as usize) {
        let Some(id) = service.linked(run, "task_field", &field.id).await? else {
            continue;
        };
        let Some((kind, options)) = gallery.get(&id) else {
            continue;
        };
        let by_name: HashMap<String, &str> = options
            .iter()
            .map(|option| (option.name.to_lowercase(), option.id.as_str()))
            .collect();
        let option_ids = checked_options(field)
            .into_iter()
            .filter_map(|(source, option)| {
                by_name
                    .get(&option.name.to_lowercase())
                    .map(|id| (source, (*id).to_owned()))
            })
            .collect();
        map.insert(
            field.id.clone(),
            FieldTarget {
                id,
                kind: *kind,
                options: options.clone(),
                option_ids,
            },
        );
    }
    Ok(map)
}

/// Appends the project's source fields to the fields the project shows here.
pub(super) async fn import_project_fields(
    service: &OrbitImportService,
    run: &Run,
    project: &model::Project,
    project_id: Id,
    fields: &FieldMap,
) -> Result<(), Failure> {
    let ids: Vec<Id> = project
        .fields
        .iter()
        .filter_map(|source| fields.get(source).map(|target| target.id))
        .collect();
    service
        .inner
        .tasks
        .import_project_task_fields(run.workspace_id, project_id, &ids)
        .await?;
    Ok(())
}

/// The value to store here for a source value, `None` to leave it out.
fn mapped_value(target: &FieldTarget, value: &Value) -> Option<Value> {
    let option = |id: &Value| {
        id.as_str()
            .and_then(|id| target.option_ids.get(id))
            .map(|id| Value::String(id.clone()))
    };
    let value = match (target.kind, value) {
        (TaskFieldKind::Select, id) => option(id)?,
        (TaskFieldKind::MultiSelect, Value::Array(ids)) => {
            Value::Array(ids.iter().take(MAX_OPTIONS).filter_map(option).collect())
        }
        (_, value) => value.clone(),
    };
    checked_value(target.kind, &target.options, value)
        .ok()
        .flatten()
}

/// Stores the task's values of imported fields; values it already has stay.
pub(super) async fn import_values(
    service: &OrbitImportService,
    run: &Run,
    task_id: Id,
    task: &model::Task,
    fields: &FieldMap,
) -> Result<(), Failure> {
    let mut seen = HashSet::new();
    let values: Vec<(Id, Value)> = task
        .field_values
        .iter()
        .take(MAX_FIELDS as usize)
        .filter_map(|value| {
            let target = fields.get(&value.field_id)?;
            let mapped = mapped_value(target, &value.value)?;
            seen.insert(target.id).then_some((target.id, mapped))
        })
        .collect();
    service
        .inner
        .tasks
        .import_task_field_values(run.workspace_id, task_id, &values, TimestampMillis::now())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn target(kind: TaskFieldKind) -> FieldTarget {
        FieldTarget {
            id: Id::new_v7(),
            kind,
            options: vec![
                TaskFieldOption {
                    id: "new-a".into(),
                    name: "A".into(),
                    color: "#ff0000".into(),
                },
                TaskFieldOption {
                    id: "new-b".into(),
                    name: "B".into(),
                    color: "#00ff00".into(),
                },
            ],
            option_ids: HashMap::from([
                ("old-a".to_owned(), "new-a".to_owned()),
                ("old-b".to_owned(), "new-b".to_owned()),
            ]),
        }
    }

    #[test]
    fn maps_option_ids_and_checks_values() {
        let select = target(TaskFieldKind::Select);
        assert_eq!(mapped_value(&select, &json!("old-b")), Some(json!("new-b")));
        assert_eq!(mapped_value(&select, &json!("forged")), None);
        assert_eq!(mapped_value(&select, &json!(3)), None);
        let multi = target(TaskFieldKind::MultiSelect);
        assert_eq!(
            mapped_value(&multi, &json!(["old-a", "gone", "old-b"])),
            Some(json!(["new-a", "new-b"]))
        );
        assert_eq!(mapped_value(&multi, &json!(["gone"])), None);
        assert_eq!(mapped_value(&multi, &json!("old-a")), None);
        let date = target(TaskFieldKind::Date);
        assert_eq!(mapped_value(&date, &json!("2026-02-30")), None);
        assert_eq!(
            mapped_value(&date, &json!("2026-02-28")),
            Some(json!("2026-02-28"))
        );
        let url = target(TaskFieldKind::Url);
        assert_eq!(mapped_value(&url, &json!("javascript:alert(1)")), None);
        let number = target(TaskFieldKind::Number);
        assert_eq!(mapped_value(&number, &json!(1e300)), None);
    }

    #[test]
    fn cleans_untrusted_options() {
        let field: model::TaskField = serde_json::from_value(json!({
            "id": "f", "name": "  Stage  ", "kind": "select",
            "options": [
                {"id": "1", "name": " Open ", "color": "#ABCDEF"},
                {"id": "2", "name": "open", "color": "#000000"},
                {"id": "3", "name": "   ", "color": "#000000"},
                {"id": "4", "name": "Done", "color": "red"},
                "not an option",
            ],
        }))
        .unwrap();
        assert_eq!(field_name(&field), "Stage");
        let options = checked_options(&field);
        let names: Vec<(&str, &str, &str)> = options
            .iter()
            .map(|(id, option)| (id.as_str(), option.name.as_str(), option.color.as_str()))
            .collect();
        assert_eq!(
            names,
            [("1", "Open", "#abcdef"), ("4", "Done", LABEL_COLORS[1])]
        );
        let many: model::TaskField = serde_json::from_value(json!({
            "id": "f", "name": "Many", "kind": "multi_select",
            "options": (0..500).map(|n| json!({"id": n.to_string(), "name": format!("O{n}"), "color": "#000000"})).collect::<Vec<_>>(),
        }))
        .unwrap();
        assert_eq!(checked_options(&many).len(), MAX_OPTIONS);
        let unknown: model::TaskField =
            serde_json::from_value(json!({"id": "f", "name": "X", "kind": "formula"})).unwrap();
        assert!(kind_of(&unknown).is_none());
    }
}
