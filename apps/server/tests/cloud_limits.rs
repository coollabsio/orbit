//! Request limits on two servers of one Postgres database (Orbit Cloud): they must
//! hold for the cluster, not for each server.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_platform::{Config, EnvironmentMode, RateLimitConfig};
use orbit_server::app::App;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

mod shared_postgres;

const COOKIE: &str = "orbit_session_dev";

struct Server {
    base: String,
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
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });
        Self {
            base,
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
