//! The few Stripe REST calls billing needs (form-encoded requests, JSON answers) and webhook
//! signature checks. Shapes follow the pinned [`API_VERSION`].

use std::collections::HashMap;
use std::time::Duration;

use hmac::{Hmac, Mac};
use orbit_platform::{BillingConfig, Id, Secret};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::Sha256;
use url::Url;

/// Every request names its API version, so the account's default version cannot change the
/// shapes below. Since 2025-03-31 the billing period lives on subscription items.
pub const API_VERSION: &str = "2026-09-30.endive";
/// How old a webhook signature may be (Stripe's libraries use the same tolerance).
const SIGNATURE_TOLERANCE_SECONDS: i64 = 5 * 60;

#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("Stripe could not be reached: {0}")]
    Unreachable(String),
    #[error("Stripe answered {status}: {message}")]
    Api { status: u16, message: String },
    #[error("Stripe sent an unexpected answer: {0}")]
    Decode(String),
}

#[derive(Clone)]
pub struct StripeClient {
    http: reqwest::Client,
    api_base: Url,
    secret_key: Secret,
}

/// A subscription as `GET /v1/subscriptions/{id}` returns it, reduced to what Orbit stores.
#[derive(Clone, Debug, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub customer: String,
    pub status: String,
    #[serde(default)]
    pub cancel_at_period_end: bool,
    /// A cancellation date; the Billing Portal sets it instead of `cancel_at_period_end`.
    pub cancel_at: Option<i64>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub items: SubscriptionItems,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SubscriptionItems {
    pub data: Vec<SubscriptionItem>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SubscriptionItem {
    pub price: Price,
    pub current_period_end: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Price {
    pub id: String,
}

impl Subscription {
    /// When the subscription renews or, when it is set to cancel, ends; unix seconds.
    #[must_use]
    pub fn period_end(&self) -> Option<i64> {
        self.cancel_at.or_else(|| {
            self.items
                .data
                .iter()
                .filter_map(|item| item.current_period_end)
                .min()
        })
    }

    #[must_use]
    pub fn cancels(&self) -> bool {
        self.cancel_at_period_end || self.cancel_at.is_some()
    }

    pub fn price_ids(&self) -> impl Iterator<Item = &str> {
        self.items.data.iter().map(|item| item.price.id.as_str())
    }
}

/// A price as `GET /v1/prices/{id}?expand[]=product` returns it, reduced to what plans show.
#[derive(Clone, Debug, Deserialize)]
pub struct PriceDetails {
    /// Minor units; `None` for prices that are not a fixed amount.
    pub unit_amount: Option<i64>,
    pub currency: String,
    pub recurring: Option<Recurring>,
    pub product: Product,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Recurring {
    /// `day`, `week`, `month` or `year`.
    pub interval: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Product {
    pub id: String,
    pub name: String,
}

/// A list as `GET /v1/{collection}` returns it.
#[derive(Deserialize)]
struct List<T> {
    data: Vec<T>,
}

/// A Checkout Session, reduced to its id.
#[derive(Deserialize)]
struct Session {
    id: String,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Deserialize)]
struct Redirect {
    url: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: Option<String>,
}

/// What a Checkout Session needs.
pub struct CheckoutRequest<'a> {
    pub customer_id: &'a str,
    pub price_id: &'a str,
    pub workspace_id: Id,
    pub success_url: &'a str,
    pub cancel_url: &'a str,
}

impl StripeClient {
    #[must_use]
    pub fn new(config: &BillingConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("the Stripe HTTP client builds"),
            api_base: config.api_base.clone(),
            secret_key: config.secret_key.clone(),
        }
    }

    /// The customer of a workspace. The idempotency key makes a retried or concurrent first
    /// checkout get the same customer back (Stripe keeps keys for 24 hours).
    pub async fn create_customer(
        &self,
        workspace_id: Id,
        name: &str,
        email: &str,
    ) -> Result<String, StripeError> {
        let workspace = workspace_id.to_string();
        let created: Created = self
            .post(
                "/v1/customers",
                &[
                    ("name", name),
                    ("email", email),
                    ("metadata[workspace_id]", &workspace),
                ],
                Some(&format!("orbit-workspace-{workspace}-customer")),
            )
            .await?;
        Ok(created.id)
    }

    /// A hosted Checkout Session for one subscription of quantity 1; returns its URL.
    pub async fn create_checkout_session(
        &self,
        request: &CheckoutRequest<'_>,
    ) -> Result<String, StripeError> {
        let workspace = request.workspace_id.to_string();
        let session: Redirect = self
            .post(
                "/v1/checkout/sessions",
                &[
                    ("mode", "subscription"),
                    ("customer", request.customer_id),
                    ("line_items[0][price]", request.price_id),
                    ("line_items[0][quantity]", "1"),
                    ("client_reference_id", &workspace),
                    ("subscription_data[metadata][workspace_id]", &workspace),
                    // Stripe Tax: Checkout asks for the billing address (and a VAT id) and saves
                    // them on the customer, which has no address before the first checkout.
                    ("automatic_tax[enabled]", "true"),
                    ("customer_update[address]", "auto"),
                    ("customer_update[name]", "auto"),
                    ("tax_id_collection[enabled]", "true"),
                    ("success_url", request.success_url),
                    ("cancel_url", request.cancel_url),
                ],
                None,
            )
            .await?;
        Ok(session.url)
    }

    /// Sets the customer's email (receipts go there). Setting the same value twice is harmless.
    pub async fn update_customer_email(&self, id: &str, email: &str) -> Result<(), StripeError> {
        let _: Created = self
            .post(&format!("/v1/customers/{id}"), &[("email", email)], None)
            .await?;
        Ok(())
    }

    /// A Billing Portal session for the customer; returns its URL.
    pub async fn create_portal_session(
        &self,
        customer_id: &str,
        return_url: &str,
    ) -> Result<String, StripeError> {
        let session: Redirect = self
            .post(
                "/v1/billing_portal/sessions",
                &[("customer", customer_id), ("return_url", return_url)],
                None,
            )
            .await?;
        Ok(session.url)
    }

    pub async fn subscription(&self, id: &str) -> Result<Subscription, StripeError> {
        self.send(reqwest::Method::GET, "/v1/subscriptions", &[id], &[])
            .await
    }

    /// Cancels the subscription now: no proration credit and no final invoice (Stripe's
    /// defaults for `prorate` and `invoice_now`).
    pub async fn cancel_subscription(&self, id: &str) -> Result<Subscription, StripeError> {
        self.send(reqwest::Method::DELETE, "/v1/subscriptions", &[id], &[])
            .await
    }

    /// The customer's subscriptions that are not canceled (up to 100, plenty for one workspace).
    pub async fn customer_subscriptions(
        &self,
        customer_id: &str,
    ) -> Result<Vec<Subscription>, StripeError> {
        let list: List<Subscription> = self
            .send(
                reqwest::Method::GET,
                "/v1/subscriptions",
                &[],
                &[("customer", customer_id), ("limit", "100")],
            )
            .await?;
        Ok(list.data)
    }

    /// Expires every open Checkout Session of the customer, so only the next one can subscribe.
    /// A session that completed or expired meanwhile (Stripe answers 400) is skipped.
    pub async fn expire_open_checkout_sessions(
        &self,
        customer_id: &str,
    ) -> Result<(), StripeError> {
        let open: List<Session> = self
            .send(
                reqwest::Method::GET,
                "/v1/checkout/sessions",
                &[],
                &[
                    ("customer", customer_id),
                    ("status", "open"),
                    ("limit", "100"),
                ],
            )
            .await?;
        for session in open.data {
            match self
                .send::<Session>(
                    reqwest::Method::POST,
                    "/v1/checkout/sessions",
                    &[&session.id, "expire"],
                    &[],
                )
                .await
            {
                Ok(_) | Err(StripeError::Api { status: 400, .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub async fn price(&self, id: &str) -> Result<PriceDetails, StripeError> {
        self.send(
            reqwest::Method::GET,
            "/v1/prices",
            &[id],
            &[("expand[]", "product")],
        )
        .await
    }

    /// `{method} {collection}/{segments…}?{query}`, each segment escaped as one path segment.
    async fn send<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        collection: &str,
        segments: &[&str],
        query: &[(&str, &str)],
    ) -> Result<T, StripeError> {
        let mut url = self.url(collection)?;
        url.path_segments_mut()
            .map_err(|()| StripeError::Decode("the Stripe API base cannot have a path".to_owned()))?
            .pop_if_empty()
            .extend(segments);
        let response = self
            .http
            .request(method, url)
            .query(query)
            .bearer_auth(self.secret_key.expose())
            .header("Stripe-Version", API_VERSION)
            .send()
            .await
            .map_err(|error| StripeError::Unreachable(error.without_url().to_string()))?;
        decode(response).await
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        form: &[(&str, &str)],
        idempotency_key: Option<&str>,
    ) -> Result<T, StripeError> {
        let mut request = self
            .http
            .post(self.url(path)?)
            .bearer_auth(self.secret_key.expose())
            .header("Stripe-Version", API_VERSION)
            .form(form);
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        let response = request
            .send()
            .await
            .map_err(|error| StripeError::Unreachable(error.without_url().to_string()))?;
        decode(response).await
    }

    fn url(&self, path: &str) -> Result<Url, StripeError> {
        self.api_base
            .join(path)
            .map_err(|error| StripeError::Decode(error.to_string()))
    }
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, StripeError> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| StripeError::Unreachable(error.without_url().to_string()))?;
    if !status.is_success() {
        let message = serde_json::from_slice::<ErrorBody>(&bytes)
            .ok()
            .and_then(|body| body.error.message)
            .unwrap_or_else(|| "no error message".to_owned());
        return Err(StripeError::Api {
            status: status.as_u16(),
            message,
        });
    }
    serde_json::from_slice(&bytes).map_err(|error| StripeError::Decode(error.to_string()))
}

#[derive(Debug, Eq, PartialEq)]
pub enum SignatureError {
    Malformed,
    Stale,
    Mismatch,
}

/// Checks a `Stripe-Signature` header (`t=<unix seconds>,v1=<hex>[,v1=…]`): one `v1` must be the
/// HMAC-SHA256 of `"{t}.{body}"` under the endpoint secret, and `t` at most five minutes from
/// `now`. Other schemes (`v0` test signatures) are ignored.
pub fn verify_signature(
    secret: &str,
    header: &str,
    body: &[u8],
    now_seconds: i64,
) -> Result<(), SignatureError> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for element in header.split(',') {
        match element.trim().split_once('=') {
            Some(("t", value)) => {
                timestamp = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| SignatureError::Malformed)?,
                );
            }
            Some(("v1", value)) => {
                if let Some(bytes) = decode_hex(value) {
                    signatures.push(bytes);
                }
            }
            _ => {}
        }
    }
    let timestamp = timestamp.ok_or(SignatureError::Malformed)?;
    if signatures.is_empty() {
        return Err(SignatureError::Malformed);
    }
    let matches = signatures.iter().any(|signature| {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes())
            .expect("HMAC accepts keys of any length");
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        mac.verify_slice(signature).is_ok()
    });
    if !matches {
        return Err(SignatureError::Mismatch);
    }
    if (now_seconds - timestamp).abs() > SIGNATURE_TOLERANCE_SECONDS {
        return Err(SignatureError::Stale);
    }
    Ok(())
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(value.get(index..index + 2)?, 16).ok())
        .collect()
}

/// A valid `Stripe-Signature` header for `body`, for tests and local tools.
#[must_use]
pub fn sign(secret: &str, body: &[u8], timestamp: i64) -> String {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key");
    mac.update(format!("{timestamp}.").as_bytes());
    mac.update(body);
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("t={timestamp},v1={hex}")
}

#[cfg(test)]
mod tests {
    use super::{SignatureError, sign, verify_signature};

    const SECRET: &str = "whsec_test";
    const BODY: &[u8] = br#"{"id":"evt_1","type":"customer.subscription.updated"}"#;
    const NOW: i64 = 1_800_000_000;

    #[test]
    fn a_fresh_signature_from_the_endpoint_secret_is_accepted() {
        let header = sign(SECRET, BODY, NOW - 10);
        assert_eq!(verify_signature(SECRET, &header, BODY, NOW), Ok(()));
    }

    #[test]
    fn a_signature_from_another_secret_or_body_is_rejected() {
        let header = sign("whsec_other", BODY, NOW);
        assert_eq!(
            verify_signature(SECRET, &header, BODY, NOW),
            Err(SignatureError::Mismatch)
        );
        let header = sign(SECRET, BODY, NOW);
        assert_eq!(
            verify_signature(SECRET, &header, b"{}", NOW),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn a_signature_older_than_five_minutes_is_rejected() {
        let header = sign(SECRET, BODY, NOW - 301);
        assert_eq!(
            verify_signature(SECRET, &header, BODY, NOW),
            Err(SignatureError::Stale)
        );
        let header = sign(SECRET, BODY, NOW - 300);
        assert_eq!(verify_signature(SECRET, &header, BODY, NOW), Ok(()));
    }

    #[test]
    fn any_of_several_v1_signatures_may_match() {
        let valid = sign(SECRET, BODY, NOW);
        let old_secret = sign("whsec_rolled", BODY, NOW);
        let old_v1 = old_secret.split_once(",v1=").unwrap().1;
        let valid_v1 = valid.split_once(",v1=").unwrap().1;
        let header = format!("t={NOW},v1={old_v1},v0=abcd,v1={valid_v1}");
        assert_eq!(verify_signature(SECRET, &header, BODY, NOW), Ok(()));
    }

    #[test]
    fn headers_without_a_timestamp_or_v1_signature_are_malformed() {
        let valid = sign(SECRET, BODY, NOW);
        let v1 = valid.split_once(",v1=").unwrap().1;
        for header in [
            "".to_owned(),
            format!("v1={v1}"),
            format!("t={NOW}"),
            format!("t={NOW},v0={v1}"),
            format!("t=soon,v1={v1}"),
        ] {
            assert_eq!(
                verify_signature(SECRET, &header, BODY, NOW),
                Err(SignatureError::Malformed),
                "{header}"
            );
        }
    }
}
