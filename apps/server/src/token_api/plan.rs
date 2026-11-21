//! Milestone and cycle writes, and status and label deletes.
use orbit_platform::{Id, TimestampMillis};

use super::{Authorized, CycleView, Result, TokenApiError, cycle_views, valid_name};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::tasks::{
    CycleChanges, CycleRecord, MilestoneChanges, MilestoneRecord, TaskError,
};

const DAY_MS: i64 = 86_400_000;

/// Noon UTC, where the web app stores a milestone's target day.
pub const NOON_MS: i64 = 12 * 3_600_000;

/// A milestone or cycle date: an RFC 3339 time, or a `YYYY-MM-DD` day at `day_offset_ms` after
/// its UTC midnight. `None` for any other text.
pub fn parse_plan_time(raw: &str, day_offset_ms: i64) -> Option<TimestampMillis> {
    if let Ok(day) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let midnight = day
            .and_time(chrono::NaiveTime::MIN)
            .and_utc()
            .timestamp_millis();
        return Some(TimestampMillis::from_millis(midnight + day_offset_ms));
    }
    serde_json::from_value(serde_json::Value::String(raw.to_owned())).ok()
}

impl Authorized<'_> {
    /// Adds a milestone after the approved project's others.
    pub async fn create_milestone(
        &self,
        project_id: Id,
        name: &str,
        description: Option<String>,
        target_at: Option<TimestampMillis>,
    ) -> Result<MilestoneRecord> {
        self.plan_project(project_id)?;
        let name = valid_name(name, 200, "A milestone name has 1 to 200 characters")?;
        let description = plan_description(description)?;
        let p = self.principal;
        self.granted()
            .create_milestone(
                p.workspace_id,
                project_id,
                p.actor_id,
                name,
                description,
                target_at,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(milestone_error)
    }

    pub async fn update_milestone(
        &self,
        project_id: Id,
        milestone_id: Id,
        expected_version: u64,
        changes: MilestoneChanges,
    ) -> Result<MilestoneRecord> {
        self.plan_project(project_id)?;
        let changes = MilestoneChanges {
            name: changes
                .name
                .map(|name| valid_name(&name, 200, "A milestone name has 1 to 200 characters"))
                .transpose()?,
            description: changes.description.map(plan_description).transpose()?,
            ..changes
        };
        let p = self.principal;
        self.granted()
            .update_milestone(
                p.workspace_id,
                project_id,
                milestone_id,
                p.actor_id,
                changes,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(milestone_error)
    }

    /// Deletes a milestone; its tasks stay, without a milestone.
    pub async fn delete_milestone(
        &self,
        project_id: Id,
        milestone_id: Id,
        expected_version: u64,
    ) -> Result<()> {
        self.plan_project(project_id)?;
        let p = self.principal;
        Ok(self
            .granted()
            .delete_milestone(
                p.workspace_id,
                project_id,
                milestone_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Adds the approved project's next cycle. Missing dates follow the web app: the cycle starts
    /// when the last one ends and lasts as many days, or (the first cycle) starts at today's UTC
    /// midnight and lasts two weeks; a given `starts_at` alone keeps that length.
    pub async fn create_cycle(
        &self,
        project_id: Id,
        name: Option<String>,
        starts_at: Option<TimestampMillis>,
        ends_at: Option<TimestampMillis>,
    ) -> Result<CycleView> {
        self.plan_project(project_id)?;
        let name = cycle_name(name)?;
        let p = self.principal;
        let now = TimestampMillis::now();
        let (starts_at, ends_at) = match (starts_at, ends_at) {
            (Some(starts_at), Some(ends_at)) => (starts_at, ends_at),
            (starts_at, ends_at) => {
                let cycles = self
                    .api
                    .tasks
                    .cycles(p.workspace_id, project_id, p.actor_id)
                    .await?;
                let (default_start, length) = next_cycle_span(&cycles, now);
                let starts_at = starts_at.unwrap_or(default_start);
                let ends_at = ends_at.unwrap_or_else(|| {
                    TimestampMillis::from_millis(starts_at.as_millis() + length)
                });
                (starts_at, ends_at)
            }
        };
        let created = self
            .granted()
            .create_cycle(
                p.workspace_id,
                project_id,
                p.actor_id,
                name,
                starts_at,
                ends_at,
                self.request_id,
                now,
            )
            .await
            .map_err(cycle_error)?;
        self.cycle_view(project_id, created).await
    }

    pub async fn update_cycle(
        &self,
        project_id: Id,
        cycle_id: Id,
        expected_version: u64,
        changes: CycleChanges,
    ) -> Result<CycleView> {
        self.plan_project(project_id)?;
        let changes = CycleChanges {
            name: changes.name.map(cycle_name).transpose()?,
            ..changes
        };
        let p = self.principal;
        let updated = self
            .granted()
            .update_cycle(
                p.workspace_id,
                project_id,
                cycle_id,
                p.actor_id,
                changes,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(cycle_error)?;
        self.cycle_view(project_id, updated).await
    }

    /// Deletes a cycle; its tasks stay, without a cycle.
    pub async fn delete_cycle(
        &self,
        project_id: Id,
        cycle_id: Id,
        expected_version: u64,
    ) -> Result<()> {
        self.plan_project(project_id)?;
        let p = self.principal;
        Ok(self
            .granted()
            .delete_cycle(
                p.workspace_id,
                project_id,
                cycle_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    /// Deletes an unused status of an approved project. Needs a member who may manage the
    /// project; the Duplicate status stays.
    pub async fn delete_status(
        &self,
        project_id: Id,
        status_id: Id,
        expected_version: u64,
    ) -> Result<()> {
        self.plan_project(project_id)?;
        let p = self.principal;
        self.granted()
            .delete_status(
                p.workspace_id,
                project_id,
                status_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(|error| match error {
                TaskError::Invalid { field: "status_id" } => {
                    TokenApiError::Invalid("The Duplicate status cannot be deleted")
                }
                TaskError::Conflict => TokenApiError::Conflict(
                    "Tasks use this status, in the trash too: move them to another status first",
                ),
                error => manage_forbidden(error),
            })
    }

    /// Deletes a label of an approved project and takes it off its tasks. Needs a member who may
    /// manage the project.
    pub async fn delete_label(
        &self,
        project_id: Id,
        label_id: Id,
        expected_version: u64,
    ) -> Result<()> {
        self.plan_project(project_id)?;
        let p = self.principal;
        self.granted()
            .delete_label(
                p.workspace_id,
                project_id,
                label_id,
                p.actor_id,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await
            .map_err(manage_forbidden)
    }

    /// A task write in an approved project.
    fn plan_project(&self, project_id: Id) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        if self.approved(project_id) {
            Ok(())
        } else {
            Err(TokenApiError::NotAvailable)
        }
    }

    /// The cycle as `list_cycles` shows it, with its `relative` place among the project's cycles.
    async fn cycle_view(&self, project_id: Id, cycle: CycleRecord) -> Result<CycleView> {
        let p = self.principal;
        let cycles = self
            .api
            .tasks
            .cycles(p.workspace_id, project_id, p.actor_id)
            .await?;
        Ok(cycle_views(cycles, TimestampMillis::now())
            .into_iter()
            .find(|view| view.cycle.id == cycle.id)
            .unwrap_or(CycleView {
                cycle,
                relative: None,
            }))
    }
}

/// The web app's default span for a new cycle, as a start and a length in milliseconds.
fn next_cycle_span(cycles: &[CycleRecord], now: TimestampMillis) -> (TimestampMillis, i64) {
    match cycles.iter().max_by_key(|cycle| cycle.ends_at.as_millis()) {
        Some(last) => {
            let days = ((last.ends_at.as_millis() - last.starts_at.as_millis()) as f64
                / DAY_MS as f64)
                .round()
                .max(1.0) as i64;
            (last.ends_at, days * DAY_MS)
        }
        None => {
            let today = now.as_millis().div_euclid(DAY_MS) * DAY_MS;
            (TimestampMillis::from_millis(today), 14 * DAY_MS)
        }
    }
}

/// A blank description is none; otherwise at most 20,000 characters.
fn plan_description(value: Option<String>) -> Result<Option<String>> {
    match value {
        Some(value) if value.trim().is_empty() => Ok(None),
        Some(value) if value.chars().count() > 20_000 || value.len() > 20_000 => Err(
            TokenApiError::Invalid("A description has at most 20,000 characters"),
        ),
        value => Ok(value),
    }
}

/// An optional cycle name, trimmed: blank is none.
fn cycle_name(value: Option<String>) -> Result<Option<String>> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| valid_name(&value, 200, "A cycle name has at most 200 characters"))
        .transpose()
}

fn milestone_error(error: TaskError) -> TokenApiError {
    match error {
        TaskError::Conflict => {
            TokenApiError::Conflict("The project already has a milestone with this name")
        }
        error => error.into(),
    }
}

fn cycle_error(error: TaskError) -> TokenApiError {
    match error {
        TaskError::CycleOverlap => TokenApiError::Conflict(
            "The dates overlap another cycle of the project (see list_cycles); cycles may touch",
        ),
        TaskError::Invalid { field: "ends_at" } => {
            TokenApiError::Invalid("ends_at must be after starts_at")
        }
        error => error.into(),
    }
}

/// The project is already approved, so a refusal is the manage permission.
pub(super) fn manage_forbidden(error: TaskError) -> TokenApiError {
    match error {
        TaskError::Forbidden => TokenApiError::Forbidden(
            "Deleting statuses and labels needs a member who may manage the project",
        ),
        error => error.into(),
    }
}
