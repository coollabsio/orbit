//! A local fake of github.com and api.github.com for the PR review tests, plus a workspace fixture whose GitHub App
//! points at it. Each test binary includes it with `mod fake_github;`.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::extract::{Form, Path, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key};
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::github::GithubEndpoints;
use orbit_server::integration_routes::{IntegrationState, integration_router};
use orbit_server::repositories::api_tokens::ApiTokenRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::{Value, json};
use tower::ServiceExt;
use url::Url;

pub const KEY: [u8; 32] = [7u8; 32];
pub const ORIGIN: &str = "https://orbit.test";

/// What the fake answers and what it saw.
#[derive(Default)]
pub struct FakeState {
    /// The body of `POST /app-manifests/{code}/conversions`.
    pub conversion: Value,
    /// OAuth `code` → GitHub user (id, login).
    pub codes: HashMap<String, (i64, String)>,
    /// Access token → GitHub user (id, login).
    pub tokens: HashMap<String, (i64, String)>,
    /// Refresh tokens GitHub still accepts; a refresh consumes its token.
    pub valid_refresh: HashSet<String>,
    /// Refresh token → GitHub user.
    pub refresh_users: HashMap<String, (i64, String)>,
    pub refresh_calls: u32,
    /// When set, every refresh gets this answer (status, body) instead.
    pub token_reply: Option<(u16, Value)>,
    /// Sleep inside a refresh, so two refreshes overlap.
    pub refresh_delay_ms: u64,
    /// Every form posted to the token endpoint.
    pub token_forms: Vec<HashMap<String, String>>,
    pub issued: u32,
    /// Issue tokens with an expiry (GitHub's default for new apps).
    pub expiring: bool,
    /// Extra routes for later test files: (method, path) → (status, body). The path is relative to `/github/api/`.
    pub api: HashMap<(String, String), (u16, Value)>,
    /// Every API request: (method, path with query, authorization header, body).
    pub api_requests: Vec<(String, String, String, Value)>,
}

#[derive(Clone)]
pub struct FakeGithub {
    pub base: Url,
    pub state: Arc<Mutex<FakeState>>,
}

impl FakeGithub {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeState {
            expiring: true,
            ..FakeState::default()
        }));
        let router = Router::new()
            .route(
                "/github/api/app-manifests/{code}/conversions",
                post(conversion),
            )
            .route("/github/web/login/oauth/access_token", post(oauth_token))
            .route("/github/api/user", get(user))
            .fallback(api)
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            base: Url::parse(&format!("http://{address}/")).unwrap(),
            state,
        }
    }

    pub fn with<R>(&self, change: impl FnOnce(&mut FakeState) -> R) -> R {
        change(&mut self.state.lock().unwrap())
    }

    /// An access token the fake accepts for this GitHub user.
    pub fn grant(&self, token: &str, user: (i64, &str)) {
        self.with(|state| {
            state
                .tokens
                .insert(token.to_owned(), (user.0, user.1.to_owned()))
        });
    }

    pub fn respond(&self, method: &str, path: &str, status: u16, body: Value) {
        self.with(|state| {
            state
                .api
                .insert((method.to_owned(), path.to_owned()), (status, body))
        });
    }
}

type Shared = Arc<Mutex<FakeState>>;

async fn conversion(State(state): State<Shared>, Path(code): Path<String>) -> Response {
    let body = state.lock().unwrap().conversion.clone();
    if code == "bad" || body.is_null() {
        return StatusCode::NOT_FOUND.into_response();
    }
    (StatusCode::CREATED, Json(body)).into_response()
}

async fn oauth_token(
    State(state): State<Shared>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let delay = {
        let mut state = state.lock().unwrap();
        state.token_forms.push(form.clone());
        if form.get("grant_type").map(String::as_str) == Some("refresh_token") {
            state.refresh_calls += 1;
        }
        state.refresh_delay_ms
    };
    let refreshing = form.get("grant_type").map(String::as_str) == Some("refresh_token");
    let reply = state.lock().unwrap().token_reply.clone();
    if let Some((status, body)) = reply.filter(|_| refreshing) {
        return (StatusCode::from_u16(status).unwrap(), Json(body)).into_response();
    }
    if refreshing && delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    let mut state = state.lock().unwrap();
    let user = if refreshing {
        let old = form.get("refresh_token").cloned().unwrap_or_default();
        if !state.valid_refresh.remove(&old) {
            return Json(json!({ "error": "bad_refresh_token" })).into_response();
        }
        state.refresh_users.get(&old).cloned()
    } else {
        form.get("code").and_then(|code| state.codes.get(code).cloned())
    };
    let Some(user) = user else {
        return Json(json!({ "error": "bad_verification_code" })).into_response();
    };
    state.issued += 1;
    let access = format!("ghu_{}", state.issued);
    let refresh = format!("ghr_{}", state.issued);
    state.tokens.insert(access.clone(), user.clone());
    if state.expiring {
        state.valid_refresh.insert(refresh.clone());
        state.refresh_users.insert(refresh.clone(), user);
        Json(json!({
            "access_token": access, "token_type": "bearer", "scope": "",
            "expires_in": 28800, "refresh_token": refresh, "refresh_token_expires_in": 15897600
        }))
        .into_response()
    } else {
        Json(json!({ "access_token": access, "token_type": "bearer", "scope": "" })).into_response()
    }
}

fn bearer(headers: &HeaderMap) -> String {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

async fn user(State(state): State<Shared>, headers: HeaderMap) -> Response {
    let token = bearer(&headers);
    let token = token.strip_prefix("Bearer ").unwrap_or("");
    match state.lock().unwrap().tokens.get(token) {
        Some((id, login)) => Json(json!({ "id": id, "login": login, "avatar_url": format!("https://avatars.test/{login}") })).into_response(),
        None => (StatusCode::UNAUTHORIZED, Json(json!({ "message": "Bad credentials" }))).into_response(),
    }
}

async fn api(State(state): State<Shared>, request: Request<Body>) -> Response {
    let method = request.method().to_string();
    let uri = request.uri().clone();
    let auth = bearer(request.headers());
    let body = to_bytes(request.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(path) = uri.path().strip_prefix("/github/api/") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let with_query = uri
        .query()
        .map_or_else(|| path.to_owned(), |query| format!("{path}?{query}"));
    let mut state = state.lock().unwrap();
    state
        .api_requests
        .push((method.clone(), with_query.clone(), auth, body));
    let reply = state
        .api
        .get(&(method.clone(), with_query))
        .or_else(|| state.api.get(&(method, path.to_owned())))
        .cloned();
    match reply {
        Some((status, body)) => {
            (StatusCode::from_u16(status).unwrap(), Json(body)).into_response()
        }
        None => (StatusCode::NOT_FOUND, Json(json!({ "message": "Not Found" }))).into_response(),
    }
}

pub fn encrypt_for_test(key: &[u8; 32], value: &str) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let mut encrypted = nonce.to_vec();
    encrypted.extend(cipher.encrypt(&nonce, value.as_bytes()).unwrap());
    encrypted
}

pub fn decrypt_for_test(key: &[u8; 32], value: &[u8]) -> String {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let (nonce, data) = value.split_at(12);
    String::from_utf8(cipher.decrypt(nonce.into(), data).unwrap()).unwrap()
}

/// A workspace (owner + project) with a router whose GitHub endpoints point at a fake.
pub struct GithubFixture {
    pub app: Router,
    pub state: IntegrationState,
    pub database: TestDatabase,
    pub github: FakeGithub,
    pub identity: IdentityRepository,
    pub user_id: Id,
    pub workspace_id: Id,
    pub project_id: Id,
    pub session_cookie: String,
}

fn integration_state(
    database: &TestDatabase,
    identity: &IdentityRepository,
    github: &FakeGithub,
) -> IntegrationState {
    IntegrationState::new(
        Arc::new(ApiTokenRepository::new((**database).clone())),
        Arc::new(TaskRepository::new((**database).clone())),
    )
    .with_github_settings(
        Arc::new(identity.clone()),
        CookieMode::secure(),
        ORIGIN.to_owned(),
        false,
        Some(KEY),
    )
    .with_github_endpoints(GithubEndpoints::local(&github.base))
    .with_native_redirects(&["exp://127.0.0.1:8081/--/auth/callback".to_owned()])
}

pub async fn fixture() -> GithubFixture {
    let database = TestDatabase::new().await.unwrap();
    let identity = IdentityRepository::new((*database).clone());
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
    let github = FakeGithub::start().await;
    let state = integration_state(&database, &identity, &github);
    let app = integration_router(state.clone())
        .layer(HttpPlatformLayer::new(OriginPolicy::new(ORIGIN)));
    GithubFixture {
        app,
        state,
        database,
        github,
        identity,
        user_id: setup.user_id,
        workspace_id: setup.workspace_id,
        project_id: setup.project_id,
        session_cookie: format!("__Host-orbit_session={}", setup.session.token),
    }
}

impl GithubFixture {
    /// The router of a server that has no app key (`ORBIT_APP_KEY` unset).
    pub fn router_without_key(&self) -> Router {
        let state = IntegrationState::new(
            Arc::new(ApiTokenRepository::new((*self.database).clone())),
            Arc::new(TaskRepository::new((*self.database).clone())),
        )
        .with_github_settings(
            Arc::new(self.identity.clone()),
            CookieMode::secure(),
            ORIGIN.to_owned(),
            false,
            None,
        )
        .with_github_endpoints(GithubEndpoints::local(&self.github.base));
        integration_router(state).layer(HttpPlatformLayer::new(OriginPolicy::new(ORIGIN)))
    }

    pub async fn send_to(&self, app: &Router, method: &str, path: &str, cookie: &str) -> Response {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, ORIGIN)
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(request).await.unwrap()
    }

    /// Another server on the same database: its own state, its own process-local locks.
    pub fn second_server(&self) -> IntegrationState {
        integration_state(&self.database, &self.identity, &self.github)
    }

    pub fn workspace(&self) -> String {
        self.workspace_id.to_string()
    }

    /// The workspace's GitHub App (id 77, webhook secret "hook-secret"), installation 1234 and the project linked to
    /// `acme/repo` with label "Orbit". `client` false seeds an app from before PR review (no OAuth client).
    pub async fn seed_app(&self, client: bool) {
        let pool = self.database.pool();
        let workspace = self.workspace();
        sqlx::query("INSERT INTO github_apps (workspace_id, app_id, slug, private_key_encrypted, webhook_secret_encrypted, created_at, client_id, client_secret_encrypted, callback_origin) VALUES ($1, 77, 'orbit-test', $2, $3, 0, $4, $5, $6)")
            .bind(&workspace)
            .bind(encrypt_for_test(&KEY, "private-key"))
            .bind(encrypt_for_test(&KEY, "hook-secret"))
            .bind(client.then_some("Iv1.abc"))
            .bind(client.then(|| encrypt_for_test(&KEY, "s3cret")))
            .bind(client.then_some(ORIGIN))
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO github_installations (installation_id, workspace_id, account_login) VALUES (1234, $1, 'acme')")
            .bind(&workspace).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO github_installation_repositories (installation_id, repository) VALUES (1234, 'acme/repo')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO github_project_connections (project_id, workspace_id, installation_id, repository, label) VALUES ($1, $2, 1234, 'acme/repo', 'Orbit')")
            .bind(self.project_id.to_string()).bind(&workspace).execute(pool).await.unwrap();
    }

    /// A second member of the workspace (role `member`) and their session cookie.
    pub async fn add_member(&self, email: &str) -> (Id, String) {
        let member_id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Member', 'unused', $4, $5)",
        )
        .bind(member_id.to_string())
        .bind(email)
        .bind(email)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(Id::new_v7().to_string())
        .bind(self.workspace())
        .bind(member_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        (member_id, self.session_for(member_id, email).await)
    }

    /// A user outside the workspace and their session cookie.
    pub async fn outsider(&self) -> String {
        let id = Id::new_v7();
        let now = TimestampMillis::now();
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, 'out@example.com', 'out@example.com', 'Out', 'unused', $2, $3)",
        )
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        self.session_for(id, "out@example.com").await
    }

    async fn session_for(&self, id: Id, email: &str) -> String {
        let session = self
            .identity
            .create_session(
                &orbit_platform::AuthenticatedUser {
                    id,
                    email: email.to_owned(),
                    display_name: "Member".to_owned(),
                },
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        format!("__Host-orbit_session={}", session.token)
    }

    /// Stores a GitHub connection for `user` directly (token `access`, optional refresh token and expiry).
    pub async fn connect(
        &self,
        user: Id,
        github: (i64, &str),
        access: &str,
        refresh: Option<&str>,
        access_expires_at: Option<i64>,
    ) {
        sqlx::query("INSERT INTO github_user_connections (workspace_id, user_id, github_user_id, login, access_token_encrypted, access_expires_at, refresh_token_encrypted, refresh_expires_at, broken, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, 0, 0)")
            .bind(self.workspace())
            .bind(user.to_string())
            .bind(github.0)
            .bind(github.1)
            .bind(encrypt_for_test(&KEY, access))
            .bind(access_expires_at)
            .bind(refresh.map(|token| encrypt_for_test(&KEY, token)))
            .bind(refresh.map(|_| TimestampMillis::now().as_millis() + 86_400_000))
            .execute(self.database.pool())
            .await
            .unwrap();
        self.github.grant(access, github);
        if let Some(refresh) = refresh {
            self.github.with(|state| {
                state.valid_refresh.insert(refresh.to_owned());
                state
                    .refresh_users
                    .insert(refresh.to_owned(), (github.0, github.1.to_owned()));
            });
        }
    }

    pub async fn send(&self, method: &str, path: &str, cookie: &str, body: Option<Value>) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, ORIGIN);
        let body = match body {
            Some(body) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        self.app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap()
    }

    pub async fn get(&self, path: &str) -> Response {
        self.send("GET", path, &self.session_cookie, None).await
    }
}

pub async fn json_body(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

pub fn location(response: &Response) -> String {
    response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

pub fn query_param(url: &str, name: &str) -> Option<String> {
    Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}
