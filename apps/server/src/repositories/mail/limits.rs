//! Send limits (spec §5.8): the numbers of a workspace's tier and the sliding windows they apply to.
//!
//! The limits and the pause cover only mail through a shared route: an instance route (the
//! default route of every workspace without its own) or a Direct route (the server's own IP).
//! A workspace's own relay or API provider enforces its own limits, so Orbit adds none.
//!
//! A recipient counts (Bcc included) when its message is released (`mail_outbound_recipients.
//! released_at`). Task "release" inserts those rows, so a message that is `held` and already due
//! but not yet released has no rows: the gate counts the recipients of such a message from its
//! draft (`release_at` inside the window, up to `now`), so two due messages cannot both pass on
//! the same free room. A message that is held for later (undo time, scheduled) is not counted
//! now; it is checked again when it is released.

use orbit_platform::{Id, JobKind, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use sqlx::{Any, AnyConnection, Transaction};
use utoipa::ToSchema;

use super::domains::{manager, parse_id};
use super::{MailError, MailRepository};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
const WEEK_MS: i64 = 7 * DAY_MS;
/// The recurring job that promotes `new` workspaces (spec §5.8), daily.
pub const SEND_POLICY_JOB: &str = "mail.send_policy";
/// A `new` workspace is promoted when its first release is this old.
const PROMOTE_AFTER_MS: i64 = 14 * DAY_MS;
/// Recipients in the last 7 days needed before the rates can pause a workspace.
const PAUSE_MIN_RECIPIENTS: i64 = 100;
/// Rates are parts per 10,000 of the released recipients (a hundredth of a percent).
const PAUSE_HARD_BP: i64 = 500;
const PAUSE_COMPLAINT_BP: i64 = 30;
const PROMOTE_HARD_BP: i64 = 200;
const PROMOTE_COMPLAINT_BP: i64 = 10;
const ADMIN_LIST_MAX: i64 = 500;

/// Whether the route in `column` is shared, as SQL. A message without a route counts as shared.
pub(super) fn shared_route(column: &str) -> String {
    format!(
        "({column} IS NULL OR {column} IN (SELECT id FROM mail_send_routes \
         WHERE workspace_id IS NULL OR kind = 'direct'))"
    )
}

/// Whether the limits and the pause cover a message through `route_id`: it is shared (see the
/// module docs), or unknown.
pub(super) async fn route_limited(
    connection: &mut AnyConnection,
    route_id: Option<Id>,
) -> Result<bool, MailError> {
    let Some(route_id) = route_id else {
        return Ok(true);
    };
    let own: Option<String> = sqlx::query_scalar(
        "SELECT id FROM mail_send_routes WHERE id = $1 AND workspace_id IS NOT NULL \
         AND kind <> 'direct'",
    )
    .bind(route_id.to_string())
    .fetch_optional(&mut *connection)
    .await?;
    Ok(own.is_none())
}

/// Whether sending through shared routes is paused for the workspace.
pub(super) async fn paused(
    connection: &mut AnyConnection,
    workspace_id: Id,
) -> Result<bool, MailError> {
    let paused: Option<i64> =
        sqlx::query_scalar("SELECT paused_at FROM mail_send_policies WHERE workspace_id = $1")
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *connection)
            .await?
            .flatten();
    Ok(paused.is_some())
}

/// `mail.send_policy`: one run at a time on the cluster.
#[must_use]
pub fn send_policy_kind() -> JobKind {
    JobKind::new(SEND_POLICY_JOB).with_concurrency_limit(1)
}

/// The tier a workspace without a policy row has: `new` on Orbit Cloud, else `unlimited`.
pub(super) const fn default_tier(cloud: bool) -> &'static str {
    if cloud { "new" } else { "unlimited" }
}

/// A send tier (spec §5.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SendTier {
    New,
    Standard,
    Custom,
    Unlimited,
}

impl SendTier {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Standard => "standard",
            Self::Custom => "custom",
            Self::Unlimited => "unlimited",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "new" => Some(Self::New),
            "standard" => Some(Self::Standard),
            "custom" => Some(Self::Custom),
            "unlimited" => Some(Self::Unlimited),
            _ => None,
        }
    }
}

/// A workspace's send policy with its use and rates (the last 7 days for the rates).
#[derive(Clone, Debug, PartialEq)]
pub struct SendPolicyView {
    pub workspace_id: Id,
    pub workspace_name: String,
    pub tier: SendTier,
    /// `None` on the `unlimited` tier.
    pub limits: Option<Limits>,
    /// Recipients released by the workspace through shared routes in the last hour and day.
    pub use_hour: i64,
    pub use_day: i64,
    /// Percent of the recipients released in 7 days that bounced hard / complained.
    pub hard_bounce_rate_7d: f64,
    pub complaint_rate_7d: f64,
    /// Recipients released in 7 days.
    pub recipients_7d: i64,
    pub paused_at: Option<TimestampMillis>,
    pub pause_reason: Option<String>,
    pub first_release_at: Option<TimestampMillis>,
}

/// Hard bounces and complaints among the recipients released through a shared route in the 7
/// days up to `now`:
/// (recipients, hard bounces, complaints). A complaint is a recipient a provider reported a
/// complaint about (`complained_at`) in the window, also when its address was on the
/// suppression list already. After a resume (`mail_send_policies.resumed_at`) only recipients
/// released after it count, so the bounces that paused the workspace cannot pause it again at
/// once. Vacation replies (`mail_outbound.automatic`) never count: their recipients were not
/// chosen by a member.
async fn rates(
    connection: &mut AnyConnection,
    workspace_id: Id,
    now: i64,
) -> Result<(i64, i64, i64), MailError> {
    let resumed: Option<i64> =
        sqlx::query_scalar("SELECT resumed_at FROM mail_send_policies WHERE workspace_id = $1")
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *connection)
            .await?
            .flatten();
    let since = (now - WEEK_MS).max(resumed.unwrap_or(i64::MIN));
    let shared = shared_route("o.route_id");
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) AS total, \
         CAST(COALESCE(SUM(CASE WHEN r.bounce_kind = 'hard' THEN 1 ELSE 0 END), 0) AS BIGINT) AS hard \
         FROM mail_outbound_recipients r JOIN mail_outbound o ON o.id = r.outbound_id \
         WHERE r.workspace_id = $1 AND o.automatic = 0 AND r.released_at > $2 \
         AND r.released_at <= $3 AND {shared}"
    )))
    .bind(workspace_id.to_string())
    .bind(since)
    .bind(now)
    .fetch_one(&mut *connection)
    .await?;
    let resumed_filter = if resumed.is_some() {
        " AND r.released_at > $4"
    } else {
        ""
    };
    let mut complaints = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM mail_outbound_recipients r \
         JOIN mail_outbound o ON o.id = r.outbound_id WHERE r.workspace_id = $1 \
         AND o.automatic = 0 AND r.complained_at > $2 AND r.complained_at <= $3 \
         AND {shared}{resumed_filter}"
    )))
    .bind(workspace_id.to_string())
    .bind(now - WEEK_MS)
    .bind(now);
    if let Some(resumed) = resumed {
        complaints = complaints.bind(resumed);
    }
    let complaints = complaints.fetch_one(&mut *connection).await?;
    Ok((row.try_get("total")?, row.try_get("hard")?, complaints))
}

fn percent(part: i64, total: i64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

/// The numbers of one tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
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

/// The instance's numbers of the `new` and `standard` tiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TierLimits {
    pub new: Limits,
    pub standard: Limits,
}

impl Limits {
    fn valid(self) -> bool {
        [
            self.per_user_hour,
            self.per_user_day,
            self.per_ws_hour,
            self.per_ws_day,
            self.max_recipients,
        ]
        .iter()
        .all(|number| (1..=10_000_000).contains(number))
    }
}

impl MailRepository {
    /// The instance's tier defaults (root user).
    pub async fn send_tiers(&self) -> Result<TierLimits, MailError> {
        let tiers: Option<String> =
            sqlx::query_scalar("SELECT mail_send_tiers FROM instance_settings WHERE id = 1")
                .fetch_optional(self.database.pool())
                .await?;
        let tiers = tiers
            .as_deref()
            .and_then(|json| serde_json::from_str::<Tiers>(json).ok());
        Ok(TierLimits {
            new: tiers.as_ref().and_then(|t| t.new).unwrap_or(Limits::NEW),
            standard: tiers
                .as_ref()
                .and_then(|t| t.standard)
                .unwrap_or(Limits::STANDARD),
        })
    }

    /// Sets the instance's tier defaults (root user); every number is 1 to 10,000,000.
    pub async fn set_send_tiers(
        &self,
        actor_id: Id,
        tiers: TierLimits,
        request_id: &str,
    ) -> Result<TierLimits, MailError> {
        if !tiers.new.valid() || !tiers.standard.valid() {
            return Err(MailError::Invalid { field: "tiers" });
        }
        let json =
            serde_json::to_string(&tiers).map_err(|_| MailError::Invalid { field: "tiers" })?;
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        sqlx::query(
            "UPDATE instance_settings SET mail_send_tiers = $1, updated_at = $2 WHERE id = 1",
        )
        .bind(json)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        crate::audit::record_global(
            &mut transaction,
            Some(actor_id),
            "instance.mail_send_tiers_updated",
            crate::audit::AuditOutcome::Success,
            "mail_send_tiers",
            None,
            request_id,
            serde_json::json!({ "new": tiers.new, "standard": tiers.standard }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(tiers)
    }
}

/// The limits of the workspace: its tier's numbers from the instance settings, its own when the
/// tier is `custom`, or `None` when the tier is `unlimited`. A workspace without a policy row is
/// `new` on Orbit Cloud (`cloud`), else `unlimited`. Numbers that cannot be read fail closed to the
/// `new` tier.
pub(super) async fn load_limits(
    connection: &mut AnyConnection,
    workspace_id: Id,
    cloud: bool,
) -> Result<Option<Limits>, MailError> {
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
        None => (default_tier(cloud).to_owned(), None),
    };
    if tier == "unlimited" {
        return Ok(None);
    }
    if tier == "custom" {
        return Ok(Some(
            match custom.as_deref().map(serde_json::from_str::<Limits>) {
                Some(Ok(limits)) => limits,
                _ => {
                    tracing::warn!(%workspace_id, "custom mail send limits are missing or malformed");
                    Limits::NEW
                }
            },
        ));
    }
    let tiers: Option<String> =
        sqlx::query_scalar("SELECT mail_send_tiers FROM instance_settings WHERE id = 1")
            .fetch_optional(&mut *connection)
            .await?;
    let Some(Ok(tiers)) = tiers.as_deref().map(serde_json::from_str::<Tiers>) else {
        tracing::warn!("mail_send_tiers is missing or malformed");
        return Ok(Some(Limits::NEW));
    };
    Ok(Some(if tier == "new" {
        tiers.new.unwrap_or(Limits::NEW)
    } else {
        tiers.standard.unwrap_or(Limits::NEW)
    }))
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
    let shared = shared_route("o.route_id");
    due.push_bind(since);
    due.push(" AND o.release_at <= ");
    due.push_bind(now);
    due.push(" AND o.workspace_id = ");
    due.push_bind(workspace_id.to_string());
    due.push(" AND ");
    due.push(&shared);
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

/// The filter of the recipients of a scope released through a shared route in a window, without
/// those of `exclude`.
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
    sql.push(" AND outbound_id IN (SELECT o.id FROM mail_outbound o WHERE ");
    sql.push(&shared_route("o.route_id"));
    sql.push(")");
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
    /// The limits that apply to the workspace: `None` when it has none.
    pub async fn limits_for(&self, workspace_id: Id) -> Result<Option<Limits>, MailError> {
        let mut connection = self.database.pool().acquire().await?;
        load_limits(&mut connection, workspace_id, self.cloud).await
    }

    /// Whether `count` more recipients fit in the hour and day windows of the member and of the
    /// workspace at `now`: `None` when they do, else the first time they would (the latest of the
    /// four windows). `exclude` is the outbound row being checked: neither its draft (a held,
    /// due row) nor its own recipient rows (a released row that delivery checks) count. Without
    /// a member (`None`: the author left the workspace) only the workspace's windows apply.
    pub async fn window_allows(
        &self,
        transaction: &mut Transaction<'_, Any>,
        workspace_id: Id,
        membership_id: Option<Id>,
        count: i64,
        now: TimestampMillis,
        exclude: Option<Id>,
    ) -> Result<Option<TimestampMillis>, MailError> {
        let Some(limits) = load_limits(transaction, workspace_id, self.cloud).await? else {
            return Ok(None);
        };
        let now = now.as_millis();
        let mut later = None;
        for (member, window, limit) in [
            (true, HOUR_MS, limits.per_user_hour),
            (true, DAY_MS, limits.per_user_day),
            (false, HOUR_MS, limits.per_ws_hour),
            (false, DAY_MS, limits.per_ws_day),
        ] {
            let scope = match (member, membership_id) {
                (true, Some(membership_id)) => Some(membership_id),
                (true, None) => continue,
                (false, _) => None,
            };
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

    /// After a hard bounce or a complaint, in the same transaction: pauses the workspace's
    /// sending when it has at least 100 recipients released in the last 7 days and more than 5 %
    /// of them bounced hard or more than 0.3 % complained (spec §5.8). A new Send is then refused
    /// (`sending_paused`) and released messages wait with `paused`. Returns whether it paused
    /// the workspace now. A workspace without a policy row gets one (default tier) first.
    pub async fn check_pause(
        &self,
        transaction: &mut Transaction<'_, Any>,
        workspace_id: Id,
        now: TimestampMillis,
    ) -> Result<bool, MailError> {
        let now = now.as_millis();
        let (total, hard, complaints) = rates(transaction, workspace_id, now).await?;
        if total < PAUSE_MIN_RECIPIENTS {
            return Ok(false);
        }
        let reason = if hard * 10_000 > PAUSE_HARD_BP * total {
            format!(
                "Hard bounces: {hard} of {total} recipients in 7 days ({:.1} %, the limit is 5 %).",
                percent(hard, total)
            )
        } else if complaints * 10_000 > PAUSE_COMPLAINT_BP * total {
            format!(
                "Complaints: {complaints} of {total} recipients in 7 days ({:.2} %, the limit is 0.3 %).",
                percent(complaints, total)
            )
        } else {
            return Ok(false);
        };
        sqlx::query(
            "INSERT INTO mail_send_policies (workspace_id, tier, created_at, updated_at) \
             VALUES ($1, $2, $3, $3) ON CONFLICT (workspace_id) DO NOTHING",
        )
        .bind(workspace_id.to_string())
        .bind(default_tier(self.cloud))
        .bind(now)
        .execute(&mut **transaction)
        .await?;
        let paused = sqlx::query(
            "UPDATE mail_send_policies SET paused_at = $1, pause_reason = $2, resumed_by = NULL, \
             first_paused_at = COALESCE(first_paused_at, $1), updated_at = $1 \
             WHERE workspace_id = $3 AND paused_at IS NULL",
        )
        .bind(now)
        .bind(reason)
        .bind(workspace_id.to_string())
        .execute(&mut **transaction)
        .await?
        .rows_affected();
        Ok(paused == 1)
    }

    /// `mail.send_policy`: moves a `new` workspace to `standard` when its first release is at
    /// least 14 days old, it was never paused (`first_paused_at` stays after a resume), and over the
    /// last 7 days its hard bounce rate is under 2 % and its complaint rate under 0.1 %.
    /// Returns how many were promoted.
    pub async fn run_send_policy(&self, now: TimestampMillis) -> Result<u64, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let rows = sqlx::query(
            "SELECT workspace_id FROM mail_send_policies WHERE tier = 'new' \
             AND first_release_at IS NOT NULL AND first_release_at <= $1 \
             AND first_paused_at IS NULL AND paused_at IS NULL AND pause_reason IS NULL ORDER BY workspace_id",
        )
        .bind(now.as_millis() - PROMOTE_AFTER_MS)
        .fetch_all(&mut *transaction)
        .await?;
        let mut promoted = 0;
        for row in rows {
            let workspace_id = parse_id(&row.try_get::<String, _>("workspace_id")?)?;
            let (total, hard, complaints) =
                rates(&mut transaction, workspace_id, now.as_millis()).await?;
            let clean = total == 0
                || (hard * 10_000 < PROMOTE_HARD_BP * total
                    && complaints * 10_000 < PROMOTE_COMPLAINT_BP * total);
            if !clean {
                continue;
            }
            promoted += sqlx::query(
                "UPDATE mail_send_policies SET tier = 'standard', updated_at = $1 \
                 WHERE workspace_id = $2 AND tier = 'new'",
            )
            .bind(now.as_millis())
            .bind(workspace_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        }
        transaction.commit().await?;
        Ok(promoted)
    }

    async fn policy_view(
        &self,
        connection: &mut AnyConnection,
        workspace_id: Id,
        now: TimestampMillis,
    ) -> Result<SendPolicyView, MailError> {
        let name: String =
            sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1 AND deleted_at IS NULL")
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *connection)
                .await?
                .ok_or(MailError::NotFound)?;
        let policy = sqlx::query(
            "SELECT tier, paused_at, pause_reason, first_release_at FROM mail_send_policies \
             WHERE workspace_id = $1",
        )
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *connection)
        .await?;
        let tier = match &policy {
            Some(row) => row.try_get::<String, _>("tier")?,
            None => default_tier(self.cloud).to_owned(),
        };
        let limits = load_limits(connection, workspace_id, self.cloud).await?;
        let now = now.as_millis();
        let mut used = [0_i64; 2];
        let shared = shared_route("o.route_id");
        for (slot, window) in used.iter_mut().zip([HOUR_MS, DAY_MS]) {
            *slot = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*) FROM mail_outbound_recipients WHERE workspace_id = $1 \
                 AND released_at > $2 AND released_at <= $3 \
                 AND outbound_id IN (SELECT o.id FROM mail_outbound o WHERE {shared})"
            )))
            .bind(workspace_id.to_string())
            .bind(now - window)
            .bind(now)
            .fetch_one(&mut *connection)
            .await?;
        }
        let (total, hard, complaints) = rates(connection, workspace_id, now).await?;
        let at = |column: &str| -> Result<Option<TimestampMillis>, MailError> {
            Ok(match &policy {
                Some(row) => row
                    .try_get::<Option<i64>, _>(column)?
                    .map(TimestampMillis::from_millis),
                None => None,
            })
        };
        Ok(SendPolicyView {
            workspace_id,
            workspace_name: name,
            tier: SendTier::parse(&tier).unwrap_or(SendTier::New),
            limits,
            use_hour: used[0],
            use_day: used[1],
            hard_bounce_rate_7d: percent(hard, total),
            complaint_rate_7d: percent(complaints, total),
            recipients_7d: total,
            paused_at: at("paused_at")?,
            pause_reason: match &policy {
                Some(row) if at("paused_at")?.is_some() => row.try_get("pause_reason")?,
                _ => None,
            },
            first_release_at: at("first_release_at")?,
        })
    }

    /// The workspace's policy with its use and rates (owners and admins).
    pub async fn workspace_send_policy(
        &self,
        workspace_id: Id,
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<SendPolicyView, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let view = self
            .policy_view(&mut transaction, workspace_id, now)
            .await?;
        transaction.commit().await?;
        Ok(view)
    }

    /// The policy of any workspace (instance admins, checked by the caller).
    pub async fn send_policy(
        &self,
        workspace_id: Id,
        now: TimestampMillis,
    ) -> Result<SendPolicyView, MailError> {
        let mut connection = self.database.pool().acquire().await?;
        self.policy_view(&mut connection, workspace_id, now).await
    }

    /// The policies of all workspaces, paused ones first (instance admins), up to 500.
    pub async fn list_send_policies(
        &self,
        now: TimestampMillis,
    ) -> Result<Vec<SendPolicyView>, MailError> {
        let mut connection = self.database.pool().acquire().await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT w.id FROM workspaces w LEFT JOIN mail_send_policies p ON p.workspace_id = w.id \
             WHERE w.deleted_at IS NULL \
             ORDER BY CASE WHEN p.paused_at IS NULL THEN 1 ELSE 0 END, w.name, w.id LIMIT $1",
        )
        .bind(ADMIN_LIST_MAX)
        .fetch_all(&mut *connection)
        .await?;
        let mut list = Vec::with_capacity(ids.len());
        for id in ids {
            list.push(
                self.policy_view(&mut connection, parse_id(&id)?, now)
                    .await?,
            );
        }
        Ok(list)
    }

    /// Sets the workspace's tier (instance admins). `custom` needs `custom` limits (all numbers
    /// at least 1); the other tiers drop them. `unlimited` removes every limit. Makes the policy row when there is none.
    pub async fn set_send_policy(
        &self,
        workspace_id: Id,
        actor_id: Id,
        tier: SendTier,
        custom: Option<Limits>,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<SendPolicyView, MailError> {
        let custom_json = match (tier, custom) {
            (SendTier::Custom, Some(limits)) => {
                if !limits.valid() {
                    return Err(MailError::Invalid {
                        field: "custom_limits",
                    });
                }
                Some(
                    serde_json::to_string(&limits).map_err(|_| MailError::Invalid {
                        field: "custom_limits",
                    })?,
                )
            }
            (SendTier::Custom, None) => {
                return Err(MailError::Invalid {
                    field: "custom_limits",
                });
            }
            _ => None,
        };
        let mut transaction = self.database.immediate_transaction().await?;
        let exists: Option<String> =
            sqlx::query_scalar("SELECT id FROM workspaces WHERE id = $1 AND deleted_at IS NULL")
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
        if exists.is_none() {
            return Err(MailError::NotFound);
        }
        sqlx::query(
            "INSERT INTO mail_send_policies (workspace_id, tier, custom_limits, created_at, \
             updated_at) VALUES ($1, $2, $3, $4, $4) ON CONFLICT (workspace_id) DO UPDATE SET \
             tier = $2, custom_limits = $3, updated_at = $4",
        )
        .bind(workspace_id.to_string())
        .bind(tier.as_str())
        .bind(custom_json)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        crate::audit::record(
            &mut transaction,
            workspace_id,
            Some(actor_id),
            "instance.mail_send_policy_updated",
            crate::audit::AuditOutcome::Success,
            "mail_send_policy",
            Some(workspace_id),
            request_id,
            serde_json::json!({ "tier": tier.as_str(), "custom_limits": custom }),
            now,
        )
        .await?;
        let view = self
            .policy_view(&mut transaction, workspace_id, now)
            .await?;
        transaction.commit().await?;
        Ok(view)
    }

    /// Resumes a paused workspace (instance admins): clears the pause and records who did it
    /// and when (`resumed_at`: the rates count only recipients released after it). The reason
    /// stays, so the workspace is never promoted. The `mail.send_sweep` queues the
    /// held `paused` messages again. `NotFound` for a workspace without a policy.
    pub async fn resume_sending(
        &self,
        workspace_id: Id,
        by: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<SendPolicyView, MailError> {
        let mut transaction = self.database.immediate_transaction().await?;
        let found = sqlx::query(
            "UPDATE mail_send_policies SET paused_at = NULL, resumed_by = $1, resumed_at = $2, \
             updated_at = $2 WHERE workspace_id = $3",
        )
        .bind(by.to_string())
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if found == 0 {
            return Err(MailError::NotFound);
        }
        crate::audit::record(
            &mut transaction,
            workspace_id,
            Some(by),
            "instance.mail_sending_resumed",
            crate::audit::AuditOutcome::Success,
            "mail_send_policy",
            Some(workspace_id),
            request_id,
            serde_json::json!({}),
            now,
        )
        .await?;
        let view = self
            .policy_view(&mut transaction, workspace_id, now)
            .await?;
        transaction.commit().await?;
        Ok(view)
    }
}
