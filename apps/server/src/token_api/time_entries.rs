//! Time logged on tasks of the token's approved projects.
use orbit_platform::{Id, TimestampMillis};

use super::{Authorized, Result, TokenApiError};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::time_entries::{
    NewTimeEntry, TaskTimeEntries, TimeEntryChanges, TimeEntryRecord,
};

impl Authorized<'_> {
    /// The time logged on a task, with its totals.
    pub async fn time_entries(&self, task_ref: &str) -> Result<TaskTimeEntries> {
        self.need(ApiTokenScope::TasksRead)?;
        let task = self.task(task_ref).await?;
        let p = self.principal;
        Ok(self
            .granted()
            .time_entries(p.workspace_id, task.id, p.actor_id)
            .await?)
    }

    /// Logs time on a task. A service account is nobody's time: its token must name the member.
    pub async fn log_time(&self, task_ref: &str, entry: NewTimeEntry) -> Result<TimeEntryRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        if self.principal.service_account.is_some() && entry.user_id.is_none() {
            return Err(TokenApiError::Invalid(
                "A service-account token must name the member in user_id",
            ));
        }
        let task = self.task(task_ref).await?;
        let p = self.principal;
        Ok(self
            .granted()
            .create_time_entry(
                p.workspace_id,
                task.id,
                p.actor_id,
                entry,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    pub async fn update_time_entry(
        &self,
        entry_id: Id,
        changes: TimeEntryChanges,
        expected_version: Option<u64>,
    ) -> Result<TimeEntryRecord> {
        self.need(ApiTokenScope::TasksWrite)?;
        let p = self.principal;
        Ok(self
            .granted()
            .update_time_entry(
                p.workspace_id,
                entry_id,
                p.actor_id,
                changes,
                expected_version,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }

    pub async fn delete_time_entry(&self, entry_id: Id) -> Result<()> {
        self.need(ApiTokenScope::TasksWrite)?;
        let p = self.principal;
        Ok(self
            .granted()
            .delete_time_entry(
                p.workspace_id,
                entry_id,
                p.actor_id,
                self.request_id,
                TimestampMillis::now(),
            )
            .await?)
    }
}
