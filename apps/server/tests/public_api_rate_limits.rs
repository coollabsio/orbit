//! Request limits of the public API (`/api/public/v1`) on the whole app: its own budget, apart
//! from the web app's, and requests refused before authentication do not use up a workspace's
//! budget. SQLite by default, Postgres when `ORBIT_TEST_POSTGRES_URL` is set.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use orbit_platform::{Config, EnvironmentMode, PasswordService, RateLimitConfig, TimestampMillis};
use orbit_server::app::App;
use orbit_server::repositories::api_tokens::{ApiTokenRepository, ApiTokenScope};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use tempfile::TempDir;
use tower::ServiceExt;

mod shared_postgres;

const COOKIE: &str = "orbit_session_dev";
const ORIGIN: &str = "http://127.0.0.1:8080";

struct Server {
    app: Option<App>,
    postgres: Option<shared_postgres::SharedDatabase>,
    session: String,
    token: String,
    workspace: String,
    /// The last client address used; each request comes from a new one, so only the
    /// workspace budget is shared between them.
    host: u32,
    _root: TempDir,
}

/// The general limit; with a workspace multiplier of 1 it is also each workspace's budget.
const LIMIT: u32 = 3;

impl Server {
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            rate_limits: RateLimitConfig {
                general_per_minute: LIMIT,
                workspace_multiplier: 1,
                ..RateLimitConfig::default()
            },
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        let postgres = shared_postgres::SharedDatabase::create().await;
        if let Some(postgres) = &postgres {
            config.data.database_url = Some(postgres.url.clone());
        }
        config.app_key = Some(shared_postgres::APP_KEY.parse().unwrap());
        let app = App::build(config).await.unwrap();

        let identity = IdentityRepository::new(app.database().clone());
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "setup",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "setup".to_owned(),
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
        let token = ApiTokenRepository::new(app.database().clone())
            .create(
                setup.workspace_id,
                setup.user_id,
                "cli".to_owned(),
                vec![setup.project_id],
                &[ApiTokenScope::TasksRead],
                false,
                None,
                "test",
                now,
            )
            .await
            .unwrap()
            .token;
        Self {
            app: Some(app),
            postgres,
            session: setup.session.token,
            token,
            workspace: setup.workspace_id.to_string(),
            host: 0,
            _root: root,
        }
    }

    async fn stop(mut self) {
        drop(self.app.take());
        if let Some(postgres) = self.postgres.take() {
            postgres.drop().await;
        }
    }

    /// Sends `method path` (a path of the workspace) from a new client address.
    async fn call(&mut self, method: &str, path: &str, credential: Credential<'_>) -> StatusCode {
        self.host += 1;
        let peer = SocketAddr::from((Ipv4Addr::from(0x0a00_0000 + self.host), 4567));
        let mut request = Request::builder()
            .method(method)
            .uri(path.replace("{W}", &self.workspace))
            .header(header::ORIGIN, ORIGIN)
            .extension(ConnectInfo(peer));
        request = match credential {
            Credential::Token(token) => {
                request.header(header::AUTHORIZATION, format!("Bearer {token}"))
            }
            Credential::Session(session) => {
                request.header(header::COOKIE, format!("{COOKIE}={session}"))
            }
        };
        let request = request.body(Body::empty()).unwrap();
        let router = self.app.as_ref().unwrap().router();
        router.oneshot(request).await.unwrap().status()
    }
}

#[derive(Clone, Copy)]
enum Credential<'a> {
    Token(&'a str),
    Session(&'a str),
}

/// Waits for the next minute when this one ends soon, so the requests of a test share a window.
async fn early_in_a_minute() {
    let second = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        % 60;
    if second > 45 {
        tokio::time::sleep(Duration::from_secs(61 - second)).await;
    }
}

const PUBLIC_PROJECTS: &str = "/api/public/v1/workspaces/{W}/projects";
const WEB_PROJECTS: &str = "/api/v1/workspaces/{W}/projects";

#[tokio::test]
async fn public_api_traffic_does_not_use_up_the_web_app_budget() {
    let mut server = Server::start().await;
    early_in_a_minute().await;
    let token = server.token.clone();
    for _ in 0..LIMIT {
        let status = server
            .call("GET", PUBLIC_PROJECTS, Credential::Token(&token))
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    // The workspace's public API budget is used up…
    let status = server
        .call("GET", PUBLIC_PROJECTS, Credential::Token(&token))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // …and its members still use the web app.
    let session = server.session.clone();
    for _ in 0..LIMIT {
        let status = server
            .call("GET", WEB_PROJECTS, Credential::Session(&session))
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    server.stop().await;
}

#[tokio::test]
async fn requests_refused_before_authentication_do_not_use_up_a_workspace_budget() {
    let mut server = Server::start().await;
    early_in_a_minute().await;
    let token = server.token.clone();
    let session = server.session.clone();
    // Made-up credentials from many addresses, refused by the router (405) or by an extractor
    // that runs before the credential is checked (400). None may use up the workspace's budget:
    // after more of them than the budget, a member's request still gets through.
    let cases = [
        (
            "PUT",
            "/api/public/v1/workspaces/{W}/tasks",
            StatusCode::METHOD_NOT_ALLOWED,
            PUBLIC_PROJECTS,
        ),
        (
            "PUT",
            "/api/v1/workspaces/{W}/tasks",
            StatusCode::METHOD_NOT_ALLOWED,
            WEB_PROJECTS,
        ),
        (
            "GET",
            "/api/v1/workspaces/{W}/tasks?unassigned=maybe",
            StatusCode::BAD_REQUEST,
            WEB_PROJECTS,
        ),
    ];
    for (method, path, refused, member_path) in cases {
        let public = path.starts_with("/api/public/");
        for n in 0..=LIMIT {
            let fake = format!("orb_made_up_{n}");
            let credential = if public {
                Credential::Token(&fake)
            } else {
                Credential::Session(&fake)
            };
            let status = server.call(method, path, credential).await;
            assert_eq!(status, refused, "{method} {path}, made-up credential {n}");
        }
        let member = if public {
            Credential::Token(&token)
        } else {
            Credential::Session(&session)
        };
        let status = server.call("GET", member_path, member).await;
        assert_eq!(status, StatusCode::OK, "member after {method} {path}");
    }
    server.stop().await;
}
