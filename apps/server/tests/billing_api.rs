//! Orbit Cloud billing over real HTTP: the billing routes, the Stripe webhook → sync job → stored
//! subscription path against a local Stripe stand-in, and the read-only guard.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use orbit_platform::{
    AuthenticatedUser, BillingConfig, Config, Database, EnvironmentMode, Id, PasswordService, Plan,
    Secret, TimestampMillis, TrialConfig,
};
use orbit_server::app::App;
use orbit_server::billing::stripe::{API_VERSION, sign};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const COOKIE: &str = "orbit_session_dev";
const WEBHOOK_SECRET: &str = "whsec_billing_test";
const SECRET_KEY: &str = "sk_test_billing";
const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// A Stripe stand-in: records requests, serves subscriptions and prices the test sets.
#[derive(Clone, Default)]
struct FakeStripe {
    requests: Arc<Mutex<Vec<StripeRequest>>>,
    subscriptions: Arc<Mutex<HashMap<String, Value>>>,
    prices: Arc<Mutex<HashMap<String, Value>>>,
    /// Checkout Sessions in creation order: id, customer, status.
    sessions: Arc<Mutex<Vec<(String, String, String)>>>,
}

#[derive(Clone, Debug)]
struct StripeRequest {
    path: String,
    form: HashMap<String, String>,
    idempotency_key: Option<String>,
}

impl FakeStripe {
    fn record(&self, path: &str, headers: &HeaderMap, form: HashMap<String, String>) {
        assert_eq!(
            headers["authorization"].to_str().unwrap(),
            format!("Bearer {SECRET_KEY}")
        );
        assert_eq!(headers["stripe-version"].to_str().unwrap(), API_VERSION);
        self.requests.lock().unwrap().push(StripeRequest {
            path: path.to_owned(),
            form,
            idempotency_key: headers
                .get("idempotency-key")
                .map(|value| value.to_str().unwrap().to_owned()),
        });
    }

    fn requests(&self, path: &str) -> Vec<StripeRequest> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path == path)
            .cloned()
            .collect()
    }

    fn set_subscription(&self, id: &str, workspace_id: &str, status: &str, price: &str) {
        self.subscriptions.lock().unwrap().insert(
            id.to_owned(),
            json!({
                "id": id,
                "object": "subscription",
                "customer": "cus_test",
                "status": status,
                "cancel_at_period_end": false,
                "cancel_at": null,
                "metadata": {"workspace_id": workspace_id},
                "items": {"object": "list", "data": [
                    {"id": "si_1", "price": {"id": price}, "current_period_end": 1_900_000_000}
                ]},
            }),
        );
    }

    fn subscription_status(&self, id: &str) -> Option<String> {
        self.subscriptions
            .lock()
            .unwrap()
            .get(id)
            .map(|subscription| subscription["status"].as_str().unwrap().to_owned())
    }

    fn sessions(&self) -> Vec<(String, String)> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .map(|(id, _, status)| (id.clone(), status.clone()))
            .collect()
    }

    fn set_price(&self, id: &str, product: (&str, &str), amount: i64, interval: &str) {
        self.prices.lock().unwrap().insert(
            id.to_owned(),
            json!({
                "id": id,
                "object": "price",
                "currency": "usd",
                "unit_amount": amount,
                "recurring": {"interval": interval, "interval_count": 1},
                "product": {"id": product.0, "object": "product", "name": product.1},
            }),
        );
    }

    async fn start(&self) -> String {
        let router = Router::new()
            .route(
                "/v1/prices/{id}",
                get(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Path(id): Path<String>,
                     Query(query): Query<HashMap<String, String>>| async move {
                        stripe.record(&format!("/v1/prices/{id}"), &headers, query);
                        match stripe.prices.lock().unwrap().get(&id) {
                            Some(price) => (StatusCode::OK, Json(price.clone())),
                            None => (
                                StatusCode::NOT_FOUND,
                                Json(json!({"error": {"message": "No such price"}})),
                            ),
                        }
                    },
                ),
            )
            .route(
                "/v1/customers",
                post(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Form(form): Form<HashMap<String, String>>| async move {
                        stripe.record("/v1/customers", &headers, form);
                        Json(json!({"id": "cus_test", "object": "customer"}))
                    },
                ),
            )
            .route(
                "/v1/customers/{id}",
                post(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Path(id): Path<String>,
                     Form(form): Form<HashMap<String, String>>| async move {
                        stripe.record(&format!("/v1/customers/{id}"), &headers, form);
                        Json(json!({"id": id, "object": "customer"}))
                    },
                ),
            )
            .route(
                "/v1/checkout/sessions",
                post(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Form(form): Form<HashMap<String, String>>| async move {
                        let mut sessions = stripe.sessions.lock().unwrap();
                        let id = format!("cs_{}", sessions.len() + 1);
                        sessions.push((id.clone(), form["customer"].clone(), "open".to_owned()));
                        drop(sessions);
                        stripe.record("/v1/checkout/sessions", &headers, form);
                        Json(json!({"id": id, "url": "https://checkout.stripe.test/cs_test"}))
                    },
                )
                .get(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Query(query): Query<HashMap<String, String>>| async move {
                        let data: Vec<Value> = stripe
                            .sessions
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|(_, customer, status)| {
                                query.get("customer") == Some(customer)
                                    && query.get("status").is_none_or(|wanted| wanted == status)
                            })
                            .map(|(id, customer, status)| {
                                json!({"id": id, "object": "checkout.session", "customer": customer, "status": status})
                            })
                            .collect();
                        stripe.record("GET /v1/checkout/sessions", &headers, query);
                        Json(json!({"object": "list", "has_more": false, "data": data}))
                    },
                ),
            )
            .route(
                "/v1/checkout/sessions/{id}/expire",
                post(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Path(id): Path<String>| async move {
                        stripe.record(&format!("/v1/checkout/sessions/{id}/expire"), &headers, HashMap::new());
                        let mut sessions = stripe.sessions.lock().unwrap();
                        match sessions.iter_mut().find(|session| session.0 == id) {
                            Some(session) if session.2 == "open" => {
                                session.2 = "expired".to_owned();
                                (StatusCode::OK, Json(json!({"id": id, "status": "expired"})))
                            }
                            _ => (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error": {"message": "Session is not open"}})),
                            ),
                        }
                    },
                ),
            )
            .route(
                "/v1/subscriptions",
                get(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Query(query): Query<HashMap<String, String>>| async move {
                        let data: Vec<Value> = stripe
                            .subscriptions
                            .lock()
                            .unwrap()
                            .values()
                            .filter(|subscription| {
                                query.get("customer").map(String::as_str)
                                    == subscription["customer"].as_str()
                                    && subscription["status"] != "canceled"
                            })
                            .cloned()
                            .collect();
                        stripe.record("GET /v1/subscriptions", &headers, query);
                        Json(json!({"object": "list", "has_more": false, "data": data}))
                    },
                ),
            )
            .route(
                "/v1/billing_portal/sessions",
                post(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Form(form): Form<HashMap<String, String>>| async move {
                        stripe.record("/v1/billing_portal/sessions", &headers, form);
                        Json(json!({"id": "bps_test", "url": "https://billing.stripe.test/p"}))
                    },
                ),
            )
            .route(
                "/v1/subscriptions/{id}",
                get(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Path(id): Path<String>| async move {
                        stripe.record("/v1/subscriptions", &headers, HashMap::new());
                        match stripe.subscriptions.lock().unwrap().get(&id) {
                            Some(subscription) => (StatusCode::OK, Json(subscription.clone())),
                            None => (
                                StatusCode::NOT_FOUND,
                                Json(json!({"error": {"message": "No such subscription"}})),
                            ),
                        }
                    },
                )
                .delete(
                    |State(stripe): State<FakeStripe>,
                     headers: HeaderMap,
                     Path(id): Path<String>| async move {
                        stripe.record(&format!("DELETE /v1/subscriptions/{id}"), &headers, HashMap::new());
                        match stripe.subscriptions.lock().unwrap().get_mut(&id) {
                            Some(subscription) if subscription["status"] != "canceled" => {
                                subscription["status"] = json!("canceled");
                                (StatusCode::OK, Json(subscription.clone()))
                            }
                            Some(_) => (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error": {"message": "This subscription has already been canceled"}})),
                            ),
                            None => (
                                StatusCode::NOT_FOUND,
                                Json(json!({"error": {"message": "No such subscription"}})),
                            ),
                        }
                    },
                ),
            )
            .with_state(self.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        base
    }
}

struct Server {
    base: String,
    database: Database,
    identity: IdentityRepository,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
    session: String,
    workspace_id: String,
    _root: TempDir,
}

impl Server {
    /// A server whose workspace is not the billing-exempt one, so its trial and subscription count.
    async fn start(stripe_base: Option<&str>) -> Self {
        Self::start_with_trial(stripe_base, TrialConfig::default()).await
    }

    async fn start_with_trial(stripe_base: Option<&str>, trial: TrialConfig) -> Self {
        let server = Self::boot(stripe_base, trial).await;
        sqlx::query("UPDATE workspaces SET billing_exempt = 0 WHERE id = $1")
            .bind(&server.workspace_id)
            .execute(server.database.pool())
            .await
            .unwrap();
        server
    }

    /// A server whose workspace is the one created at setup: the root user's first, exempt one.
    async fn start_with_root_team(stripe_base: Option<&str>) -> Self {
        Self::boot(stripe_base, TrialConfig::default()).await
    }

    async fn boot(stripe_base: Option<&str>, trial: TrialConfig) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            app_key: Some(orbit_platform::AppKey::new([1; 32])),
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.billing = stripe_base.map(|base| BillingConfig {
            api_base: base.parse().unwrap(),
            trial,
            ..BillingConfig::new(
                Secret::new(SECRET_KEY),
                Secret::new(WEBHOOK_SECRET),
                vec![
                    Plan {
                        key: "team".to_owned(),
                        price_id: "price_team".to_owned(),
                    },
                    Plan {
                        key: "business".to_owned(),
                        price_id: "price_business".to_owned(),
                    },
                ],
            )
        });
        let app = App::build(config).await.unwrap();
        let database = app.database().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move { app.serve(listener, token).await.unwrap() });
        let identity = IdentityRepository::new(database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "billing-setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "billing-setup".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Acme".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        Self {
            base,
            database,
            identity,
            shutdown,
            task,
            http: reqwest::Client::new(),
            session: setup.session.token,
            workspace_id: setup.workspace_id.to_string(),
            _root: root,
        }
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }

    async fn call_as(
        &self,
        session: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("origin", &self.base);
        if !session.is_empty() {
            request = request.header("cookie", format!("{COOKIE}={session}"));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.call_as(&self.session, method, path, body).await
    }

    fn workspace(&self, rest: &str) -> String {
        format!("/api/v1/workspaces/{}{rest}", self.workspace_id)
    }

    async fn create_task(&self, title: &str) -> (u16, Value) {
        self.create_task_as(&self.session, title).await
    }

    async fn create_task_as(&self, session: &str, title: &str) -> (u16, Value) {
        let project: String = sqlx::query_scalar("SELECT id FROM projects WHERE workspace_id = $1")
            .bind(&self.workspace_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        let status: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
        )
        .bind(&project)
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        self.call_as(
            session,
            "POST",
            &self.workspace("/tasks"),
            Some(json!({"project_id": project, "status_id": status, "title": title})),
        )
        .await
    }

    /// Posts a signed Stripe event; returns the status code.
    async fn webhook(&self, event: &Value) -> u16 {
        let body = event.to_string();
        let signature = sign(
            WEBHOOK_SECRET,
            body.as_bytes(),
            TimestampMillis::now().as_millis() / 1000,
        );
        self.http
            .post(format!("{}/api/v1/billing/stripe/webhook", self.base))
            .header("stripe-signature", signature)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    async fn set_status(&self, status: &str) {
        sqlx::query(
            "INSERT INTO workspace_subscriptions (workspace_id, stripe_customer_id, status, updated_at) \
             VALUES ($1, 'cus_test', $2, 0) \
             ON CONFLICT (workspace_id) DO UPDATE SET status = excluded.status",
        )
        .bind(&self.workspace_id)
        .bind(status)
        .execute(self.database.pool())
        .await
        .unwrap();
    }

    /// Moves the workspace's creation `days` into the past.
    async fn age_workspace(&self, days: i64) {
        sqlx::query("UPDATE workspaces SET created_at = $1 WHERE id = $2")
            .bind(TimestampMillis::now().as_millis() - days * DAY_MILLIS)
            .bind(&self.workspace_id)
            .execute(self.database.pool())
            .await
            .unwrap();
    }

    async fn sync_jobs(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'billing.stripe_sync'")
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// Waits until every sync job finished, then returns the workspace's billing view.
    async fn settled_billing(&self) -> Value {
        for _ in 0..100 {
            let pending: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM jobs WHERE kind = 'billing.stripe_sync' AND state != 'succeeded'",
            )
            .fetch_one(self.database.pool())
            .await
            .unwrap();
            if pending == 0 {
                return self.call("GET", &self.workspace("/billing"), None).await.1;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("billing.stripe_sync jobs did not finish");
    }

    /// Waits until every job of `kind` finished; panics on a job that failed for good.
    async fn settle(&self, kind: &str) {
        for _ in 0..100 {
            let states: Vec<String> = sqlx::query_scalar("SELECT state FROM jobs WHERE kind = $1")
                .bind(kind)
                .fetch_all(self.database.pool())
                .await
                .unwrap();
            assert!(
                !states.iter().any(|state| state == "dead"),
                "a {kind} job died"
            );
            if states.iter().all(|state| state == "succeeded") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("{kind} jobs did not finish");
    }

    async fn workspace_version(&self) -> i64 {
        sqlx::query_scalar("SELECT version FROM workspaces WHERE id = $1")
            .bind(&self.workspace_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn delete_workspace(&self) -> u16 {
        let version = self.workspace_version().await;
        self.call(
            "DELETE",
            &format!("{}?expected_version={version}", self.workspace("")),
            None,
        )
        .await
        .0
    }

    /// Subscribes the workspace to `subscription` (active) through the webhook path.
    async fn subscribe(&self, stripe: &FakeStripe, subscription: &str) {
        stripe.set_subscription(subscription, &self.workspace_id, "active", "price_team");
        assert_eq!(
            self.webhook(&checkout_completed(
                &format!("evt_{subscription}"),
                subscription,
                &self.workspace_id
            ))
            .await,
            200
        );
        assert_eq!(self.settled_billing().await["status"], "active");
    }

    async fn add_member(&self) -> String {
        self.add_user("member@example.com", true).await
    }

    /// A signed-in user, a member of the workspace when `member` is set; returns the session.
    async fn add_user(&self, email: &str, member: bool) -> String {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Member', 'unused', $4, $5)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(email)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        if member {
            sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&self.workspace_id)
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        }
        self.identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email: email.to_owned(),
                    display_name: "Member".to_owned(),
                },
                now,
            )
            .await
            .unwrap()
            .token
    }
}

fn subscription_event(id: &str, kind: &str, subscription: &str, workspace_id: &str) -> Value {
    json!({
        "id": id,
        "object": "event",
        "type": kind,
        "data": {"object": {
            "id": subscription,
            "object": "subscription",
            "metadata": {"workspace_id": workspace_id},
        }},
    })
}

fn checkout_completed(id: &str, subscription: &str, workspace_id: &str) -> Value {
    json!({
        "id": id,
        "object": "event",
        "type": "checkout.session.completed",
        "data": {"object": {
            "id": "cs_test",
            "object": "checkout.session",
            "mode": "subscription",
            "subscription": subscription,
            "client_reference_id": workspace_id,
        }},
    })
}

#[tokio::test]
async fn a_workspace_without_a_subscription_is_read_only_after_its_trial_until_it_subscribes() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    server.age_workspace(7).await;

    let (status, problem) = server.create_task("Blocked").await;
    assert_eq!(status, 402, "{problem}");
    assert_eq!(problem["code"], "workspace_read_only");
    let (status, _) = server.call("GET", &server.workspace("/tasks"), None).await;
    assert_eq!(status, 200);
    let (status, _) = server
        .call("POST", &server.workspace("/tasks/query"), Some(json!({})))
        .await;
    assert_ne!(status, 402);

    // A percent-encoded workspace id reaches the same handler, so the guard must decode it too.
    let encoded: String = server
        .workspace_id
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let (status, problem) = server
        .call(
            "PATCH",
            &format!("/api/v1/workspaces/{encoded}"),
            Some(json!({"name": "Renamed"})),
        )
        .await;
    assert_eq!(status, 402, "{problem}");

    let (status, options) = server.call("GET", "/api/v1/auth/options", None).await;
    assert_eq!(status, 200);
    assert_eq!(options["billing_enabled"], true);
    let (status, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        billing,
        json!({
            "plan": null,
            "status": null,
            "access": "read_only",
            "exempt": false,
            "cancel_at_period_end": false,
            "current_period_end": null,
            "plans": [
                {"key": "team", "product_id": null, "name": "Team", "amount": null, "currency": null, "interval": null},
                {"key": "business", "product_id": null, "name": "Business", "amount": null, "currency": null, "interval": null},
            ],
            "has_customer": false,
            "can_manage": true,
            "trial_days": 7,
            "trial": null,
        })
    );

    let (status, problem) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(json!({"plan": "gold"})),
        )
        .await;
    assert_eq!((status, &problem["code"]), (422, &json!("unknown_plan")));
    let (status, problem) = server
        .call("POST", &server.workspace("/billing/portal"), None)
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (409, &json!("billing_customer_missing"))
    );

    let (status, redirect) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(json!({"plan": "business"})),
        )
        .await;
    assert_eq!(status, 200, "{redirect}");
    assert_eq!(
        redirect,
        json!({"url": "https://checkout.stripe.test/cs_test"})
    );
    let customers = stripe.requests("/v1/customers");
    assert_eq!(customers.len(), 1);
    assert_eq!(
        customers[0].idempotency_key.as_deref(),
        Some(format!("orbit-workspace-{}-customer", server.workspace_id).as_str())
    );
    assert_eq!(
        customers[0].form["metadata[workspace_id]"],
        server.workspace_id
    );
    assert_eq!(customers[0].form["email"], "owner@example.com");
    assert_eq!(customers[0].form["name"], "Acme");
    let checkout = &stripe.requests("/v1/checkout/sessions")[0].form;
    let expected: HashMap<String, String> = [
        ("mode", "subscription".to_owned()),
        ("customer", "cus_test".to_owned()),
        ("line_items[0][price]", "price_business".to_owned()),
        ("line_items[0][quantity]", "1".to_owned()),
        ("client_reference_id", server.workspace_id.clone()),
        (
            "subscription_data[metadata][workspace_id]",
            server.workspace_id.clone(),
        ),
        ("automatic_tax[enabled]", "true".to_owned()),
        ("customer_update[address]", "auto".to_owned()),
        ("customer_update[name]", "auto".to_owned()),
        ("tax_id_collection[enabled]", "true".to_owned()),
        (
            "success_url",
            "http://127.0.0.1:8080/settings/billing?checkout=success".to_owned(),
        ),
        (
            "cancel_url",
            "http://127.0.0.1:8080/settings/billing?checkout=cancel".to_owned(),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    assert_eq!(checkout, &expected);

    // A second checkout reuses the stored customer.
    let (status, _) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(json!({"plan": "team"})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(stripe.requests("/v1/customers").len(), 1);

    stripe.set_subscription("sub_1", &server.workspace_id, "active", "price_business");
    assert_eq!(
        server
            .webhook(&checkout_completed(
                "evt_checkout",
                "sub_1",
                &server.workspace_id
            ))
            .await,
        200
    );
    let billing = server.settled_billing().await;
    assert_eq!(billing["access"], "full", "{billing}");
    assert_eq!(billing["status"], "active");
    assert_eq!(billing["plan"], "business");
    assert_eq!(billing["has_customer"], true);
    assert_eq!(billing["current_period_end"], "2030-03-17T17:46:40.000Z");

    let (status, task) = server.create_task("Allowed").await;
    assert_eq!(status, 201, "{task}");
    let (status, problem) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(json!({"plan": "team"})),
        )
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (409, &json!("subscription_active"))
    );
    let (status, redirect) = server
        .call("POST", &server.workspace("/billing/portal"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(redirect["url"], "https://billing.stripe.test/p");
    let portal = &stripe.requests("/v1/billing_portal/sessions")[0].form;
    assert_eq!(portal["customer"], "cus_test");
    assert_eq!(
        portal["return_url"],
        "http://127.0.0.1:8080/settings/billing"
    );

    let member = server.add_member().await;
    let (status, billing) = server
        .call_as(&member, "GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(billing["can_manage"], false);
    let (status, problem) = server
        .call_as(&member, "POST", &server.workspace("/billing/portal"), None)
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (403, &json!("billing_owner_required"))
    );

    // The subscription is cancelled: the workspace goes back to read-only, data stays readable.
    stripe.set_subscription("sub_1", &server.workspace_id, "canceled", "price_business");
    assert_eq!(
        server
            .webhook(&subscription_event(
                "evt_deleted",
                "customer.subscription.deleted",
                "sub_1",
                &server.workspace_id,
            ))
            .await,
        200
    );
    let billing = server.settled_billing().await;
    assert_eq!(billing["access"], "read_only");
    assert_eq!(billing["status"], "canceled");
    assert_eq!(server.create_task("Blocked again").await.0, 402);
    let (status, tasks) = server.call("GET", &server.workspace("/tasks"), None).await;
    assert_eq!(status, 200);
    assert!(tasks.to_string().contains("Allowed"));

    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE action = 'billing.subscription_status_changed'",
    )
    .fetch_one(server.database.pool())
    .await
    .unwrap();
    assert_eq!(audited, 2);
    server.stop().await;
}

#[tokio::test]
async fn redelivered_and_out_of_order_events_converge_on_stripe() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    let workspace = server.workspace_id.clone();

    // Stripe now says active; the older "created" event (incomplete at the time) arrives last.
    stripe.set_subscription("sub_2", &workspace, "active", "price_team");
    let updated = subscription_event(
        "evt_updated",
        "customer.subscription.updated",
        "sub_2",
        &workspace,
    );
    assert_eq!(server.webhook(&updated).await, 200);
    assert_eq!(server.webhook(&updated).await, 200);
    assert_eq!(server.sync_jobs().await, 1);
    assert_eq!(
        server
            .webhook(&subscription_event(
                "evt_created",
                "customer.subscription.created",
                "sub_2",
                &workspace,
            ))
            .await,
        200
    );
    assert_eq!(server.sync_jobs().await, 2);
    let billing = server.settled_billing().await;
    assert_eq!(billing["status"], "active");
    assert_eq!(billing["plan"], "team");

    // A late event about an old, cancelled subscription leaves the new one in place.
    stripe.set_subscription("sub_old", &workspace, "canceled", "price_team");
    assert_eq!(
        server
            .webhook(&subscription_event(
                "evt_old",
                "customer.subscription.deleted",
                "sub_old",
                &workspace,
            ))
            .await,
        200
    );
    assert_eq!(server.settled_billing().await["access"], "full");

    // Unknown prices are stored without a plan; other event types queue nothing.
    stripe.set_subscription("sub_2", &workspace, "trialing", "price_legacy");
    let ignored = json!({"id": "evt_invoice", "type": "invoice.paid", "data": {"object": {}}});
    assert_eq!(server.webhook(&ignored).await, 200);
    assert_eq!(
        server
            .webhook(&subscription_event(
                "evt_legacy",
                "customer.subscription.updated",
                "sub_2",
                &workspace,
            ))
            .await,
        200
    );
    assert_eq!(server.sync_jobs().await, 4);
    let billing = server.settled_billing().await;
    assert_eq!(billing["status"], "trialing");
    assert_eq!(billing["plan"], Value::Null);
    assert_eq!(billing["access"], "full");
    server.stop().await;
}

#[tokio::test]
async fn webhooks_without_a_valid_signature_are_rejected() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    let body = subscription_event(
        "evt_forged",
        "customer.subscription.updated",
        "sub_x",
        &server.workspace_id,
    )
    .to_string();
    let now = TimestampMillis::now().as_millis() / 1000;
    for signature in [
        None,
        Some(sign("whsec_wrong", body.as_bytes(), now)),
        Some(sign(WEBHOOK_SECRET, body.as_bytes(), now - 600)),
        Some("t=1,v1=00".to_owned()),
    ] {
        let mut request = server
            .http
            .post(format!("{}/api/v1/billing/stripe/webhook", server.base))
            .body(body.clone());
        if let Some(signature) = signature {
            request = request.header("stripe-signature", signature);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status().as_u16(), 400);
        let problem: Value = response.json().await.unwrap();
        assert_eq!(problem["code"], "invalid_stripe_signature");
    }
    assert_eq!(server.sync_jobs().await, 0);
    server.stop().await;
}

#[tokio::test]
async fn only_members_learn_that_a_workspace_is_read_only() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    let stranger = server.add_user("stranger@example.com", false).await;
    let member = server.add_member().await;
    server.age_workspace(8).await;
    server.set_status("active").await;
    let [paid_anonymous, paid_stranger, paid_member] =
        write_answers(&server, &stranger, &member).await;
    server.set_status("canceled").await;
    let [anonymous, stranger_answer, member_answer] =
        write_answers(&server, &stranger, &member).await;

    assert_eq!(paid_anonymous.0, 401);
    assert_eq!(
        anonymous, paid_anonymous,
        "an anonymous write gets the same answer either way"
    );
    assert_eq!(paid_stranger.0, 404);
    assert_eq!(
        stranger_answer, paid_stranger,
        "a non-member gets the non-member answer either way"
    );
    assert_eq!(paid_member.0, 201);
    assert_eq!(member_answer, (402, json!("workspace_read_only")));
    server.stop().await;
}

#[tokio::test]
async fn a_new_workspace_is_on_a_trial_with_limits() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    let created_at: i64 = sqlx::query_scalar("SELECT created_at FROM workspaces WHERE id = $1")
        .bind(&server.workspace_id)
        .fetch_one(server.database.pool())
        .await
        .unwrap();
    let tasks_before: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM tasks WHERE workspace_id = $1 AND deleted_at IS NULL",
    )
    .bind(&server.workspace_id)
    .fetch_one(server.database.pool())
    .await
    .unwrap();

    let (status, task) = server.create_task("Trial task").await;
    assert_eq!(status, 201, "{task}");
    let (status, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(billing["access"], "trial", "{billing}");
    assert_eq!(
        billing["trial"],
        json!({
            "ends_at": serde_json::to_value(TimestampMillis::from_millis(created_at + 7 * DAY_MILLIS)).unwrap(),
            "tasks": {"used": tasks_before + 1, "limit": 50},
            "pages": {"used": 0, "limit": 20},
            "members": {"used": 1, "limit": 3},
        })
    );

    for index in tasks_before + 1..50 {
        let (status, task) = server.create_task(&format!("Task {index}")).await;
        assert_eq!(status, 201, "{task}");
    }
    let (status, problem) = server.create_task("One too many").await;
    assert_eq!(status, 402, "{problem}");
    assert_eq!(
        (&problem["code"], &problem["type"], &problem["detail"]),
        (
            &json!("trial_limit_reached"),
            &json!("https://docs.orbit.dev/problems/trial_limit_reached"),
            &json!("The trial allows 50 tasks. Choose a plan to add more."),
        )
    );
    let path = server.workspace(&format!(
        "/tasks/{}?expected_version={}",
        task["id"].as_str().unwrap(),
        task["version"]
    ));
    let (status, deleted) = server.call("DELETE", &path, None).await;
    assert_eq!(status, 204, "{deleted}");
    assert_eq!(server.create_task("Freed slot").await.0, 201);

    for index in 0..20 {
        let (status, page) = server
            .call(
                "POST",
                &server.workspace("/pages"),
                Some(json!({"title": format!("Page {index}")})),
            )
            .await;
        assert_eq!(status, 201, "{page}");
    }
    let (status, problem) = server
        .call("POST", &server.workspace("/pages"), Some(json!({})))
        .await;
    assert_eq!(
        (status, &problem["detail"]),
        (
            402,
            &json!("The trial allows 20 pages. Choose a plan to add more.")
        )
    );
    let page: String = sqlx::query_scalar("SELECT id FROM pages WHERE workspace_id = $1 LIMIT 1")
        .bind(&server.workspace_id)
        .fetch_one(server.database.pool())
        .await
        .unwrap();
    let (status, problem) = server
        .call(
            "POST",
            &server.workspace(&format!("/pages/{page}/duplicate")),
            Some(json!({"include_children": false})),
        )
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (402, &json!("trial_limit_reached"))
    );

    for email in ["one@example.com", "two@example.com"] {
        let (status, invitation) = server
            .call(
                "POST",
                &server.workspace("/invitations"),
                Some(json!({"email": email, "role": "member", "delivery": "manual"})),
            )
            .await;
        assert_eq!(status, 201, "{invitation}");
    }
    let (status, problem) = server
        .call(
            "POST",
            &server.workspace("/invitations"),
            Some(json!({"email": "three@example.com", "role": "member", "delivery": "manual"})),
        )
        .await;
    assert_eq!(
        (status, &problem["detail"]),
        (
            402,
            &json!("The trial allows 3 members. Choose a plan to add more.")
        )
    );

    // A stranger learns nothing about the trial: the handler refuses them as on any workspace.
    let stranger = server.add_user("stranger@example.com", false).await;
    assert_eq!(server.create_task_as(&stranger, "Task").await.0, 404);

    server.age_workspace(7).await;
    let (status, problem) = server.create_task("After the trial").await;
    assert_eq!(
        (status, &problem["code"]),
        (402, &json!("workspace_read_only"))
    );
    let (_, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(
        (&billing["access"], &billing["trial"]),
        (&json!("read_only"), &Value::Null)
    );
    server.stop().await;
}

#[tokio::test]
async fn the_trial_length_and_limits_come_from_the_config() {
    let stripe = FakeStripe::default();
    let trial = TrialConfig {
        days: 1,
        tasks: 2,
        ..TrialConfig::default()
    };
    let server = Server::start_with_trial(Some(&stripe.start().await), trial).await;
    sqlx::query("UPDATE tasks SET deleted_at = $1 WHERE workspace_id = $2")
        .bind(TimestampMillis::now().as_millis())
        .bind(&server.workspace_id)
        .execute(server.database.pool())
        .await
        .unwrap();

    assert_eq!(server.create_task("One").await.0, 201);
    assert_eq!(server.create_task("Two").await.0, 201);
    let (status, problem) = server.create_task("Three").await;
    assert_eq!(
        (status, &problem["code"], &problem["detail"]),
        (
            402,
            &json!("trial_limit_reached"),
            &json!("The trial allows 2 tasks. Choose a plan to add more."),
        )
    );
    let (_, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(
        (&billing["trial_days"], &billing["trial"]["tasks"]),
        (&json!(1), &json!({"used": 2, "limit": 2}))
    );

    sqlx::query("UPDATE workspaces SET created_at = $1 WHERE id = $2")
        .bind(TimestampMillis::now().as_millis() - DAY_MILLIS - 1)
        .bind(&server.workspace_id)
        .execute(server.database.pool())
        .await
        .unwrap();
    let (status, problem) = server
        .call("POST", &server.workspace("/pages"), Some(json!({})))
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (402, &json!("workspace_read_only"))
    );
    let (_, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(
        (&billing["access"], &billing["trial"]),
        (&json!("read_only"), &Value::Null)
    );
    server.stop().await;
}

#[tokio::test]
async fn billing_describes_each_plan_as_stripe_prices_it_and_asks_stripe_once() {
    let stripe = FakeStripe::default();
    stripe.set_price("price_team", ("prod_team", "Team"), 1000, "month");
    stripe.set_price(
        "price_business",
        ("prod_business", "Business"),
        29000,
        "year",
    );
    let server = Server::start(Some(&stripe.start().await)).await;

    for _ in 0..2 {
        let (status, billing) = server
            .call("GET", &server.workspace("/billing"), None)
            .await;
        assert_eq!(status, 200, "{billing}");
        assert_eq!(
            billing["plans"],
            json!([
                {"key": "team", "product_id": "prod_team", "name": "Team", "amount": 1000, "currency": "usd", "interval": "month"},
                {"key": "business", "product_id": "prod_business", "name": "Business", "amount": 29000, "currency": "usd", "interval": "year"},
            ])
        );
    }
    let fetches = stripe.requests("/v1/prices/price_team");
    assert_eq!(fetches.len(), 1);
    assert_eq!(fetches[0].form["expand[]"], "product");
    assert_eq!(stripe.requests("/v1/prices/price_business").len(), 1);
    server.stop().await;
}

#[tokio::test]
async fn billing_names_plans_by_key_while_stripe_fails_and_asks_again_next_time() {
    let stripe = FakeStripe::default();
    stripe.set_price("price_team", ("prod_team", "Team"), 1000, "month");
    let server = Server::start(Some(&stripe.start().await)).await;

    let (status, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(status, 200, "{billing}");
    assert_eq!(
        billing["plans"],
        json!([
            {"key": "team", "product_id": null, "name": "Team", "amount": null, "currency": null, "interval": null},
            {"key": "business", "product_id": null, "name": "Business", "amount": null, "currency": null, "interval": null},
        ])
    );

    stripe.set_price(
        "price_business",
        ("prod_business", "Business"),
        2900,
        "month",
    );
    let (_, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(billing["plans"][1]["amount"], 2900);
    assert_eq!(stripe.requests("/v1/prices/price_business").len(), 2);
    server.stop().await;
}

#[tokio::test]
async fn a_subscribed_workspace_has_no_trial_limits() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    server.set_status("active").await;
    for index in 0..21 {
        let (status, page) = server
            .call(
                "POST",
                &server.workspace("/pages"),
                Some(json!({"title": format!("Page {index}")})),
            )
            .await;
        assert_eq!(status, 201, "{page}");
    }
    for email in ["one@example.com", "two@example.com", "three@example.com"] {
        let (status, invitation) = server
            .call(
                "POST",
                &server.workspace("/invitations"),
                Some(json!({"email": email, "role": "member", "delivery": "manual"})),
            )
            .await;
        assert_eq!(status, 201, "{invitation}");
    }
    let (_, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(
        (&billing["access"], &billing["trial"]),
        (&json!("full"), &Value::Null)
    );
    server.stop().await;
}

#[tokio::test]
async fn the_root_users_first_workspace_needs_no_subscription() {
    let stripe = FakeStripe::default();
    let server = Server::start_with_root_team(Some(&stripe.start().await)).await;
    server.age_workspace(8).await;

    for index in 0..21 {
        let (status, page) = server
            .call(
                "POST",
                &server.workspace("/pages"),
                Some(json!({"title": format!("Page {index}")})),
            )
            .await;
        assert_eq!(status, 201, "{page}");
    }
    for email in ["one@example.com", "two@example.com", "three@example.com"] {
        let (status, invitation) = server
            .call(
                "POST",
                &server.workspace("/invitations"),
                Some(json!({"email": email, "role": "member", "delivery": "manual"})),
            )
            .await;
        assert_eq!(status, 201, "{invitation}");
    }
    let (status, task) = server.create_task("After eight days").await;
    assert_eq!(status, 201, "{task}");
    let (_, billing) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(
        (&billing["access"], &billing["exempt"], &billing["trial"]),
        (&json!("full"), &json!(true), &Value::Null),
        "{billing}"
    );
    let (status, problem) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(json!({"plan": "team"})),
        )
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (409, &json!("subscription_active"))
    );
    assert!(stripe.requests("/v1/customers").is_empty());
    server.stop().await;
}

#[tokio::test]
async fn other_workspaces_of_root_and_workspaces_of_other_users_are_on_a_trial() {
    let stripe = FakeStripe::default();
    let server = Server::start_with_root_team(Some(&stripe.start().await)).await;
    let other_user = server.add_user("other@example.com", false).await;

    for (session, name) in [
        (server.session.as_str(), "Root second"),
        (other_user.as_str(), "Other team"),
    ] {
        let (status, workspace) = server
            .call_as(
                session,
                "POST",
                "/api/v1/workspaces",
                Some(json!({"name": name})),
            )
            .await;
        assert_eq!(status, 201, "{workspace}");
        let path = format!(
            "/api/v1/workspaces/{}/billing",
            workspace["id"].as_str().unwrap()
        );
        let (_, billing) = server.call_as(session, "GET", &path, None).await;
        assert_eq!(
            (&billing["access"], &billing["exempt"]),
            (&json!("trial"), &json!(false)),
            "{name}: {billing}"
        );
    }
    let exempt: Vec<String> =
        sqlx::query_scalar("SELECT name FROM workspaces WHERE billing_exempt = 1")
            .fetch_all(server.database.pool())
            .await
            .unwrap();
    assert_eq!(exempt, ["Acme"]);
    server.stop().await;
}

/// Status and problem code of a task created by an anonymous caller, by `stranger` and by `member`.
async fn write_answers(server: &Server, stranger: &str, member: &str) -> [(u16, Value); 3] {
    let mut answers = Vec::new();
    for session in ["", stranger, member] {
        let (status, problem) = server.create_task_as(session, "Task").await;
        answers.push((status, problem["code"].clone()));
    }
    answers.try_into().unwrap()
}

#[tokio::test]
async fn self_hosted_servers_have_no_billing_and_no_guard() {
    let server = Server::start(None).await;

    let (status, task) = server.create_task("Self-hosted").await;
    assert_eq!(status, 201, "{task}");
    let (status, _) = server
        .call("GET", &server.workspace("/billing"), None)
        .await;
    assert_eq!(status, 404);
    let (status, _) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(json!({"plan": "team"})),
        )
        .await;
    assert_eq!(status, 404);
    let response = server
        .http
        .post(format!("{}/api/v1/billing/stripe/webhook", server.base))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 404);
    let (_, options) = server.call("GET", "/api/v1/auth/options", None).await;
    assert_eq!(options["billing_enabled"], false);
    server.stop().await;
}

#[tokio::test]
async fn deleting_a_subscribed_workspace_cancels_its_subscription_at_stripe() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    server.age_workspace(30).await;
    server.subscribe(&stripe, "sub_del").await;

    assert_eq!(server.delete_workspace().await, 204);
    server.settle("billing.stripe_cancel").await;
    server.settle("billing.stripe_sync").await;
    assert_eq!(stripe.requests("DELETE /v1/subscriptions/sub_del").len(), 1);
    assert_eq!(
        stripe.requests("DELETE /v1/subscriptions/sub_del")[0].form,
        HashMap::new(),
        "no proration and no final invoice"
    );
    assert_eq!(
        stripe.subscription_status("sub_del").as_deref(),
        Some("canceled")
    );
    let stored: String =
        sqlx::query_scalar("SELECT status FROM workspace_subscriptions WHERE workspace_id = $1")
            .bind(&server.workspace_id)
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    assert_eq!(stored, "canceled");

    // Restoring does not resume billing: the old workspace is read-only until it subscribes again.
    let version = server.workspace_version().await;
    let (status, _) = server
        .call(
            "POST",
            &server.workspace("/restore"),
            Some(json!({"expected_version": version})),
        )
        .await;
    assert_eq!(status, 204);
    let billing = server.settled_billing().await;
    assert_eq!(
        (&billing["status"], &billing["access"]),
        (&json!("canceled"), &json!("read_only"))
    );
    let cancel_jobs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'billing.stripe_cancel'")
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    assert_eq!(cancel_jobs, 1);

    // Deleting it again queues nothing: its subscription already ended.
    assert_eq!(server.delete_workspace().await, 204);
    let cancel_jobs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'billing.stripe_cancel'")
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    assert_eq!(cancel_jobs, 1);
    server.stop().await;
}

#[tokio::test]
async fn the_cancel_job_finishes_when_stripe_already_ended_the_subscription_or_the_workspace_is_gone()
 {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    server.subscribe(&stripe, "sub_gone").await;
    // Cancelled at Stripe while Orbit has not heard of it yet.
    stripe.set_subscription("sub_gone", &server.workspace_id, "canceled", "price_team");
    assert_eq!(server.delete_workspace().await, 204);
    server.settle("billing.stripe_cancel").await;
    assert_eq!(
        stripe.requests("DELETE /v1/subscriptions/sub_gone").len(),
        1
    );

    // A job for a purged workspace (its id only in the payload) and for an unknown subscription.
    stripe.set_subscription(
        "sub_purged",
        &Id::new_v7().to_string(),
        "active",
        "price_team",
    );
    for subscription in ["sub_purged", "sub_unknown"] {
        sqlx::query(
            "INSERT INTO jobs (id, workspace_id, kind, payload_json, state, priority, attempt_count, \
             max_attempts, available_at, created_at, updated_at) \
             VALUES ($1, NULL, 'billing.stripe_cancel', $2, 'queued', 'normal', 0, 10, 0, 0, 0)",
        )
        .bind(Id::new_v7().to_string())
        .bind(
            json!({"subscription_id": subscription, "workspace_id": Id::new_v7().to_string()})
                .to_string(),
        )
        .execute(server.database.pool())
        .await
        .unwrap();
    }
    server.settle("billing.stripe_cancel").await;
    server.settle("billing.stripe_sync").await;
    assert_eq!(
        stripe.subscription_status("sub_purged").as_deref(),
        Some("canceled")
    );
    assert_eq!(
        stripe
            .requests("DELETE /v1/subscriptions/sub_unknown")
            .len(),
        1
    );
    server.stop().await;
}

#[tokio::test]
async fn checkout_is_refused_while_stripe_has_a_live_subscription_orbit_has_not_stored() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    let checkout = json!({"plan": "team"});
    let (status, _) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(checkout.clone()),
        )
        .await;
    assert_eq!(status, 200);
    // The customer paid, but the webhook has not arrived.
    stripe.set_subscription("sub_early", &server.workspace_id, "trialing", "price_team");
    assert_eq!(server.sync_jobs().await, 0);

    let (status, problem) = server
        .call(
            "POST",
            &server.workspace("/billing/checkout"),
            Some(checkout),
        )
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (409, &json!("subscription_active"))
    );
    assert_eq!(stripe.requests("/v1/checkout/sessions").len(), 1);
    let listed = stripe.requests("GET /v1/subscriptions");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].form["customer"], "cus_test");
    // The refusal queued the sync Orbit missed.
    assert_eq!(server.sync_jobs().await, 1);
    let billing = server.settled_billing().await;
    assert_eq!(
        (&billing["status"], &billing["access"]),
        (&json!("trialing"), &json!("full"))
    );
    server.stop().await;
}

#[tokio::test]
async fn a_new_checkout_expires_the_open_sessions_of_the_customer() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    let checkout = json!({"plan": "team"});
    for _ in 0..3 {
        let (status, redirect) = server
            .call(
                "POST",
                &server.workspace("/billing/checkout"),
                Some(checkout.clone()),
            )
            .await;
        assert_eq!(status, 200, "{redirect}");
    }
    assert_eq!(
        stripe.sessions(),
        [
            ("cs_1".to_owned(), "expired".to_owned()),
            ("cs_2".to_owned(), "expired".to_owned()),
            ("cs_3".to_owned(), "open".to_owned()),
        ]
    );
    let listed = stripe.requests("GET /v1/checkout/sessions");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].form["status"], "open");
    assert_eq!(listed[0].form["customer"], "cus_test");
    server.stop().await;
}

#[tokio::test]
async fn orbit_cloud_refuses_ownership_transfer() {
    let stripe = FakeStripe::default();
    let server = Server::start(Some(&stripe.start().await)).await;
    let member = server.add_member().await;
    drop(member);
    let (status, members) = server
        .call("GET", &server.workspace("/members"), None)
        .await;
    assert_eq!(status, 200, "{members}");
    let items = members["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|item| item["can_transfer_ownership"] == false),
        "{members}"
    );
    let target = items.iter().find(|item| item["role"] == "member").unwrap();
    let version = server.workspace_version().await;
    let (status, problem) = server
        .call(
            "POST",
            &server.workspace("/transfer-ownership"),
            Some(json!({
                "membership_id": target["id"],
                "expected_version": version,
                "membership_version": target["version"],
            })),
        )
        .await;
    assert_eq!(
        (status, &problem["code"]),
        (409, &json!("transfer_unavailable"))
    );
    let owner: String = sqlx::query_scalar(
        "SELECT users.email FROM memberships JOIN users ON users.id = memberships.user_id \
         WHERE memberships.workspace_id = $1 AND memberships.role = 'owner'",
    )
    .bind(&server.workspace_id)
    .fetch_one(server.database.pool())
    .await
    .unwrap();
    assert_eq!(owner, "owner@example.com");
    server.stop().await;
}

#[tokio::test]
async fn a_confirmed_email_change_updates_the_stripe_customer_of_each_owned_workspace() {
    let stripe = FakeStripe::default();
    let base = stripe.start().await;
    let server = Server::start(Some(&base)).await;
    server.subscribe(&stripe, "sub_email").await;
    let owner_id: String =
        sqlx::query_scalar("SELECT id FROM users WHERE normalized_email = 'owner@example.com'")
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    let now = TimestampMillis::now();
    server
        .identity
        .store_email_change_token(
            owner_id.parse().unwrap(),
            "Billing@Acme.example",
            "email-change-token",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
            "test",
            now,
        )
        .await
        .unwrap();
    let (status, body) = server
        .call(
            "POST",
            "/api/v1/auth/email-change/confirm",
            Some(json!({"token": "email-change-token"})),
        )
        .await;
    assert_eq!(
        (status, body),
        (200, json!({"email": "Billing@Acme.example"}))
    );
    server.settle("billing.stripe_customer_email").await;
    let updates: Vec<_> = stripe
        .requests("/v1/customers/cus_test")
        .into_iter()
        .map(|request| request.form)
        .collect();
    assert_eq!(
        updates,
        [HashMap::from([(
            "email".to_owned(),
            "Billing@Acme.example".to_owned()
        )])]
    );
    server.stop().await;
}

#[tokio::test]
async fn an_admin_email_change_updates_the_stripe_customer_of_each_owned_workspace() {
    let stripe = FakeStripe::default();
    let base = stripe.start().await;
    let server = Server::start(Some(&base)).await;
    server.subscribe(&stripe, "sub_admin_email").await;
    let owner_id: String =
        sqlx::query_scalar("SELECT id FROM users WHERE normalized_email = 'owner@example.com'")
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    let (status, _) = server
        .call(
            "PUT",
            "/api/v1/admin/settings/smtp",
            Some(json!({
                "host": "127.0.0.1", "port": 9, "security": "none",
                "from_address": "orbit@example.com"
            })),
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = server
        .call(
            "PUT",
            &format!("/api/v1/admin/users/{owner_id}/email"),
            Some(json!({"email": "Billing@Acme.example"})),
        )
        .await;
    assert_eq!(status, 204);
    server.settle("billing.stripe_customer_email").await;
    let updates: Vec<_> = stripe
        .requests("/v1/customers/cus_test")
        .into_iter()
        .map(|request| request.form)
        .collect();
    assert_eq!(
        updates,
        [HashMap::from([(
            "email".to_owned(),
            "Billing@Acme.example".to_owned()
        )])]
    );
    server.stop().await;
}
