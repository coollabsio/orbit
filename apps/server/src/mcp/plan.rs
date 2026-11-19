//! Milestone and cycle writes, and status and label deletes.
use orbit_platform::TimestampMillis;
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{arguments, parse_id, present, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{CycleChanges, MilestoneChanges};
use crate::token_api::{Authorized, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateMilestoneArgs {
    project_id: String,
    /// 1 to 200 characters, unique in the project.
    name: String,
    /// Up to 20,000 characters.
    description: Option<String>,
    /// The target date: `YYYY-MM-DD` (stored at noon UTC, as the web app does) or an RFC 3339
    /// time such as `2026-12-01T12:00:00Z`.
    target_at: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateMilestoneArgs {
    project_id: String,
    milestone_id: String,
    /// The milestone's version, from list_milestones.
    expected_version: u64,
    /// 1 to 200 characters, unique in the project.
    name: Option<String>,
    /// Up to 20,000 characters; `null` or blank clears it.
    #[serde(default, deserialize_with = "present")]
    description: Option<Option<String>>,
    /// `YYYY-MM-DD` (noon UTC) or an RFC 3339 time; `null` clears it.
    #[serde(default, deserialize_with = "present")]
    target_at: Option<Option<String>>,
    /// The milestone's place in the project's order; list_milestones shows the others'.
    position: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteMilestoneArgs {
    project_id: String,
    milestone_id: String,
    /// The milestone's version, from list_milestones.
    expected_version: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateCycleArgs {
    project_id: String,
    /// Up to 200 characters. Without one, the cycle shows as its number.
    name: Option<String>,
    /// The first instant: an RFC 3339 time such as `2026-11-23T00:00:00Z`, or `YYYY-MM-DD` for
    /// midnight UTC. Default: when the project's last cycle ends, or today's UTC midnight for the
    /// first cycle.
    starts_at: Option<String>,
    /// The end, exclusive (`YYYY-MM-DD` is midnight UTC, so the day before is the last one).
    /// Default: as many days after `starts_at` as the last cycle lasted, or 14 for the first.
    /// Cycles of a project cannot overlap; they may touch.
    ends_at: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateCycleArgs {
    project_id: String,
    cycle_id: String,
    /// The cycle's version, from list_cycles.
    expected_version: u64,
    /// Up to 200 characters; `null` or blank clears it, and the cycle shows as its number.
    #[serde(default, deserialize_with = "present")]
    name: Option<Option<String>>,
    /// An RFC 3339 time, or `YYYY-MM-DD` for midnight UTC.
    starts_at: Option<String>,
    /// The end, exclusive: an RFC 3339 time, or `YYYY-MM-DD` for midnight UTC.
    ends_at: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteCycleArgs {
    project_id: String,
    cycle_id: String,
    /// The cycle's version, from list_cycles.
    expected_version: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteStatusArgs {
    project_id: String,
    status_id: String,
    /// The status's version, from list_task_statuses.
    expected_version: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteLabelArgs {
    project_id: String,
    label_id: String,
    /// The label's version, from list_labels.
    expected_version: u64,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "create_milestone" | "update_milestone" | "delete_milestone" | "create_cycle"
        | "update_cycle" | "delete_cycle" | "delete_task_status" | "delete_label" => {
            Some(ApiTokenScope::TasksWrite)
        }
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        write_definition::<CreateMilestoneArgs>(
            "create_milestone",
            "Add a milestone after the others of an approved project. Returns it as list_milestones does.",
            false,
        ),
        write_definition::<UpdateMilestoneArgs>(
            "update_milestone",
            "Change a milestone of an approved project: name, description, target date or position. Missing fields stay. Returns it as list_milestones does.",
            false,
        ),
        write_definition::<DeleteMilestoneArgs>(
            "delete_milestone",
            "Delete a milestone of an approved project. Its tasks stay, without a milestone.",
            true,
        ),
        write_definition::<CreateCycleArgs>(
            "create_cycle",
            "Add the next-numbered cycle to an approved project. Missing dates continue after the last cycle with its length, as the web app does. Returns it as list_cycles does.",
            false,
        ),
        write_definition::<UpdateCycleArgs>(
            "update_cycle",
            "Rename a cycle of an approved project or move its dates. Missing fields stay. Returns it as list_cycles does.",
            false,
        ),
        write_definition::<DeleteCycleArgs>(
            "delete_cycle",
            "Delete a cycle of an approved project. Its tasks stay, without a cycle.",
            true,
        ),
        write_definition::<DeleteStatusArgs>(
            "delete_task_status",
            "Delete a task status of an approved project. No task may use it (trashed tasks count); the Duplicate status cannot be deleted. Needs a member who may manage the project.",
            true,
        ),
        write_definition::<DeleteLabelArgs>(
            "delete_label",
            "Delete a label of an approved project and take it off its tasks. Needs a member who may manage the project.",
            true,
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "create_milestone" => {
            let run = async {
                let a: CreateMilestoneArgs = arguments(request)?;
                let target_at = a
                    .target_at
                    .as_deref()
                    .map(|raw| parse_time(raw, "target_at", NOON_MS))
                    .transpose()?;
                Ok(api
                    .create_milestone(parse_id(&a.project_id)?, &a.name, a.description, target_at)
                    .await
                    .map(|milestone| json!(milestone)))
            };
            run.await
        }
        "update_milestone" => {
            let run = async {
                let a: UpdateMilestoneArgs = arguments(request)?;
                let changes = MilestoneChanges {
                    name: a.name,
                    description: a.description,
                    target_at: a
                        .target_at
                        .map(|value| {
                            value
                                .as_deref()
                                .map(|raw| parse_time(raw, "target_at", NOON_MS))
                                .transpose()
                        })
                        .transpose()?,
                    position: a.position,
                };
                Ok(api
                    .update_milestone(
                        parse_id(&a.project_id)?,
                        parse_id(&a.milestone_id)?,
                        a.expected_version,
                        changes,
                    )
                    .await
                    .map(|milestone| json!(milestone)))
            };
            run.await
        }
        "delete_milestone" => {
            let run = async {
                let a: DeleteMilestoneArgs = arguments(request)?;
                Ok(api
                    .delete_milestone(
                        parse_id(&a.project_id)?,
                        parse_id(&a.milestone_id)?,
                        a.expected_version,
                    )
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "create_cycle" => {
            let run = async {
                let a: CreateCycleArgs = arguments(request)?;
                let starts_at = a
                    .starts_at
                    .as_deref()
                    .map(|raw| parse_time(raw, "starts_at", 0))
                    .transpose()?;
                let ends_at = a
                    .ends_at
                    .as_deref()
                    .map(|raw| parse_time(raw, "ends_at", 0))
                    .transpose()?;
                Ok(api
                    .create_cycle(parse_id(&a.project_id)?, a.name, starts_at, ends_at)
                    .await
                    .map(|cycle| json!(cycle)))
            };
            run.await
        }
        "update_cycle" => {
            let run = async {
                let a: UpdateCycleArgs = arguments(request)?;
                let changes = CycleChanges {
                    name: a.name,
                    starts_at: a
                        .starts_at
                        .as_deref()
                        .map(|raw| parse_time(raw, "starts_at", 0))
                        .transpose()?,
                    ends_at: a
                        .ends_at
                        .as_deref()
                        .map(|raw| parse_time(raw, "ends_at", 0))
                        .transpose()?,
                };
                Ok(api
                    .update_cycle(
                        parse_id(&a.project_id)?,
                        parse_id(&a.cycle_id)?,
                        a.expected_version,
                        changes,
                    )
                    .await
                    .map(|cycle| json!(cycle)))
            };
            run.await
        }
        "delete_cycle" => {
            let run = async {
                let a: DeleteCycleArgs = arguments(request)?;
                Ok(api
                    .delete_cycle(
                        parse_id(&a.project_id)?,
                        parse_id(&a.cycle_id)?,
                        a.expected_version,
                    )
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "delete_task_status" => {
            let run = async {
                let a: DeleteStatusArgs = arguments(request)?;
                Ok(api
                    .delete_status(
                        parse_id(&a.project_id)?,
                        parse_id(&a.status_id)?,
                        a.expected_version,
                    )
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        "delete_label" => {
            let run = async {
                let a: DeleteLabelArgs = arguments(request)?;
                Ok(api
                    .delete_label(
                        parse_id(&a.project_id)?,
                        parse_id(&a.label_id)?,
                        a.expected_version,
                    )
                    .await
                    .map(|()| json!({ "deleted": true })))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}

/// Noon UTC, where the web app stores a milestone's target day.
const NOON_MS: i64 = 12 * 3_600_000;

/// An RFC 3339 time, or a `YYYY-MM-DD` day at `day_offset_ms` after its UTC midnight.
fn parse_time(raw: &str, field: &str, day_offset_ms: i64) -> Result<TimestampMillis, ErrorData> {
    let invalid = || {
        ErrorData::invalid_params(
            format!("{field} must be YYYY-MM-DD or an RFC 3339 time such as 2026-11-23T00:00:00Z"),
            None,
        )
    };
    if let Ok(day) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let midnight = day
            .and_time(chrono::NaiveTime::MIN)
            .and_utc()
            .timestamp_millis();
        return Ok(TimestampMillis::from_millis(midnight + day_offset_ms));
    }
    serde_json::from_value(Value::String(raw.to_owned())).map_err(|_| invalid())
}
