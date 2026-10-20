//! Request limits and email caps on two servers of one Postgres database (Orbit Cloud): they must
//! hold for the cluster, not for each server.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_platform::{Config, EnvironmentMode, PasswordService, RateLimitConfig, TimestampMillis};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

mod shared_postgres;

const COOKIE: &str = "orbit_session_dev";

struct Server {
    base: String,
    database: orbit_platform::Database,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
    _root: TempDir,
}

impl Server {
    async fn start(url: &str, rate_limits: RateLimitConfig) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            rate_limits,
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.data.database_url = Some(url.to_owned());
        config.secrets.insert(
            "app_key".to_owned(),
            orbit_platform::Secret::new(shared_postgres::APP_KEY),
        );
        let app = App::build(config).await.unwrap();
        let database = app.database().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });
        Self {
            base,
            database,
            shutdown,
            task,
            http: reqwest::Client::new(),
            _root: root,
        }
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        session: Option<&str>,
        body: Value,
    ) -> (u16, Value) {
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("origin", &self.base)
            .json(&body);
        if let Some(session) = session {
            request = request.header("cookie", format!("{COOKIE}={session}"));
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// Completes the setup; returns the root user's session and the workspace.
    async fn setup(&self) -> (String, String) {
        let identity = IdentityRepository::new(self.database.clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "limits-setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "limits-setup".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        (setup.session.token, setup.workspace_id.to_string())
    }
}

/// Waits for the next minute when this one ends soon, so the requests of a test share a window.
async fn early_in_a_minute() {
    let second = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        % 60;
    if second > 50 {
        tokio::time::sleep(Duration::from_secs(61 - second)).await;
    }
}

#[tokio::test]
async fn sensitive_request_limits_hold_across_servers() {
    let Some(database) = shared_postgres::SharedDatabase::create().await else {
        return; // several servers need a Postgres database
    };
    let limits = RateLimitConfig {
        recovery_per_minute: 2,
        ..RateLimitConfig::default()
    };
    let one = Server::start(&database.url, limits).await;
    let two = Server::start(&database.url, limits).await;
    early_in_a_minute().await;
    let recovery = json!({"email": "someone@example.com"});
    let path = "/api/v1/auth/recovery/request";
    assert_eq!(one.call("POST", path, None, recovery.clone()).await.0, 202);
    assert_eq!(two.call("POST", path, None, recovery.clone()).await.0, 202);
    // Each server has seen one request; the cluster has seen the limit.
    let (status, body) = one.call("POST", path, None, recovery.clone()).await;
    assert_eq!(status, 429);
    assert_eq!(body["code"], "rate_limit_exceeded");
    assert_eq!(two.call("POST", path, None, recovery).await.0, 429);
    one.stop().await;
    two.stop().await;
    database.drop().await;
}

#[tokio::test]
async fn email_caps_hold_across_servers() {
    let Some(database) = shared_postgres::SharedDatabase::create().await else {
        return; // several servers need a Postgres database
    };
    let limits = RateLimitConfig {
        invitation_emails_per_workspace_per_day: 1,
        ..RateLimitConfig::default()
    };
    let one = Server::start(&database.url, limits).await;
    let two = Server::start(&database.url, limits).await;
    let (root, workspace) = one.setup().await;
    // A mail server that refuses connections: the emails fail after they were counted.
    let smtp = json!({
        "host": "127.0.0.1", "port": 9, "security": "none", "username": null, "password": null,
        "from_address": "orbit@example.com", "from_name": "Orbit"
    });
    let (status, body) = one
        .call("PUT", "/api/v1/admin/settings/smtp", Some(&root), smtp)
        .await;
    assert_eq!(status, 200, "{body}");

    // A password reset link from each server: only the first is issued (the address cooldown).
    let reset = json!({"email": "owner@example.com"});
    let path = "/api/v1/auth/recovery/request";
    assert_eq!(one.call("POST", path, None, reset.clone()).await.0, 202);
    assert_eq!(two.call("POST", path, None, reset).await.0, 202);
    let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recovery_tokens")
        .fetch_one(one.database.pool())
        .await
        .unwrap();
    assert_eq!(links, 1);

    // The workspace's one invitation email of the day was used on server one.
    let invitations = format!("/api/v1/workspaces/{workspace}/invitations");
    let invite = |email: &str| json!({"email": email, "role": "member", "delivery": "smtp"});
    let (status, body) = one
        .call("POST", &invitations, Some(&root), invite("a@example.com"))
        .await;
    assert_eq!(status, 502, "{body}");
    let (status, body) = two
        .call("POST", &invitations, Some(&root), invite("b@example.com"))
        .await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["code"], "invitation_email_limit");
    one.stop().await;
    two.stop().await;
    database.drop().await;
}
