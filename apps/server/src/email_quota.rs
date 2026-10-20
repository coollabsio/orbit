//! Caps on the emails Orbit sends on behalf of someone else: registration and password reset links
//! (to an address anyone can type) and invitations. The sent emails are kept in `email_sends` for a
//! day, so the caps hold for every server of the database.

use std::time::Duration;

use orbit_platform::{Database, Row, ThrottleDecision, TimestampMillis};

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1_000;

/// At most `per_day` emails in 24 hours for `subject` (an address, a workspace, a user) in `scope`,
/// at least `cooldown` apart.
pub(crate) struct Quota {
    pub scope: &'static str,
    pub subject: String,
    pub per_day: u32,
    pub cooldown: Duration,
}

/// Counts one email against every quota when all of them allow it; otherwise counts nothing and
/// tells when the email would be allowed.
pub(crate) async fn take(
    database: &Database,
    quotas: &[Quota],
    now: TimestampMillis,
) -> Result<ThrottleDecision, sqlx::Error> {
    let now = now.as_millis();
    let mut transaction = database.immediate_transaction().await?;
    let mut wait = 0_i64;
    for quota in quotas {
        let row = sqlx::query(
            "SELECT COUNT(*) AS sent, MIN(sent_at) AS first_at, MAX(sent_at) AS last_at \
             FROM email_sends WHERE scope = $1 AND subject = $2 AND sent_at > $3",
        )
        .bind(quota.scope)
        .bind(&quota.subject)
        .bind(now - DAY_MILLIS)
        .fetch_one(&mut *transaction)
        .await?;
        let sent: i64 = row.get("sent");
        if sent == 0 {
            continue;
        }
        let cooldown = i64::try_from(quota.cooldown.as_millis()).unwrap_or(i64::MAX);
        wait = wait.max(row.get::<i64, _>("last_at").saturating_add(cooldown) - now);
        if sent >= i64::from(quota.per_day) {
            wait = wait.max(row.get::<i64, _>("first_at") + DAY_MILLIS - now);
        }
    }
    if wait > 0 {
        transaction.rollback().await?;
        return Ok(ThrottleDecision::RetryAfter(Duration::from_millis(
            wait.unsigned_abs(),
        )));
    }
    for quota in quotas {
        sqlx::query("INSERT INTO email_sends (scope, subject, sent_at) VALUES ($1, $2, $3)")
            .bind(quota.scope)
            .bind(&quota.subject)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
    }
    sqlx::query("DELETE FROM email_sends WHERE sent_at <= $1")
        .bind(now - DAY_MILLIS)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(ThrottleDecision::Allowed)
}
