//! The HTTP adapters: Mailgun (`messages.mime`), Amazon SES (`outbound-emails` with
//! `Content.Raw`, SigV4) and Cloudflare (`send_raw`). Each takes the signed message as it is.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::Utc;
use futures_util::future::BoxFuture;
use orbit_platform::{PublicResolver, SigV4Credentials, SigV4Request, sigv4_authorization};
use rand_core::{OsRng, RngCore};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{RequestBuilder, StatusCode, Url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Delivery, DeliveryContext, Outgoing, RecipientOutcome, all};
use crate::repositories::mail::{RouteSecrets, RouteSettings, SendRouteKind};

/// One API call, the message upload included.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Most of a response body read.
const BODY_MAX_BYTES: usize = 64 * 1024;
/// Recipients in one call: SES and Cloudflare take at most 50; Mailgun 1,000.
const SES_BATCH: usize = 50;
const CLOUDFLARE_BATCH: usize = 50;
const MAILGUN_BATCH: usize = 1000;

/// The client for the HTTP adapters: no redirects, and the name resolved at connect time to
/// public addresses only, unless `allow_private` (an instance route, which may name its own base
/// URL on a private host).
#[must_use]
pub fn delivery_http_client(allow_private: bool) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .user_agent("Orbit");
    if !allow_private {
        builder = builder.dns_resolver(Arc::new(PublicResolver));
    }
    builder.build().expect("the delivery HTTP client builds")
}

/// The API base URL (no trailing slash) of an HTTP route: the provider's, or the route's own
/// `base_url` when `allow_private` (only an instance route may have one). `Err`: a setting the
/// kind needs is missing or not valid.
pub fn api_base(
    kind: SendRouteKind,
    settings: &RouteSettings,
    allow_private: bool,
) -> Result<String, String> {
    let provider = match kind {
        SendRouteKind::Mailgun => match settings.region.as_deref().unwrap_or("us") {
            "us" => "https://api.mailgun.net".to_owned(),
            "eu" => "https://api.eu.mailgun.net".to_owned(),
            "au" => "https://api.au.mailgun.net".to_owned(),
            _ => return Err("The route's Mailgun region is not valid.".to_owned()),
        },
        SendRouteKind::Ses => {
            let region = settings
                .region
                .as_deref()
                .filter(|region| host_label(region))
                .ok_or_else(|| "The route has no valid AWS region.".to_owned())?;
            format!("https://email.{region}.amazonaws.com")
        }
        SendRouteKind::Cloudflare => "https://api.cloudflare.com/client/v4".to_owned(),
        _ => return Err("The route does not send through an HTTP API.".to_owned()),
    };
    Ok(settings
        .base_url
        .as_deref()
        .filter(|_| allow_private)
        .map_or(provider, |url| url.trim_end_matches('/').to_owned()))
}

/// Who answered, for the error texts and the error rules.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Provider {
    Mailgun,
    Ses,
    Cloudflare,
}

impl Provider {
    const fn name(self) -> &'static str {
        match self {
            Self::Mailgun => "Mailgun",
            Self::Ses => "Amazon SES",
            Self::Cloudflare => "Cloudflare",
        }
    }
}

enum Auth {
    Mailgun {
        api_key: String,
    },
    Ses {
        region: String,
        access_key_id: String,
        secret_access_key: String,
    },
    Cloudflare {
        account_id: String,
        api_token: String,
    },
}

pub(super) struct Api {
    http: reqwest::Client,
    /// Without a trailing slash.
    base: String,
    auth: Auth,
}

/// A host label of an AWS region (it goes into the host name).
fn host_label(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Api {
    /// `Err`: the route lacks a setting it needs (every recipient is deferred).
    pub(super) fn new(
        kind: SendRouteKind,
        settings: &RouteSettings,
        secrets: &RouteSecrets,
        ctx: &DeliveryContext,
    ) -> Result<Self, String> {
        let base = api_base(kind, settings, ctx.allow_private)?;
        let auth = match (kind, secrets) {
            (SendRouteKind::Mailgun, RouteSecrets::Mailgun { api_key, .. }) => Auth::Mailgun {
                api_key: api_key.clone(),
            },
            (
                SendRouteKind::Ses,
                RouteSecrets::Ses {
                    access_key_id,
                    secret_access_key,
                },
            ) => {
                let region = settings
                    .region
                    .clone()
                    .filter(|region| host_label(region))
                    .ok_or_else(|| "The route has no valid AWS region.".to_owned())?;
                Auth::Ses {
                    region,
                    access_key_id: access_key_id.clone(),
                    secret_access_key: secret_access_key.clone(),
                }
            }
            (SendRouteKind::Cloudflare, RouteSecrets::Cloudflare { api_token, .. }) => {
                let account_id = settings
                    .account_id
                    .clone()
                    .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| "The route has no valid Cloudflare account ID.".to_owned())?;
                Auth::Cloudflare {
                    account_id,
                    api_token: api_token.clone(),
                }
            }
            _ => return Err("The route's secrets do not match its kind.".to_owned()),
        };
        Ok(Self {
            http: ctx.http.clone(),
            base,
            auth,
        })
    }

    async fn mailgun(
        &self,
        api_key: &str,
        outgoing: &Outgoing<'_>,
    ) -> Vec<(String, RecipientOutcome)> {
        // Mailgun sends for the From domain; its URL takes the A-label form.
        let Ok(domain) = orbit_mail::normalize_domain(outgoing.from_domain) else {
            return all(
                outgoing.recipients,
                &RecipientOutcome::route_error(format!(
                    "Route error: the From domain {:?} is not a domain Mailgun can send for",
                    outgoing.from_domain
                )),
            );
        };
        let url = format!("{}/v3/{domain}/messages.mime", self.base);
        let mut outcomes = Vec::with_capacity(outgoing.recipients.len());
        for batch in outgoing.recipients.chunks(MAILGUN_BATCH) {
            let (boundary, body) = multipart(batch, outgoing.raw);
            let request = self
                .http
                .post(&url)
                .basic_auth("api", Some(api_key))
                .header(
                    CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body);
            let outcome = match call(request).await {
                Ok((status, body)) if status.is_success() => RecipientOutcome::Accepted {
                    provider_message_id: json_field(&body, &["id"]),
                },
                Ok((status, body)) => failure(Provider::Mailgun, status, &body),
                Err(error) => RecipientOutcome::deferred(error),
            };
            outcomes.extend(all(batch, &outcome));
        }
        outcomes
    }

    async fn ses(
        &self,
        region: &str,
        credentials: SigV4Credentials<'_>,
        outgoing: &Outgoing<'_>,
    ) -> Vec<(String, RecipientOutcome)> {
        let url = match Url::parse(&format!("{}/v2/email/outbound-emails", self.base)) {
            Ok(url) => url,
            Err(error) => {
                return all(
                    outgoing.recipients,
                    &RecipientOutcome::route_error(format!("The SES URL is not valid: {error}")),
                );
            }
        };
        let Some(host) = url.host_str().map(|host| match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        }) else {
            return all(
                outgoing.recipients,
                &RecipientOutcome::route_error("The SES URL has no host"),
            );
        };
        let data = BASE64.encode(outgoing.raw);
        let mut outcomes = Vec::with_capacity(outgoing.recipients.len());
        for batch in outgoing.recipients.chunks(SES_BATCH) {
            let body = json!({
                "Destination": { "ToAddresses": batch },
                "Content": { "Raw": { "Data": data } },
            })
            .to_string();
            let payload_hash = format!("{:x}", Sha256::digest(body.as_bytes()));
            let mut headers = vec![
                ("host".to_owned(), host.clone()),
                ("content-type".to_owned(), "application/json".to_owned()),
            ];
            let authorization = sigv4_authorization(
                "ses",
                region,
                &credentials,
                &SigV4Request {
                    method: "POST",
                    path: url.path(),
                    query: "",
                    payload_hash: &payload_hash,
                    now: Utc::now(),
                },
                &mut headers,
            );
            let mut request = self.http.post(url.clone());
            for (name, value) in headers {
                // reqwest sends the same Host itself.
                if name != "host" {
                    request = request.header(name, value);
                }
            }
            let request = request.header(AUTHORIZATION, authorization).body(body);
            let outcome = match call(request).await {
                Ok((status, body)) if status.is_success() => RecipientOutcome::Accepted {
                    provider_message_id: json_field(&body, &["MessageId"]),
                },
                Ok((status, body)) => failure(Provider::Ses, status, &body),
                Err(error) => RecipientOutcome::deferred(error),
            };
            outcomes.extend(all(batch, &outcome));
        }
        outcomes
    }

    async fn cloudflare(
        &self,
        account_id: &str,
        api_token: &str,
        outgoing: &Outgoing<'_>,
    ) -> Vec<(String, RecipientOutcome)> {
        let Ok(mime_message) = std::str::from_utf8(outgoing.raw) else {
            return all(
                outgoing.recipients,
                &RecipientOutcome::rejected("5.6.0 Cloudflare takes only UTF-8 messages"),
            );
        };
        let url = format!("{}/accounts/{account_id}/email/sending/send_raw", self.base);
        let mut outcomes = Vec::with_capacity(outgoing.recipients.len());
        for batch in outgoing.recipients.chunks(CLOUDFLARE_BATCH) {
            let request = self.http.post(&url).bearer_auth(api_token).json(&json!({
                "from": outgoing.mail_from,
                "recipients": batch,
                "mime_message": mime_message,
            }));
            match call(request).await {
                Ok((status, body)) if status.is_success() => {
                    outcomes.extend(cloudflare_outcomes(batch, &body));
                }
                Ok((status, body)) => {
                    outcomes.extend(all(batch, &failure(Provider::Cloudflare, status, &body)))
                }
                Err(error) => outcomes.extend(all(batch, &RecipientOutcome::deferred(error))),
            }
        }
        outcomes
    }
}

impl Delivery for Api {
    fn deliver<'a>(
        &'a self,
        outgoing: &'a Outgoing<'a>,
    ) -> BoxFuture<'a, Vec<(String, RecipientOutcome)>> {
        Box::pin(async move {
            match &self.auth {
                Auth::Mailgun { api_key } => self.mailgun(api_key, outgoing).await,
                Auth::Ses {
                    region,
                    access_key_id,
                    secret_access_key,
                } => {
                    let credentials = SigV4Credentials {
                        access_key_id,
                        secret_access_key,
                    };
                    self.ses(region, credentials, outgoing).await
                }
                Auth::Cloudflare {
                    account_id,
                    api_token,
                } => self.cloudflare(account_id, api_token, outgoing).await,
            }
        })
    }
}

/// A `multipart/form-data` body with a `to` field for each recipient and the message as the
/// `message` file. The boundary is random and not in the message.
fn multipart(recipients: &[String], raw: &[u8]) -> (String, Vec<u8>) {
    let boundary = loop {
        let mut bytes = [0_u8; 16];
        OsRng.fill_bytes(&mut bytes);
        let boundary: String = std::iter::once("orbit-".to_owned())
            .chain(bytes.iter().map(|byte| format!("{byte:02x}")))
            .collect();
        if !raw
            .windows(boundary.len())
            .any(|window| window == boundary.as_bytes())
        {
            break boundary;
        }
    };
    let mut body = Vec::with_capacity(raw.len() + 256 + recipients.len() * 96);
    for recipient in recipients {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"to\"\r\n\r\n{recipient}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message\"; \
             filename=\"message.mime\"\r\nContent-Type: message/rfc822\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(raw);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

/// Sends the request: the status and the start of the body, or why there is no response.
async fn call(request: RequestBuilder) -> Result<(StatusCode, String), String> {
    let mut response = request
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "The provider did not answer in time".to_owned()
            } else {
                format!("The provider could not be reached: {error}")
            }
        })?;
    let status = response.status();
    // SES names the error in a header; keep it with the body.
    let mut body: Vec<u8> = response
        .headers()
        .get("x-amzn-errortype")
        .and_then(|value| value.to_str().ok())
        .map(|kind| format!("{kind}: ").into_bytes())
        .unwrap_or_default();
    while body.len() < BODY_MAX_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) | Err(_) => break,
        }
    }
    body.truncate(BODY_MAX_BYTES);
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// SES error types (`x-amzn-ErrorType`) and messages that mean the route, not the recipient, is
/// at fault: an unverified identity (also the sandbox's unverified recipients), a paused or
/// suspended account, a missing configuration, throttling.
const SES_ROUTE_ERRORS: [&str; 12] = [
    "not verified",
    "mailfromdomainnotverified",
    "sendingpaused",
    "accountsendingpaused",
    "accountsuspended",
    "notfoundexception",
    "throttling",
    "toomanyrequests",
    "limitexceeded",
    "accessdenied",
    "signaturedoesnotmatch",
    "unrecognizedclient",
];

/// The outcome of a call that failed with `status`. Only an answer about the recipients or the
/// message bounces; anything about the route (credentials, an unknown domain or account, a
/// paused account, rate limits) or the provider (server errors, timeouts) defers, so the message
/// waits for the admin's fix and nothing feeds the suppression list or the pause.
fn failure(provider: Provider, status: StatusCode, body: &str) -> RecipientOutcome {
    let code = status.as_u16();
    let body = body.trim();
    let name = provider.name();
    let route_error = |what: &str| {
        RecipientOutcome::route_error(format!("Route error: {name} {what} (HTTP {code}): {body}"))
    };
    let lower = body.to_ascii_lowercase();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            route_error("refused the route's credentials")
        }
        StatusCode::NOT_FOUND => route_error("does not know the route's domain or account"),
        StatusCode::PAYMENT_REQUIRED => route_error("refused the account"),
        _ if provider == Provider::Ses
            && status.is_client_error()
            && SES_ROUTE_ERRORS.iter().any(|needle| lower.contains(needle)) =>
        {
            route_error("refused to send for the route")
        }
        _ if status.is_client_error()
            && status != StatusCode::TOO_MANY_REQUESTS
            && status != StatusCode::REQUEST_TIMEOUT =>
        {
            RecipientOutcome::bounced(format!("{name} refused the message (HTTP {code}): {body}"))
        }
        _ => RecipientOutcome::deferred(format!("{name} answered HTTP {code}: {body}")),
    }
}

fn json_field(body: &str, path: &[&str]) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let mut current = &value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str().map(str::to_owned)
}

/// Cloudflare reports each recipient: delivered or queued (accepted), a permanent bounce, or on
/// its suppression list. A recipient it does not list was accepted with the call.
fn cloudflare_outcomes(batch: &[String], body: &str) -> Vec<(String, RecipientOutcome)> {
    let value: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if value.get("success") == Some(&Value::Bool(false)) {
        let errors = value
            .get("errors")
            .map(Value::to_string)
            .unwrap_or_default();
        return all(
            batch,
            &RecipientOutcome::deferred(format!("Cloudflare: {errors}")),
        );
    }
    let result = value.get("result").unwrap_or(&Value::Null);
    let listed = |key: &str, address: &str| {
        result
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .any(|item| item.eq_ignore_ascii_case(address))
            })
    };
    let message_id = result
        .get("message_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    batch
        .iter()
        .map(|address| {
            let outcome = if listed("permanent_bounces", address) {
                RecipientOutcome::bounced("5.0.0 Cloudflare: permanent bounce")
            } else if listed("suppressed_recipients", address) {
                RecipientOutcome::bounced(
                    "5.7.1 Cloudflare: the address is on its suppression list",
                )
            } else {
                RecipientOutcome::Accepted {
                    provider_message_id: message_id.clone(),
                }
            };
            (address.clone(), outcome)
        })
        .collect()
}
