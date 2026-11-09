//! Native iOS pushes through Apple's push service (APNs): the payload, the provider token, and
//! [`ApnsClient`], which sends over HTTP/2 to `/3/device/<token>`.
//!
//! The provider token is a JWT (ES256) signed with the team's `.p8` key. APNs takes one for an
//! hour and refuses tokens that change more often than every 20 minutes, so one is kept for
//! [`TOKEN_LIFETIME`] and made again early only when APNs says it expired.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use orbit_platform::{ApnsConfig, Id, TimestampMillis};
use p256::SecretKey;
use p256::pkcs8::DecodePrivateKey;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use super::{Notice, SendError, webpush};

/// The largest payload APNs takes for an alert.
pub const MAX_PAYLOAD_BYTES: usize = 4096;
/// The longest `apns-collapse-id` APNs takes.
const COLLAPSE_ID_BYTES: usize = 64;
/// How long a provider token is used: within the 20 to 60 minutes APNs asks for.
const TOKEN_LIFETIME: Duration = Duration::from_secs(40 * 60);
/// How long APNs keeps a push for a phone that is not reachable.
const EXPIRATION_SECONDS: i64 = 24 * 60 * 60;
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const PRODUCTION_HOST: &str = "https://api.push.apple.com";
const SANDBOX_HOST: &str = "https://api.sandbox.push.apple.com";

/// Which APNs a device token belongs to: `sandbox` for development builds, `production` for the
/// App Store and TestFlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ApnsEnvironment {
    Production,
    Sandbox,
}

impl ApnsEnvironment {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Sandbox => "sandbox",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "production" => Some(Self::Production),
            "sandbox" => Some(Self::Sandbox),
            _ => None,
        }
    }

    const fn host(self) -> &'static str {
        match self {
            Self::Production => PRODUCTION_HOST,
            Self::Sandbox => SANDBOX_HOST,
        }
    }
}

/// A phone the user gets pushes on.
#[derive(Clone, Debug)]
pub struct ApnsDevice {
    pub id: Id,
    /// Lowercase hex.
    pub token: String,
    pub environment: ApnsEnvironment,
}

/// What goes to every phone of a notice.
#[derive(Clone, Debug)]
pub struct ApnsMessage {
    pub payload: Vec<u8>,
    pub collapse_id: String,
}

pub trait ApnsSender: Send + Sync {
    fn send(
        &self,
        device: ApnsDevice,
        message: ApnsMessage,
    ) -> BoxFuture<'static, Result<(), SendError>>;
}

/// The device token as it is stored (lowercase hex), or `None` when it is no APNs token. Tokens
/// are 32 bytes today; Apple may make them longer.
#[must_use]
pub fn parse_token(token: &str) -> Option<String> {
    let token = token.trim();
    let fits = (16..=200).contains(&token.len())
        && token.len().is_multiple_of(2)
        && token.bytes().all(|byte| byte.is_ascii_hexdigit());
    fits.then(|| token.to_ascii_lowercase())
}

/// The message of a clamped notice. Notices with the same tag replace each other on the phone,
/// as in a browser, and group in one thread.
#[must_use]
pub fn message(notice: &Notice) -> ApnsMessage {
    let payload = json!({
        "aps": {
            "alert": { "title": notice.title, "body": notice.body },
            "sound": "default",
            "thread-id": notice.tag,
        },
        "url": notice.url,
    });
    ApnsMessage {
        payload: serde_json::to_vec(&payload).unwrap_or_default(),
        collapse_id: truncate(&notice.tag, COLLAPSE_ID_BYTES).to_owned(),
    }
}

/// The longest start of `text` of at most `bytes` bytes that ends at a character boundary.
fn truncate(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Sends to APNs with the team's key. Clones share the HTTP/2 connections and the token.
#[derive(Clone)]
pub struct ApnsClient {
    client: reqwest::Client,
    credentials: Arc<Credentials>,
}

struct Credentials {
    key: SecretKey,
    key_id: String,
    team_id: String,
    topic: String,
    /// The provider token and when it was made.
    token: Mutex<Option<(String, Instant)>>,
}

impl Credentials {
    fn token(&self) -> String {
        let mut token = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((token, made)) = token.as_ref()
            && made.elapsed() < TOKEN_LIFETIME
        {
            return token.clone();
        }
        let fresh = webpush::es256_jwt(
            &self.key,
            &json!({ "alg": "ES256", "kid": self.key_id }),
            &json!({ "iss": self.team_id, "iat": TimestampMillis::now().as_millis() / 1000 }),
        );
        *token = Some((fresh.clone(), Instant::now()));
        fresh
    }

    /// APNs said `token` expired: the next push makes another one. Pushes that were sent with the
    /// same token at the same time do not each make one.
    fn expired(&self, expired: &str) {
        let mut token = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        if token.as_ref().is_some_and(|(token, _)| token == expired) {
            *token = None;
        }
    }
}

impl ApnsClient {
    /// A client for this key; an error when the key is not a PKCS#8 P-256 key in PEM.
    pub fn new(config: &ApnsConfig) -> Result<Self, String> {
        let key = SecretKey::from_pkcs8_pem(config.key.expose().trim())
            .map_err(|_| "ORBIT_APNS_KEY is not a .p8 key (PKCS#8 P-256 PEM)".to_owned())?;
        // APNs speaks HTTP/2 only; TLS negotiates it (ALPN `h2`).
        let client = reqwest::Client::builder()
            .http2_prior_knowledge()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("the HTTP client for APNs: {error}"))?;
        Ok(Self {
            client,
            credentials: Arc::new(Credentials {
                key,
                key_id: config.key_id.clone(),
                team_id: config.team_id.clone(),
                topic: config.topic.clone(),
                token: Mutex::new(None),
            }),
        })
    }
}

#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    reason: String,
}

impl ApnsSender for ApnsClient {
    fn send(
        &self,
        device: ApnsDevice,
        message: ApnsMessage,
    ) -> BoxFuture<'static, Result<(), SendError>> {
        let (client, credentials) = (self.client.clone(), Arc::clone(&self.credentials));
        Box::pin(async move {
            let token = credentials.token();
            let expiration = TimestampMillis::now().as_millis() / 1000 + EXPIRATION_SECONDS;
            let mut request = client
                .post(format!(
                    "{}/3/device/{}",
                    device.environment.host(),
                    device.token
                ))
                .header("authorization", format!("bearer {token}"))
                .header("apns-topic", &credentials.topic)
                .header("apns-push-type", "alert")
                .header("apns-priority", "10")
                .header("apns-expiration", expiration.to_string())
                .header("content-type", "application/json")
                .body(message.payload)
                .timeout(SEND_TIMEOUT);
            if !message.collapse_id.is_empty() {
                request = request.header("apns-collapse-id", message.collapse_id);
            }
            let response = request
                .send()
                .await
                .map_err(|_| SendError::Busy("APNs did not answer".to_owned()))?;
            let status = response.status().as_u16();
            if status == 200 {
                return Ok(());
            }
            let reason = response
                .json::<Refusal>()
                .await
                .map(|refusal| refusal.reason)
                .unwrap_or_default();
            let refused = format!("APNs status {status} {reason}");
            match (status, reason.as_str()) {
                (410, _) | (400, "BadDeviceToken" | "DeviceTokenNotForTopic") => {
                    Err(SendError::Gone)
                }
                // The same push goes again with a new token.
                (403, "ExpiredProviderToken") => {
                    credentials.expired(&token);
                    Err(SendError::Busy(refused))
                }
                (429 | 500..=599, _) => Err(SendError::Busy(refused)),
                _ => Err(SendError::Failed(refused)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::{Signature, VerifyingKey};
    use p256::elliptic_curve::Generate;
    use p256::pkcs8::{EncodePrivateKey, LineEnding};

    use super::*;

    /// Needs the network: `cargo test -p orbit-server --lib apns -- --ignored`.
    #[tokio::test]
    #[ignore = "reaches api.sandbox.push.apple.com"]
    async fn the_client_speaks_http2_with_apns() {
        let pem = SecretKey::generate().to_pkcs8_pem(LineEnding::LF).unwrap();
        let client = ApnsClient::new(&ApnsConfig {
            key: orbit_platform::Secret::new(pem.as_str()),
            key_id: "KEY1234567".to_owned(),
            team_id: "TEAM123456".to_owned(),
            topic: "io.coollabs.orbit".to_owned(),
        })
        .unwrap();
        let response = client
            .client
            .post(format!("{SANDBOX_HOST}/3/device/{}", "00".repeat(32)))
            .header(
                "authorization",
                format!("bearer {}", client.credentials.token()),
            )
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.version(), reqwest::Version::HTTP_2);
        // A key that Apple does not know.
        assert_eq!(response.status().as_u16(), 403);
        let device = ApnsDevice {
            id: Id::new_v7(),
            token: "00".repeat(32),
            environment: ApnsEnvironment::Sandbox,
        };
        let result = client
            .send(
                device,
                message(&Notice {
                    title: "t".to_owned(),
                    body: "b".to_owned(),
                    url: "/".to_owned(),
                    tag: "test".to_owned(),
                    sound: "message",
                    icon: None,
                }),
            )
            .await;
        assert!(
            matches!(&result, Err(SendError::Failed(reason)) if reason.contains("InvalidProviderToken")),
            "{result:?}"
        );
    }

    #[test]
    fn tokens_are_hex_of_a_sane_length() {
        let token = "AB".repeat(32);
        assert_eq!(parse_token(&format!(" {token} ")), Some("ab".repeat(32)));
        assert_eq!(parse_token("abc"), None);
        assert_eq!(parse_token(&"a".repeat(65)), None);
        assert_eq!(parse_token(&"zz".repeat(32)), None);
        assert_eq!(parse_token(&"ab".repeat(101)), None);
    }

    #[test]
    fn the_payload_shows_the_notice_and_the_collapse_id_fits() {
        let notice = Notice {
            title: "Ada in #general".to_owned(),
            body: "Hello".to_owned(),
            url: "/chat/1".to_owned(),
            tag: format!("chat:{}é", "x".repeat(62)),
            sound: "message",
            icon: None,
        };
        let message = message(&notice);
        let payload: serde_json::Value = serde_json::from_slice(&message.payload).unwrap();
        assert_eq!(
            payload,
            json!({
                "aps": {
                    "alert": { "title": "Ada in #general", "body": "Hello" },
                    "sound": "default",
                    "thread-id": notice.tag,
                },
                "url": "/chat/1",
            })
        );
        // Cut at 64 bytes, without splitting a character.
        assert_eq!(message.collapse_id, format!("chat:{}", "x".repeat(59)));
        assert!(message.collapse_id.len() <= 64);
    }

    #[test]
    fn the_provider_token_names_the_key_and_the_team_and_is_kept() {
        let key = SecretKey::generate();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let client = ApnsClient::new(&ApnsConfig {
            key: orbit_platform::Secret::new(pem.as_str()),
            key_id: "KEY1234567".to_owned(),
            team_id: "TEAM123456".to_owned(),
            topic: "io.coollabs.orbit".to_owned(),
        })
        .unwrap();
        let token = client.credentials.token();
        assert_eq!(client.credentials.token(), token);
        let parts: Vec<&str> = token.split('.').collect();
        let part = |index: usize| -> serde_json::Value {
            serde_json::from_slice(&webpush::decode(parts[index]).unwrap()).unwrap()
        };
        assert_eq!(part(0), json!({ "alg": "ES256", "kid": "KEY1234567" }));
        assert_eq!(part(1)["iss"], "TEAM123456");
        let signature = Signature::from_slice(&webpush::decode(parts[2]).unwrap()).unwrap();
        VerifyingKey::from(key.public_key())
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
        client.credentials.expired("another token");
        assert_eq!(client.credentials.token(), token);
        client.credentials.expired(&token);
        assert!(client.credentials.token.lock().unwrap().is_none());

        let error = ApnsClient::new(&ApnsConfig {
            key: orbit_platform::Secret::new("not a key"),
            key_id: String::new(),
            team_id: String::new(),
            topic: String::new(),
        });
        assert!(error.is_err());
    }
}
