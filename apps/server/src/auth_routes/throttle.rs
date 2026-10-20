//! Sign-in and registration throttling that holds on every server of the database: the failures
//! (per email address and per IP address) are kept in `auth_throttle_emails` and
//! `auth_throttle_ips`. Each server keeps its own [`LoginThrottler`] for the attempts in flight,
//! and loads the stored failures into it before each attempt.

use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use orbit_platform::{
    Database, EmailFailures, LoginReservation, LoginThrottler, Row, THROTTLE_WINDOW_MILLIS,
    ThrottleDecision, TimestampMillis, normalize_email,
};

pub(crate) struct SharedThrottler {
    /// Keeps the stored failures of different throttlers apart (`login`, `registration`).
    scope: &'static str,
    database: Database,
    local: Mutex<LoginThrottler>,
}

impl SharedThrottler {
    pub(crate) fn new(scope: &'static str, database: Database) -> Self {
        Self {
            scope,
            database,
            local: Mutex::new(LoginThrottler::new()),
        }
    }

    fn local(&self) -> MutexGuard<'_, LoginThrottler> {
        self.local.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reserves an attempt, after reading what every server recorded for the email and IP
    /// address. When the database cannot be read, this server's own record decides.
    pub(crate) async fn reserve(
        &self,
        email: &str,
        ip: IpAddr,
        now: TimestampMillis,
    ) -> Result<LoginReservation, ThrottleDecision> {
        match self.stored(email, ip, now).await {
            Ok((failures, ip_failures)) => self.local().load(email, failures, ip, ip_failures),
            Err(error) => tracing::warn!(%error, "stored sign-in failures"),
        }
        self.local().reserve(email, ip, now)
    }

    pub(crate) async fn finish_failure(&self, reservation: LoginReservation, now: TimestampMillis) {
        let attempt = {
            let mut local = self.local();
            let attempt = local.attempt(reservation);
            local.finish_failure(reservation, now);
            attempt
        };
        if let Some((email, ip)) = attempt
            && let Err(error) = self.record_failure(&email, ip, now).await
        {
            tracing::warn!(%error, "recording a sign-in failure");
        }
    }

    pub(crate) async fn finish_success(&self, reservation: LoginReservation) {
        let attempt = {
            let mut local = self.local();
            let attempt = local.attempt(reservation);
            local.finish_success(reservation);
            attempt
        };
        if let Some((email, _)) = attempt
            && let Err(error) =
                sqlx::query("DELETE FROM auth_throttle_emails WHERE scope = $1 AND email = $2")
                    .bind(self.scope)
                    .bind(&email)
                    .execute(self.database.pool())
                    .await
        {
            tracing::warn!(%error, "clearing sign-in failures");
        }
    }

    /// The failures of the email address and the IP address that still count.
    async fn stored(
        &self,
        email: &str,
        ip: IpAddr,
        now: TimestampMillis,
    ) -> Result<(Option<EmailFailures>, Vec<TimestampMillis>), sqlx::Error> {
        let pool = self.database.pool();
        let cutoff = now.as_millis() - THROTTLE_WINDOW_MILLIS;
        let failures = sqlx::query(
            "SELECT failures, last_failure_at, blocked_until FROM auth_throttle_emails \
             WHERE scope = $1 AND email = $2 AND (last_failure_at > $3 OR blocked_until >= $4)",
        )
        .bind(self.scope)
        .bind(normalize_email(email))
        .bind(cutoff)
        .bind(now.as_millis())
        .fetch_optional(pool)
        .await?
        .map(|row| EmailFailures {
            count: u32::try_from(row.get::<i64, _>("failures")).unwrap_or(u32::MAX),
            last_failure_at: TimestampMillis::from_millis(row.get("last_failure_at")),
            blocked_until: TimestampMillis::from_millis(row.get("blocked_until")),
        });
        let ip_failures = sqlx::query_scalar::<_, i64>(
            "SELECT failed_at FROM auth_throttle_ips WHERE scope = $1 AND ip = $2 AND failed_at > $3 \
             ORDER BY failed_at",
        )
        .bind(self.scope)
        .bind(ip.to_string())
        .bind(cutoff)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(TimestampMillis::from_millis)
        .collect();
        Ok((failures, ip_failures))
    }

    /// Records a failure on top of what every server recorded, in one serialized transaction, so
    /// failures that two servers record at once both count.
    async fn record_failure(
        &self,
        email: &str,
        ip: IpAddr,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.database.immediate_transaction().await?;
        let cutoff = now.as_millis() - THROTTLE_WINDOW_MILLIS;
        let stored = sqlx::query(
            "SELECT failures, last_failure_at, blocked_until FROM auth_throttle_emails \
             WHERE scope = $1 AND email = $2 AND (last_failure_at > $3 OR blocked_until >= $4)",
        )
        .bind(self.scope)
        .bind(email)
        .bind(cutoff)
        .bind(now.as_millis())
        .fetch_optional(&mut *transaction)
        .await?
        .map(|row| EmailFailures {
            count: u32::try_from(row.get::<i64, _>("failures")).unwrap_or(u32::MAX),
            last_failure_at: TimestampMillis::from_millis(row.get("last_failure_at")),
            blocked_until: TimestampMillis::from_millis(row.get("blocked_until")),
        });
        let mut counted = LoginThrottler::new();
        counted.load(email, stored, ip, Vec::new());
        counted.record_failure(email, ip, now);
        let Some(failures) = counted.email_failures(email) else {
            return transaction.commit().await;
        };
        sqlx::query(
            "INSERT INTO auth_throttle_emails (scope, email, failures, last_failure_at, blocked_until) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (scope, email) DO UPDATE SET \
             failures = excluded.failures, last_failure_at = excluded.last_failure_at, \
             blocked_until = excluded.blocked_until",
        )
        .bind(self.scope)
        .bind(email)
        .bind(i64::from(failures.count))
        .bind(failures.last_failure_at.as_millis())
        .bind(failures.blocked_until.as_millis())
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO auth_throttle_ips (scope, ip, failed_at) VALUES ($1, $2, $3)")
            .bind(self.scope)
            .bind(ip.to_string())
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        // What no longer counts.
        // Only this scope: others (`oauth.register`) count over a longer time.
        sqlx::query("DELETE FROM auth_throttle_ips WHERE scope = $1 AND failed_at <= $2")
            .bind(self.scope)
            .bind(cutoff)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "DELETE FROM auth_throttle_emails WHERE last_failure_at <= $1 AND blocked_until < $2",
        )
        .bind(cutoff)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await
    }
}
