//! Send limits (spec §5.8): the numbers of a workspace's tier and the sliding windows they apply to.
//!
//! A recipient counts (Bcc included) when its message is released (`mail_outbound_recipients.
//! released_at`). Task "release" inserts those rows, so a message that is `held` and already due
//! but not yet released has no rows: the gate counts the recipients of such a message from its
//! draft (`release_at` inside the window, up to `now`), so two due messages cannot both pass on
//! the same free room. A message that is held for later (undo time, scheduled) is not counted
//! now; it is checked again when it is released.

use orbit_platform::{Id, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use sqlx::{Any, AnyConnection, Transaction};

use super::{MailError, MailRepository};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;

/// The numbers of one tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub per_user_hour: i64,
    pub per_user_day: i64,
    pub per_ws_hour: i64,
    pub per_ws_day: i64,
    pub max_recipients: i64,
}

impl Limits {
    pub const NEW: Self = Self {
        per_user_hour: 30,
        per_user_day: 100,
        per_ws_hour: 60,
        per_ws_day: 200,
        max_recipients: 50,
    };
    pub const STANDARD: Self = Self {
        per_user_hour: 500,
        per_user_day: 2_000,
        per_ws_hour: 2_000,
        per_ws_day: 10_000,
        max_recipients: 100,
    };
}

#[derive(Deserialize)]
struct Tiers {
    new: Option<Limits>,
    standard: Option<Limits>,
}

/// The limits of the workspace: its tier's numbers from the instance settings, or its own when the
/// tier is `custom`. A workspace without a policy row is `standard`.
pub(super) async fn load_limits(
    connection: &mut AnyConnection,
    workspace_id: Id,
) -> Result<Limits, MailError> {
    let policy =
        sqlx::query("SELECT tier, custom_limits FROM mail_send_policies WHERE workspace_id = $1")
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *connection)
            .await?;
    let (tier, custom) = match &policy {
        Some(row) => (
            row.try_get::<String, _>("tier")?,
            row.try_get::<Option<String>, _>("custom_limits")?,
        ),
        None => ("standard".to_owned(), None),
    };
    let tiers: Option<String> =
        sqlx::query_scalar("SELECT mail_send_tiers FROM instance_settings WHERE id = 1")
            .fetch_optional(&mut *connection)
            .await?;
    let tiers: Option<Tiers> = tiers.and_then(|json| serde_json::from_str(&json).ok());
    let standard = tiers
        .as_ref()
        .and_then(|tiers| tiers.standard)
        .unwrap_or(Limits::STANDARD);
    Ok(match tier.as_str() {
        "new" => tiers
            .as_ref()
            .and_then(|tiers| tiers.new)
            .unwrap_or(Limits::NEW),
        "custom" => custom
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or(standard),
        _ => standard,
    })
}

/// `(time, recipients)` of what counts in a window, oldest first.
async fn entries(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Option<Id>,
    now: i64,
    exclude: Option<Id>,
) -> Result<Vec<(i64, i64)>, MailError> {
    let since = now - DAY_MS;
    let mut released = orbit_platform::SqlBuilder::new(
        "SELECT released_at FROM mail_outbound_recipients WHERE released_at > ",
    );
    released.push_bind(since);
    released.push(" AND released_at <= ");
    released.push_bind(now);
    released.push(" AND workspace_id = ");
    released.push_bind(workspace_id.to_string());
    if let Some(membership_id) = membership_id {
        released.push(" AND membership_id = ");
        released.push_bind(membership_id.to_string());
    }
    let mut list: Vec<(i64, i64)> = released
        .build()
        .fetch_all(&mut **transaction)
        .await?
        .iter()
        .map(|row| Ok((row.try_get::<i64, _>("released_at")?, 1)))
        .collect::<Result<_, MailError>>()?;
    let mut due = orbit_platform::SqlBuilder::new(
        "SELECT o.id, o.release_at, d.to_addrs, d.cc_addrs, d.bcc_addrs FROM mail_outbound o \
         JOIN mail_drafts d ON d.id = o.draft_id WHERE o.status = 'held' AND o.release_at > ",
    );
    due.push_bind(since);
    due.push(" AND o.release_at <= ");
    due.push_bind(now);
    due.push(" AND o.workspace_id = ");
    due.push_bind(workspace_id.to_string());
    if let Some(membership_id) = membership_id {
        due.push(" AND o.membership_id = ");
        due.push_bind(membership_id.to_string());
    }
    if let Some(exclude) = exclude {
        due.push(" AND o.id <> ");
        due.push_bind(exclude.to_string());
    }
    for row in due.build().fetch_all(&mut **transaction).await? {
        let count: i64 = ["to_addrs", "cc_addrs", "bcc_addrs"]
            .iter()
            .map(|column| {
                let json: String = row.try_get(column)?;
                Ok(serde_json::from_str::<Vec<String>>(&json).map_or(0, |list| list.len() as i64))
            })
            .sum::<Result<i64, MailError>>()?;
        list.push((row.try_get::<i64, _>("release_at")?, count));
    }
    list.sort_unstable();
    Ok(list)
}

/// When `count` more recipients fit under `limit` in a window of `window` ms: `None` now, else
/// the time the oldest entries leave the window. A window that holds nothing allows any message,
/// else one bigger than the limit would never go.
fn next_ok(list: &[(i64, i64)], now: i64, window: i64, limit: i64, count: i64) -> Option<i64> {
    let inside: Vec<&(i64, i64)> = list.iter().filter(|(at, _)| *at > now - window).collect();
    let used: i64 = inside.iter().map(|(_, n)| n).sum();
    if used == 0 || used + count <= limit {
        return None;
    }
    let mut remaining = used;
    for (at, n) in inside {
        remaining -= n;
        if remaining == 0 || remaining + count <= limit {
            return Some(at + window);
        }
    }
    None
}

impl MailRepository {
    /// The limits that apply to the workspace.
    pub async fn limits_for(&self, workspace_id: Id) -> Result<Limits, MailError> {
        let mut connection = self.database.pool().acquire().await?;
        load_limits(&mut connection, workspace_id).await
    }

    /// Whether `count` more recipients fit in the hour and day windows of the member and of the
    /// workspace at `now`: `None` when they do, else the first time they would (the latest of the
    /// four windows). `exclude` is the outbound row being checked (a release checks a row that
    /// is itself held and due).
    pub async fn window_allows(
        &self,
        transaction: &mut Transaction<'_, Any>,
        workspace_id: Id,
        membership_id: Id,
        count: i64,
        now: TimestampMillis,
        exclude: Option<Id>,
    ) -> Result<Option<TimestampMillis>, MailError> {
        let limits = load_limits(transaction, workspace_id).await?;
        let now = now.as_millis();
        let user = entries(transaction, workspace_id, Some(membership_id), now, exclude).await?;
        let workspace = entries(transaction, workspace_id, None, now, exclude).await?;
        let later = [
            next_ok(&user, now, HOUR_MS, limits.per_user_hour, count),
            next_ok(&user, now, DAY_MS, limits.per_user_day, count),
            next_ok(&workspace, now, HOUR_MS, limits.per_ws_hour, count),
            next_ok(&workspace, now, DAY_MS, limits.per_ws_day, count),
        ]
        .into_iter()
        .flatten()
        .max();
        Ok(later.map(TimestampMillis::from_millis))
    }
}
