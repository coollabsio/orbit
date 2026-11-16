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
/// tier is `custom`. A workspace without a policy row is `standard`. Numbers that cannot be read
/// fail closed to the `new` tier.
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
    if tier == "custom" {
        return Ok(
            match custom.as_deref().map(serde_json::from_str::<Limits>) {
                Some(Ok(limits)) => limits,
                _ => {
                    tracing::warn!(%workspace_id, "custom mail send limits are missing or malformed");
                    Limits::NEW
                }
            },
        );
    }
    let tiers: Option<String> =
        sqlx::query_scalar("SELECT mail_send_tiers FROM instance_settings WHERE id = 1")
            .fetch_optional(&mut *connection)
            .await?;
    let Some(Ok(tiers)) = tiers.as_deref().map(serde_json::from_str::<Tiers>) else {
        tracing::warn!("mail_send_tiers is missing or malformed");
        return Ok(Limits::NEW);
    };
    Ok(if tier == "new" {
        tiers.new.unwrap_or(Limits::NEW)
    } else {
        tiers.standard.unwrap_or(Limits::NEW)
    })
}

/// The recipients of due, unreleased messages in the window (they have no recipient rows yet),
/// counted like Send: distinct normalized addresses of each message.
async fn due_held(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Option<Id>,
    since: i64,
    now: i64,
    exclude: Option<Id>,
) -> Result<Vec<(i64, i64)>, MailError> {
    let mut due = orbit_platform::SqlBuilder::new(
        "SELECT o.release_at, d.to_addrs, d.cc_addrs, d.bcc_addrs FROM mail_outbound o \
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
    let mut list = Vec::new();
    for row in due.build().fetch_all(&mut **transaction).await? {
        let mut distinct = std::collections::BTreeSet::new();
        for column in ["to_addrs", "cc_addrs", "bcc_addrs"] {
            let json: String = row.try_get(column)?;
            for address in serde_json::from_str::<Vec<String>>(&json).unwrap_or_default() {
                match orbit_mail::normalize_address(address.trim()) {
                    Ok(address) => distinct.insert(address.to_string()),
                    Err(_) => distinct.insert(address),
                };
            }
        }
        list.push((row.try_get::<i64, _>("release_at")?, distinct.len() as i64));
    }
    Ok(list)
}

/// The filter of the released recipients of a scope in a window, without those of `exclude`.
fn released_filter(
    head: &str,
    workspace_id: Id,
    membership_id: Option<Id>,
    since: i64,
    now: i64,
    exclude: Option<Id>,
) -> orbit_platform::SqlBuilder {
    let mut sql = orbit_platform::SqlBuilder::new(head);
    sql.push_bind(since);
    sql.push(" AND released_at <= ");
    sql.push_bind(now);
    sql.push(" AND workspace_id = ");
    sql.push_bind(workspace_id.to_string());
    if let Some(membership_id) = membership_id {
        sql.push(" AND membership_id = ");
        sql.push_bind(membership_id.to_string());
    }
    if let Some(exclude) = exclude {
        sql.push(" AND outbound_id <> ");
        sql.push_bind(exclude.to_string());
    }
    sql
}

/// The first time `count` more recipients fit in one window of one scope: `None` when they fit
/// now. The count is aggregated in SQL; the individual times are only read when the window is
/// over its limit.
#[allow(clippy::too_many_arguments)]
async fn scope_allows(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Option<Id>,
    window: i64,
    limit: i64,
    count: i64,
    now: i64,
    exclude: Option<Id>,
) -> Result<Option<i64>, MailError> {
    let since = now - window;
    let held = due_held(
        transaction,
        workspace_id,
        membership_id,
        since,
        now,
        exclude,
    )
    .await?;
    let mut counted = released_filter(
        "SELECT COUNT(*) AS n FROM mail_outbound_recipients WHERE released_at > ",
        workspace_id,
        membership_id,
        since,
        now,
        exclude,
    );
    let released: i64 = counted
        .build()
        .fetch_one(&mut **transaction)
        .await?
        .try_get("n")?;
    let used = released + held.iter().map(|(_, n)| n).sum::<i64>();
    if used == 0 || used + count <= limit {
        return Ok(None);
    }
    let mut list = held;
    let mut times = released_filter(
        "SELECT released_at FROM mail_outbound_recipients WHERE released_at > ",
        workspace_id,
        membership_id,
        since,
        now,
        exclude,
    );
    let rows = times.build().fetch_all(&mut **transaction).await?;
    for row in rows {
        list.push((row.try_get::<i64, _>("released_at")?, 1));
    }
    list.sort_unstable();
    Ok(next_ok(&list, now, window, limit, count))
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
    /// four windows). `exclude` is the outbound row being checked: neither its draft (a held,
    /// due row) nor its own recipient rows (a released row that delivery checks) count.
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
        let mut later = None;
        for (scope, window, limit) in [
            (Some(membership_id), HOUR_MS, limits.per_user_hour),
            (Some(membership_id), DAY_MS, limits.per_user_day),
            (None, HOUR_MS, limits.per_ws_hour),
            (None, DAY_MS, limits.per_ws_day),
        ] {
            let next = scope_allows(
                transaction,
                workspace_id,
                scope,
                window,
                limit,
                count,
                now,
                exclude,
            )
            .await?;
            later = later.max(next);
        }
        Ok(later.map(TimestampMillis::from_millis))
    }
}
