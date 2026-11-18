//! Native Android pushes through Firebase Cloud Messaging (FCM HTTP v1): the message, the OAuth 2
//! access token, and [`FcmClient`], which sends to `/v1/projects/<project>/messages:send`.
//!
//! The access token comes from Google for a JWT (RS256) signed with the service account's key. It
//! is kept until shortly before it expires, and made again early only when FCM refuses it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use futures_util::future::BoxFuture;
use orbit_platform::{FcmConfig, Id, Row, TimestampMillis};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::{MAX_SUBSCRIPTIONS, Notice, PushService, SendError, decode_id, send_twice};

/// The largest message FCM takes.
pub const MAX_PAYLOAD_BYTES: usize = 4096;
/// The Android notification channel the app makes; a push names it.
pub const CHANNEL_ID: &str = "default";
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const FCM_HOST: &str = "https://fcm.googleapis.com";
/// How long Google takes the signed JWT for: the most it allows.
const ASSERTION_SECONDS: i64 = 60 * 60;
/// An access token is made again this long before it expires.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);
/// How long FCM keeps a push for a phone that is not reachable.
const TTL: &str = "86400s";
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// A phone the user gets pushes on.
#[derive(Clone, Debug)]
pub struct FcmDevice {
    pub id: Id,
    pub token: String,
}

/// What goes to every phone of a notice: the FCM `message` without its `token`.
#[derive(Clone, Debug)]
pub struct FcmMessage {
    pub message: Value,
}

impl FcmMessage {
    /// The request body for one phone.
    #[must_use]
    pub fn body(&self, token: &str) -> Value {
        let mut message = self.message.clone();
        message["token"] = json!(token);
        json!({ "message": message })
    }
}

pub trait FcmSender: Send + Sync {
    fn send(
        &self,
        device: FcmDevice,
        message: FcmMessage,
    ) -> BoxFuture<'static, Result<(), SendError>>;
}

/// A phone the user gets pushes on through FCM.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct FcmDeviceRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub label: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// The registration token as it is stored, or `None` when it is no FCM token. Tokens are about
/// 160 characters of base64url with a `:`; Google does not promise a length.
#[must_use]
pub fn parse_token(token: &str) -> Option<String> {
    let token = token.trim();
    let fits = (32..=1024).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'));
    fits.then(|| token.to_owned())
}

/// The message of a clamped notice: the same title, body and `url` as an APNs push. Notices with
/// the same tag replace each other on the phone.
#[must_use]
pub fn message(notice: &Notice) -> FcmMessage {
    FcmMessage {
        message: json!({
            "notification": { "title": notice.title, "body": notice.body },
            "data": { "url": notice.url },
            "android": {
                "priority": "high",
                "ttl": TTL,
                "notification": { "channel_id": CHANNEL_ID, "tag": notice.tag, "sound": "default" },
            },
        }),
    }
}

/// Where the client gets its access token and sends its pushes (other hosts in tests).
struct Endpoints {
    token: String,
    fcm: String,
}

/// Sends to FCM with the service account's key. Clones share the connections and the token.
#[derive(Clone)]
pub struct FcmClient {
    client: reqwest::Client,
    credentials: Arc<Credentials>,
}

struct Credentials {
    key: RsaKeyPair,
    client_email: String,
    project_id: String,
    endpoints: Endpoints,
    /// The access token and when it is made again.
    token: tokio::sync::Mutex<Option<(String, Instant)>>,
}

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
}

#[derive(Deserialize)]
struct AccessToken {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct TokenRefusal {
    #[serde(default)]
    error: String,
}

impl FcmClient {
    /// A client for this service account; an error when it is not the JSON key of a Google
    /// service account. The error never shows the key.
    pub fn new(config: &FcmConfig) -> Result<Self, String> {
        Self::with_endpoints(
            config,
            Endpoints {
                token: TOKEN_URL.to_owned(),
                fcm: FCM_HOST.to_owned(),
            },
        )
    }

    fn with_endpoints(config: &FcmConfig, endpoints: Endpoints) -> Result<Self, String> {
        let account: ServiceAccount = serde_json::from_str(config.service_account.expose())
            .map_err(|_| {
                "ORBIT_FCM_SERVICE_ACCOUNT is not the JSON key of a Google service account \
                 (it needs project_id, client_email and private_key)"
                    .to_owned()
            })?;
        let project_fits = (1..=64).contains(&account.project_id.len())
            && account
                .project_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !project_fits || account.client_email.is_empty() {
            return Err(
                "ORBIT_FCM_SERVICE_ACCOUNT has no valid project_id or client_email".to_owned(),
            );
        }
        let key = pkcs8_der(&account.private_key)
            .and_then(|der| RsaKeyPair::from_pkcs8(&der).ok())
            .ok_or_else(|| {
                "the private_key of ORBIT_FCM_SERVICE_ACCOUNT is not an RSA key (PKCS#8 PEM)"
                    .to_owned()
            })?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("the HTTP client for FCM: {error}"))?;
        Ok(Self {
            client,
            credentials: Arc::new(Credentials {
                key,
                client_email: account.client_email,
                project_id: account.project_id,
                endpoints,
                token: tokio::sync::Mutex::new(None),
            }),
        })
    }
}

/// The DER bytes of a `-----BEGIN PRIVATE KEY-----` PEM.
fn pkcs8_der(pem: &str) -> Option<Vec<u8>> {
    let body = pem.trim();
    let body = body
        .strip_prefix("-----BEGIN PRIVATE KEY-----")?
        .strip_suffix("-----END PRIVATE KEY-----")?;
    let base64: String = body.split_whitespace().collect();
    STANDARD.decode(base64).ok()
}

impl Credentials {
    /// The JWT that Google exchanges for an access token (RS256).
    fn assertion(&self, now_seconds: i64) -> Result<String, SendError> {
        let header = json!({ "alg": "RS256", "typ": "JWT" });
        let claims = json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.endpoints.token,
            "iat": now_seconds,
            "exp": now_seconds + ASSERTION_SECONDS,
        });
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let mut signature = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signed.as_bytes(),
                &mut signature,
            )
            .map_err(|_| {
                SendError::Failed("the FCM access token could not be signed".to_owned())
            })?;
        Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    /// The kept access token, or a new one from Google. Pushes that need one at the same time
    /// wait for the same request.
    async fn token(&self, client: &reqwest::Client) -> Result<String, SendError> {
        let mut kept = self.token.lock().await;
        if let Some((token, renew_at)) = kept.as_ref()
            && Instant::now() < *renew_at
        {
            return Ok(token.clone());
        }
        let assertion = self.assertion(TimestampMillis::now().as_millis() / 1000)?;
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer")
            .append_pair("assertion", &assertion)
            .finish();
        let response = client
            .post(&self.endpoints.token)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .timeout(SEND_TIMEOUT)
            .send()
            .await
            .map_err(|_| SendError::Busy("Google did not answer the token request".to_owned()))?;
        let status = response.status().as_u16();
        if status != 200 {
            let error = response
                .json::<TokenRefusal>()
                .await
                .map(|refusal| refusal.error)
                .unwrap_or_default();
            let refused = format!("Google token status {status} {error}");
            return Err(if status == 429 || status >= 500 {
                SendError::Busy(refused)
            } else {
                SendError::Failed(refused)
            });
        }
        let fresh = response
            .json::<AccessToken>()
            .await
            .map_err(|_| SendError::Busy("Google sent no access token".to_owned()))?;
        let lifetime = Duration::from_secs(fresh.expires_in).saturating_sub(TOKEN_MARGIN);
        *kept = Some((fresh.access_token.clone(), Instant::now() + lifetime));
        Ok(fresh.access_token)
    }

    /// FCM refused `expired`: the next push gets another one.
    async fn expired(&self, expired: &str) {
        let mut kept = self.token.lock().await;
        if kept.as_ref().is_some_and(|(token, _)| token == expired) {
            *kept = None;
        }
    }
}

#[derive(Deserialize)]
struct Refusal {
    error: RefusalError,
}

#[derive(Deserialize)]
struct RefusalError {
    #[serde(default)]
    status: String,
    #[serde(default)]
    details: Vec<RefusalDetail>,
}

#[derive(Deserialize)]
struct RefusalDetail {
    /// Set in a `google.firebase.fcm.v1.FcmError`.
    #[serde(default, rename = "errorCode")]
    error_code: String,
    /// Set in a `google.rpc.BadRequest`: the fields of the message that are wrong.
    #[serde(default, rename = "fieldViolations")]
    field_violations: Vec<FieldViolation>,
}

#[derive(Deserialize)]
struct FieldViolation {
    #[serde(default)]
    field: String,
}

/// What a refused send means. `INVALID_ARGUMENT` is a bad token only when no other field of the
/// message is named: a mistake in the message must not remove every phone.
fn refusal(status: u16, body: &[u8]) -> SendError {
    let error = serde_json::from_slice::<Refusal>(body)
        .ok()
        .map(|refusal| refusal.error);
    let code = error
        .as_ref()
        .and_then(|error| {
            error
                .details
                .iter()
                .map(|detail| detail.error_code.as_str())
                .find(|code| !code.is_empty())
                .or(Some(error.status.as_str()))
        })
        .unwrap_or_default();
    let other_field = error.as_ref().is_some_and(|error| {
        error
            .details
            .iter()
            .flat_map(|detail| &detail.field_violations)
            .any(|violation| violation.field != "message.token")
    });
    let refused = format!("FCM status {status} {code}");
    match (status, code) {
        (_, "UNREGISTERED" | "SENDER_ID_MISMATCH") => SendError::Gone,
        (_, "INVALID_ARGUMENT") if !other_field => SendError::Gone,
        // An access token that expired: the push goes again with a new one.
        (401 | 429 | 500..=599, _) | (_, "QUOTA_EXCEEDED" | "UNAVAILABLE" | "INTERNAL") => {
            SendError::Busy(refused)
        }
        _ => SendError::Failed(refused),
    }
}

impl FcmSender for FcmClient {
    fn send(
        &self,
        device: FcmDevice,
        message: FcmMessage,
    ) -> BoxFuture<'static, Result<(), SendError>> {
        let (client, credentials) = (self.client.clone(), Arc::clone(&self.credentials));
        Box::pin(async move {
            let token = credentials.token(&client).await?;
            let response = client
                .post(format!(
                    "{}/v1/projects/{}/messages:send",
                    credentials.endpoints.fcm, credentials.project_id
                ))
                .bearer_auth(&token)
                .json(&message.body(&device.token))
                .timeout(SEND_TIMEOUT)
                .send()
                .await
                .map_err(|_| SendError::Busy("FCM did not answer".to_owned()))?;
            let status = response.status().as_u16();
            if status == 200 {
                return Ok(());
            }
            if status == 401 {
                credentials.expired(&token).await;
            }
            let body = response.bytes().await.unwrap_or_default();
            Err(refusal(status, &body))
        })
    }
}

impl PushService {
    /// Sends native Android pushes through this sender from now on; the first one set stays.
    pub fn set_fcm_sender(&self, sender: Arc<dyn FcmSender>) {
        let _ = self.inner.fcm.set(sender);
    }

    /// Whether this server can push to Android phones.
    #[must_use]
    pub fn fcm_enabled(&self) -> bool {
        self.inner.fcm.get().is_some()
    }

    pub async fn fcm_devices(&self, user_id: Id) -> Result<Vec<FcmDeviceRecord>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, label, created_at FROM fcm_devices WHERE user_id = $1 ORDER BY id",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(FcmDeviceRecord {
                    id: decode_id(&row.get::<String, _>("id"))?,
                    label: row.get("label"),
                    created_at: TimestampMillis::from_millis(row.get("created_at")),
                })
            })
            .collect()
    }

    /// Stores a phone's registration token for the user and the session. A phone has one token:
    /// its row moves to whoever registers it last. A user keeps the newest [`MAX_SUBSCRIPTIONS`]
    /// phones only. Callers check the token first ([`parse_token`]).
    pub async fn register_fcm(
        &self,
        user_id: Id,
        session_id: Id,
        token: &str,
        label: &str,
        now: TimestampMillis,
    ) -> Result<FcmDeviceRecord, sqlx::Error> {
        let mut tx = self.database.pool().begin().await?;
        let row = sqlx::query(
            "INSERT INTO fcm_devices (id, user_id, session_id, token, label, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (token) DO UPDATE SET \
             user_id = excluded.user_id, session_id = excluded.session_id, label = excluded.label \
             RETURNING id, created_at",
        )
        .bind(Id::new_v7().to_string())
        .bind(user_id.to_string())
        .bind(session_id.to_string())
        .bind(token)
        .bind(label)
        .bind(now.as_millis())
        .fetch_one(&mut *tx)
        .await?;
        let id: String = row.get("id");
        // The oldest ones above the limit go; never the one that registered now.
        sqlx::query(
            "DELETE FROM fcm_devices WHERE user_id = $1 AND id NOT IN ( \
             SELECT id FROM fcm_devices WHERE user_id = $1 \
             ORDER BY id = $2 DESC, created_at DESC, id DESC LIMIT $3)",
        )
        .bind(user_id.to_string())
        .bind(&id)
        .bind(MAX_SUBSCRIPTIONS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(FcmDeviceRecord {
            id: decode_id(&id)?,
            label: label.to_owned(),
            created_at: TimestampMillis::from_millis(row.get("created_at")),
        })
    }

    /// Removes one of the user's phones; nothing happens when it is not theirs.
    pub async fn unregister_fcm(&self, user_id: Id, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM fcm_devices WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id.to_string())
            .execute(self.database.pool())
            .await?;
        Ok(())
    }

    pub(super) async fn deliver_fcm(
        &self,
        user_id: Id,
        notice: &Notice,
    ) -> Result<usize, sqlx::Error> {
        let Some(sender) = self.inner.fcm.get() else {
            return Ok(0);
        };
        let now = TimestampMillis::now().as_millis();
        let rows = sqlx::query(
            "SELECT devices.id, devices.token FROM fcm_devices AS devices \
             JOIN sessions ON sessions.id = devices.session_id \
             WHERE devices.user_id = $1 AND sessions.revoked_at IS NULL \
             AND sessions.idle_expires_at > $2 AND sessions.absolute_expires_at > $3",
        )
        .bind(user_id.to_string())
        .bind(now)
        .bind(now)
        .fetch_all(self.database.pool())
        .await?;
        if rows.is_empty() {
            return Ok(0);
        }
        let message = message(notice);
        if message.message.to_string().len() > MAX_PAYLOAD_BYTES {
            tracing::warn!("FCM push not sent: the notice is too large");
            return Ok(0);
        }
        let sends = rows.into_iter().filter_map(|row| {
            let device = FcmDevice {
                id: row.get::<String, _>("id").parse().ok()?,
                token: row.get("token"),
            };
            let (sender, message) = (Arc::clone(sender), message.clone());
            Some(async move {
                let id = device.id;
                let result = send_twice(|| sender.send(device.clone(), message.clone())).await;
                (id, result)
            })
        });
        let mut sent = 0;
        for (id, result) in futures_util::future::join_all(sends).await {
            match result {
                Ok(()) => sent += 1,
                Err(SendError::Gone) => {
                    sqlx::query("DELETE FROM fcm_devices WHERE id = $1")
                        .bind(id.to_string())
                        .execute(self.database.pool())
                        .await?;
                }
                // A refusal here is mostly the server's (its key, its project): the phone stays.
                Err(SendError::Failed(reason) | SendError::Busy(reason)) => {
                    tracing::warn!(reason, "FCM push not sent");
                }
            }
        }
        Ok(sent)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use ring::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

    use super::*;

    /// A throwaway RSA key made for these tests; Google does not know it.
    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDG6Wa5d2m2JheJ\nJeYu70ly2OMRIBYUXLe1kxd6rpYvIr/Bk1fOiI49FDGD+VtKG5bC8s1e+k3ga6h9\nFqQGK6hPBHZz6bKcg4lV7El3uyJPDNxoOX9bG1IEdqcuz2R15SPIDESRJjKNrBqt\n6WJ0WLajokbe8KfqkNvsJlkGvBxGHoppO0g82SL2d0JfKCUaDOG3uNBgxPtgqcOO\n+9aWIj9eyX6Ty5HFmwZ/l76l/o+jftkVRYmABYc3QSfsIAu7vtey4blF+qLqt2Nr\nPNYZk9hTyU/q9+sSot2O9DSwUneg2L11cC8rhuo2cVZohbuFPQg0nlyv8fxjGX9f\n0IeJ14FVAgMBAAECggEAR+/CSGHw18C381BDCodcm03n9NoTUiLkB6VeMO6x/vME\nwYe9kkKdmmZSsAV+5/LJV9DCyE4i2v5kbXTCNj8UvBsDkdEbWPJTOUFeopAJjOg8\nT+83aWYch+O3e7zieqa3hZI2O/5B+y2aanoEssogp1Y5ZkhHmhPwjVujB8YBPbQL\njj8qE3luqnkGFWzK2WNtpx0KXwPRO5WWdWtmFD2Sjiv7NZZR3ggSGcaa/uy97HI6\nrkUxdqr28balHwIP3BAuNu2y+9r+vG7ryhpBjdQTycmluNgPANTNg2VB8NjxZp/v\nwxDkBCFDxs3xdF84Bb0DvFejDcFwcDG85oTZNz6qQwKBgQDpkEkE2N0eXWvj3SMn\nM3aOqQMxNWTe6Zgt/XpWV5GfQH6QLl5OHcRMEzMwbQSEioEnGzPqM34tpnE/99A+\nDHjzOJSvP7omePEThr5Q1Wmk5TUty5mWRLUSVU8V2CaO4fAW4a2L35hyhnSKJOgE\nb2vr9Da2IRQor/XMF6+nO5JmAwKBgQDaBPgHCdg2C7z8VSzRSmbU/NkQvy4DchYB\nvYhEjnl7KltmDewS7xPVXkCGn6zuZFTtm/NDdaH5HyANIpjzBFjA4cD+iXZ1LnjO\nnBQR0Dx69+zG60AnqEt9JH/FuscgsbeVPHRCbte84iJZprlmN/P4i4WXlWl4liGn\nCpvOJ1VnxwKBgQCFheDYjonKhCnbuF4SWQ9b+y9CAH5MGUrQb+pyVdmAK0SvdHKh\nQc8ololbE9fTVcMD8j85knp96nsv0KIR9ttmYGBP16S5vcZEHXHLkFW22PoP1idV\niq3hbzepC0rvwCabTiUCo67pOgiBuRuS/Ye6+oAUmtCFeiKQrLHUbvAAtQKBgQDG\n1aVG9gmEMOxozV2UeuCR+xAUn+ddKR089RhkPfQ5CgZVavkUw4o5HD6JCPN0b3it\nQ3921L+mLmbS0+zeJeaz8rBK9B9e+8Ud2WbMYDfohlghisf14x0FwtXtBU06Whpu\nNrZpKCIFl89wKAgKW5lb4hMyjgek/HcwmxJBz8mqXQKBgDLVWgnVWTK1MgASt0Hu\nBuOhJ3Z15oT/IlL8XulbhZBywH/0+gTFc41lwnmljnD/Bfi6vCdWRr3CbxbuR754\nW64RUjtvnRgBYuRlTLYGuHcNS4yFLF1kvRK7EzIIWWVS6z5WIDnt9dSRrvIMWUZK\nip+lg2pdaU9cxa1pTpxoKcuH\n-----END PRIVATE KEY-----";

    fn config(private_key: &str) -> FcmConfig {
        FcmConfig {
            service_account: orbit_platform::Secret::new(
                json!({
                    "type": "service_account",
                    "project_id": "orbit-test",
                    "client_email": "push@orbit-test.iam.gserviceaccount.com",
                    "private_key": private_key,
                })
                .to_string(),
            ),
        }
    }

    fn notice() -> Notice {
        Notice {
            title: "Ada in #general".to_owned(),
            body: "Hello".to_owned(),
            url: "/chat/1?thread=2".to_owned(),
            tag: "chat:1".to_owned(),
            sound: "message",
            icon: Some("/api/v1/users/1/avatar".to_owned()),
        }
    }

    #[test]
    fn tokens_are_base64url_with_colons_of_a_sane_length() {
        let token = format!("cX9{}:APA91b{}", "a".repeat(19), "Z-_9".repeat(34));
        assert_eq!(parse_token(&format!(" {token}\n")), Some(token));
        assert_eq!(parse_token("short:token"), None);
        assert_eq!(parse_token(&"a".repeat(1025)), None);
        assert_eq!(parse_token(&format!("{}/../x", "a".repeat(40))), None);
        assert_eq!(parse_token(&format!("{} x", "a".repeat(40))), None);
    }

    #[test]
    fn the_message_shows_the_notice_with_its_url_as_data() {
        let message = message(&notice());
        assert_eq!(
            message.body("device-token"),
            json!({ "message": {
                "token": "device-token",
                "notification": { "title": "Ada in #general", "body": "Hello" },
                "data": { "url": "/chat/1?thread=2" },
                "android": {
                    "priority": "high",
                    "ttl": "86400s",
                    "notification": { "channel_id": "default", "tag": "chat:1", "sound": "default" },
                },
            } })
        );
    }

    #[test]
    fn refusals_remove_only_dead_tokens() {
        let fcm_error = |status: u16, code: &str| {
            json!({ "error": { "code": status, "status": code, "details": [
                { "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": code },
            ] } })
            .to_string()
        };
        let bad_field = |field: &str| {
            json!({ "error": { "code": 400, "status": "INVALID_ARGUMENT", "details": [
                { "@type": "type.googleapis.com/google.rpc.BadRequest",
                  "fieldViolations": [{ "field": field, "description": "bad" }] },
            ] } })
            .to_string()
        };
        let gone = |status, body: String| refusal(status, body.as_bytes()) == SendError::Gone;
        assert!(gone(404, fcm_error(404, "UNREGISTERED")));
        assert!(gone(400, fcm_error(400, "INVALID_ARGUMENT")));
        assert!(gone(403, fcm_error(403, "SENDER_ID_MISMATCH")));
        assert!(gone(400, bad_field("message.token")));
        // A mistake in the message itself: the phones stay.
        assert_eq!(
            refusal(400, bad_field("message.android.ttl").as_bytes()),
            SendError::Failed("FCM status 400 INVALID_ARGUMENT".to_owned())
        );
        assert_eq!(
            refusal(401, fcm_error(401, "UNAUTHENTICATED").as_bytes()),
            SendError::Busy("FCM status 401 UNAUTHENTICATED".to_owned())
        );
        assert!(matches!(
            refusal(503, fcm_error(503, "UNAVAILABLE").as_bytes()),
            SendError::Busy(_)
        ));
        assert!(matches!(
            refusal(429, fcm_error(429, "QUOTA_EXCEEDED").as_bytes()),
            SendError::Busy(_)
        ));
        assert_eq!(
            refusal(403, b"not json"),
            SendError::Failed("FCM status 403 ".to_owned())
        );
    }

    #[test]
    fn a_service_account_without_an_rsa_key_is_refused_without_showing_it() {
        for bad in [
            config("-----BEGIN PRIVATE KEY-----\nc2VjcmV0\n-----END PRIVATE KEY-----"),
            FcmConfig {
                service_account: orbit_platform::Secret::new("secret-value"),
            },
        ] {
            let error = FcmClient::new(&bad).err().unwrap();
            assert!(
                !error.contains("c2VjcmV0") && !error.contains("secret-value"),
                "{error}"
            );
        }
        let mut wrong_project: Value =
            serde_json::from_str(config(TEST_KEY).service_account.expose()).unwrap();
        wrong_project["project_id"] = json!("../other");
        let wrong_project = FcmConfig {
            service_account: orbit_platform::Secret::new(wrong_project.to_string()),
        };
        assert!(FcmClient::new(&wrong_project).is_err());
        assert!(FcmClient::new(&config(TEST_KEY)).is_ok());
    }

    /// What the fake Google got: the form of each token request, and the access token and body
    /// of each send.
    #[derive(Default)]
    struct Google {
        token_requests: Mutex<Vec<String>>,
        sends: Mutex<Vec<(String, Value)>>,
        /// The answers to the sends, first to last; then 200.
        answers: Mutex<Vec<(u16, Value)>>,
    }

    async fn fake_google() -> (Arc<Google>, String) {
        let google = Arc::new(Google::default());
        let app = axum::Router::new()
            .route(
                "/token",
                post(
                    |State(google): State<Arc<Google>>, form: String| async move {
                        let mut requests = google.token_requests.lock().unwrap();
                        requests.push(form);
                        axum::Json(json!({
                            "access_token": format!("access-{}", requests.len()),
                            "expires_in": 3599,
                            "token_type": "Bearer",
                        }))
                    },
                ),
            )
            .route(
                "/v1/projects/orbit-test/messages:send",
                post(
                    |State(google): State<Arc<Google>>,
                     headers: HeaderMap,
                     axum::Json(body): axum::Json<Value>| async move {
                        let bearer = headers["authorization"].to_str().unwrap().to_owned();
                        google.sends.lock().unwrap().push((bearer, body));
                        let mut answers = google.answers.lock().unwrap();
                        let (status, answer) = if answers.is_empty() {
                            (200, json!({ "name": "projects/orbit-test/messages/1" }))
                        } else {
                            answers.remove(0)
                        };
                        (StatusCode::from_u16(status).unwrap(), axum::Json(answer))
                    },
                ),
            )
            .with_state(Arc::clone(&google));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (google, origin)
    }

    #[tokio::test]
    async fn the_client_signs_in_with_the_service_account_keeps_the_token_and_sends() {
        let (google, origin) = fake_google().await;
        let client = FcmClient::with_endpoints(
            &config(TEST_KEY),
            Endpoints {
                token: format!("{origin}/token"),
                fcm: origin.clone(),
            },
        )
        .unwrap();
        let device = |token: &str| FcmDevice {
            id: Id::new_v7(),
            token: token.to_owned(),
        };
        let message = message(&notice());
        client
            .send(device("phone-1"), message.clone())
            .await
            .unwrap();
        client
            .send(device("phone-2"), message.clone())
            .await
            .unwrap();

        // One access token for both pushes, from a JWT signed with the key.
        let requests = google.token_requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        let form: std::collections::HashMap<String, String> =
            url::form_urlencoded::parse(requests[0].as_bytes())
                .into_owned()
                .collect();
        assert_eq!(
            form["grant_type"],
            "urn:ietf:params:oauth:grant-type:jwt-bearer"
        );
        let parts: Vec<&str> = form["assertion"].split('.').collect();
        let part = |index: usize| -> Value {
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[index]).unwrap()).unwrap()
        };
        assert_eq!(part(0), json!({ "alg": "RS256", "typ": "JWT" }));
        let claims = part(1);
        assert_eq!(claims["iss"], "push@orbit-test.iam.gserviceaccount.com");
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["aud"], format!("{origin}/token"));
        assert_eq!(
            claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
            3600
        );
        let public = client.credentials.key.public().as_ref().to_vec();
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
            )
            .unwrap();

        let sends = std::mem::take(&mut *google.sends.lock().unwrap());
        assert_eq!(sends.len(), 2);
        assert_eq!(sends[0].0, "Bearer access-1");
        assert_eq!(sends[0].1, message.body("phone-1"));
        assert_eq!(sends[1].1["message"]["token"], "phone-2");

        // A dead token is gone; a refused access token is made again for the next push.
        google.answers.lock().unwrap().extend([
            (
                404,
                json!({ "error": { "code": 404, "status": "NOT_FOUND", "details": [
                    { "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError",
                      "errorCode": "UNREGISTERED" }] } }),
            ),
            (
                401,
                json!({ "error": { "code": 401, "status": "UNAUTHENTICATED" } }),
            ),
        ]);
        assert_eq!(
            client.send(device("dead"), message.clone()).await,
            Err(SendError::Gone)
        );
        assert!(matches!(
            client.send(device("phone-1"), message.clone()).await,
            Err(SendError::Busy(_))
        ));
        client.send(device("phone-1"), message).await.unwrap();
        assert_eq!(google.token_requests.lock().unwrap().len(), 2);
        let sends = google.sends.lock().unwrap();
        assert_eq!(sends.last().unwrap().0, "Bearer access-2");
    }
}
