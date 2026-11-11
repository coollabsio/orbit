//! `webhook.deliver`: sends one delivery, signed as Standard Webhooks signs it
//! (<https://www.standardwebhooks.com/>). Any 2xx answer delivers it; anything else is retried with
//! the default backoff until the last attempt, which marks it `failed`. Redirects are not followed.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use orbit_platform::{
    Database, Id, JobContext, JobError, JobKindRegistrationError, PublicResolver, Row,
    TimestampMillis, Worker,
};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use url::Url;

use super::{DELIVER_JOB, job_kind, store};
use crate::chat_routes::interactions::sendable;

const TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = "Orbit-Webhooks/1.0";
const SECRET_PREFIX: &str = "whsec_";

/// Sends deliveries. Outside development it reaches public https addresses only.
#[derive(Clone)]
pub struct Deliverer {
    database: Database,
    app_key: Option<[u8; 32]>,
    client: reqwest::Client,
    development: bool,
}

impl Deliverer {
    #[must_use]
    pub fn new(database: Database, app_key: Option<[u8; 32]>, development: bool) -> Self {
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .connect_timeout(TIMEOUT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none());
        if !development {
            builder = builder.dns_resolver(Arc::new(PublicResolver));
        }
        Self {
            database,
            app_key,
            client: builder.build().expect("the webhook client builds"),
            development,
        }
    }

    async fn send(
        &self,
        url: &str,
        secret: &str,
        event_id: &str,
        event_type: &str,
        body: String,
    ) -> Result<u16, &'static str> {
        let url = Url::parse(url).map_err(|_| "blocked_address")?;
        if !sendable(&url, self.development) {
            return Err("blocked_address");
        }
        let key = secret_key(secret).ok_or("secret_unreadable")?;
        let timestamp = TimestampMillis::now()
            .as_millis()
            .div_euclid(1000)
            .to_string();
        let signature = sign(&key, event_id, &timestamp, &body);
        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("webhook-id", event_id)
            .header("webhook-timestamp", &timestamp)
            .header("webhook-signature", signature)
            .header("orbit-event-type", event_type)
            .body(body)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "unreachable"
                } else {
                    "failed"
                }
            })?;
        Ok(response.status().as_u16())
    }

    async fn deliver(&self, context: &JobContext) -> Result<(), JobError> {
        let delivery_id = context
            .job
            .payload
            .get("delivery_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| id.parse::<Id>().ok())
            .ok_or_else(|| JobError::Permanent(format!("{DELIVER_JOB} needs a delivery_id")))?;
        let job_id = context.job.id.to_string();
        let retryable =
            |error: sqlx::Error| JobError::Retryable(format!("webhook delivery: {error}"));
        let Some(row) = sqlx::query(
            "SELECT d.state, d.job_id, d.body, d.event_id, d.event_type, d.subscription_id, \
             s.url, s.signing_secret, s.enabled, \
             (SELECT COUNT(*) FROM workspaces w WHERE w.id = d.workspace_id AND w.deleted_at IS NULL) AS workspace_live, \
             (SELECT COUNT(*) FROM projects p JOIN webhook_subscription_projects sp \
               ON sp.project_id = p.id AND sp.subscription_id = d.subscription_id \
               WHERE p.id = d.project_id AND p.deleted_at IS NULL) AS project_live \
             FROM webhook_deliveries d JOIN webhook_subscriptions s ON s.id = d.subscription_id \
             WHERE d.id = $1",
        )
        .bind(delivery_id.to_string())
        .fetch_optional(self.database.pool())
        .await
        .map_err(retryable)?
        else {
            // The subscription was deleted with its deliveries.
            return Ok(());
        };
        // Only the delivery's current job sends it: an older one was cancelled or replaced.
        if row.get::<String, _>("state") != "pending"
            || row.get::<Option<String>, _>("job_id").as_deref() != Some(job_id.as_str())
        {
            return Ok(());
        }
        let subscription_id = store::parse_id(&row.get::<String, _>("subscription_id"))
            .map_err(|error| JobError::Permanent(error.to_string()))?;
        let now = TimestampMillis::now();
        if !row.get::<bool, _>("enabled")
            || row.get::<i64, _>("workspace_live") == 0
            || row.get::<i64, _>("project_live") == 0
        {
            sqlx::query(
                "UPDATE webhook_deliveries SET state = 'cancelled', updated_at = $1 \
                 WHERE id = $2 AND state = 'pending' AND job_id = $3",
            )
            .bind(now.as_millis())
            .bind(delivery_id.to_string())
            .bind(&job_id)
            .execute(self.database.pool())
            .await
            .map_err(retryable)?;
            return Ok(());
        }

        let secret = match self.app_key {
            None => Err("app_key_missing"),
            Some(key) => {
                crate::secret_box::decrypt_secret(&key, &row.get::<Vec<u8>, _>("signing_secret"))
                    .map_err(|()| "secret_unreadable")
            }
        };
        let result = match &secret {
            Ok(secret) => {
                self.send(
                    &row.get::<String, _>("url"),
                    secret,
                    &row.get::<String, _>("event_id"),
                    &row.get::<String, _>("event_type"),
                    row.get("body"),
                )
                .await
            }
            Err(reason) => Err(*reason),
        };
        let delivered = matches!(result, Ok(status) if (200..300).contains(&status));
        // A server without its key cannot sign until an operator fixes it: no point retrying.
        let last_attempt = secret.is_err() || context.claim.attempt >= context.job.max_attempts;
        let state = if delivered {
            "succeeded"
        } else if last_attempt {
            "failed"
        } else {
            "pending"
        };
        let (status, error) = match result {
            Ok(status) if delivered => (Some(i64::from(status)), None),
            Ok(status) => (Some(i64::from(status)), Some("http_status")),
            Err(reason) => (None, Some(reason)),
        };
        sqlx::query(
            "UPDATE webhook_deliveries SET attempt_count = attempt_count + 1, last_status = $1, \
             last_error = $2, last_attempt_at = $3, state = $4, delivered_at = $5, updated_at = $6 \
             WHERE id = $7 AND job_id = $8 AND state = 'pending'",
        )
        .bind(status)
        .bind(error)
        .bind(now.as_millis())
        .bind(state)
        .bind(delivered.then_some(now.as_millis()))
        .bind(now.as_millis())
        .bind(delivery_id.to_string())
        .bind(&job_id)
        .execute(self.database.pool())
        .await
        .map_err(retryable)?;
        if state != "pending"
            && let Err(error) = store::prune(&self.database, subscription_id, now).await
        {
            tracing::warn!(error = %error, "webhook delivery pruning");
        }
        let reason = || match status {
            Some(status) => format!("the endpoint answered HTTP {status}"),
            None => error.unwrap_or("failed").to_owned(),
        };
        match state {
            "succeeded" => Ok(()),
            "failed" => Err(JobError::Permanent(reason())),
            _ => Err(JobError::Retryable(reason())),
        }
    }
}

pub(crate) fn register_job(
    worker: Worker,
    deliverer: Deliverer,
) -> Result<Worker, JobKindRegistrationError> {
    worker.with_handler(job_kind(), move |context| {
        let deliverer = deliverer.clone();
        async move { deliverer.deliver(&context).await }
    })
}

/// A new signing secret: `whsec_` and 32 random bytes in base64.
pub(crate) fn new_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("{SECRET_PREFIX}{}", STANDARD.encode(bytes))
}

/// The HMAC key of a `whsec_` secret.
fn secret_key(secret: &str) -> Option<Vec<u8>> {
    STANDARD
        .decode(secret.strip_prefix(SECRET_PREFIX)?)
        .ok()
        .filter(|key| !key.is_empty())
}

/// `v1,` and the base64 HMAC-SHA256 of `{id}.{timestamp}.{body}`.
fn sign(key: &[u8], id: &str, timestamp: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(format!("{id}.{timestamp}.{body}").as_bytes());
    format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{new_secret, secret_key, sign};

    /// The example of the Standard Webhooks spec, checked with Python's `hmac` module.
    #[test]
    fn signs_as_standard_webhooks() {
        let key = secret_key("whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw").unwrap();
        assert_eq!(
            sign(
                &key,
                "msg_p5jXN8AQM9LWM0D4loKWxJek",
                "1614265330",
                r#"{"test": 2432232314}"#
            ),
            "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="
        );
    }

    #[test]
    fn new_secrets_carry_32_bytes() {
        let secret = new_secret();
        assert!(secret.starts_with("whsec_"));
        assert_eq!(secret_key(&secret).unwrap().len(), 32);
        assert_ne!(secret, new_secret());
    }
}
