use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use orbit_platform::{
    HttpPlatformLayer, Id, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mcp::{McpState, router as mcp_router};
use orbit_server::oauth::{OAuthState, router};
use orbit_server::repositories::api_tokens::ApiTokenRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;

const ORIGIN: &str = "https://orbit.test";
const CALLBACK: &str = "http://127.0.0.1:6274/callback";
const VERIFIER: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~";
struct Fixture {
    db: TestDatabase,
    app: axum::Router,
    cookie: String,
    workspace: Id,
    project: Id,
    user: Id,
    oauth: OAuthState,
}
impl Fixture {
    async fn new() -> Self {
        let db = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*db).clone()));
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
                    token: "setup".into(),
                    email: "owner@example.com".into(),
                    display_name: "Owner".into(),
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".into(),
                    project_name: "General".into(),
                },
                now,
            )
            .await
            .unwrap();
        let oauth = OAuthState::new(identity, CookieMode::secure(), ORIGIN);
        let mcp = mcp_router(
            McpState::new(
                Arc::new(ApiTokenRepository::new((*db).clone())),
                Arc::new(TaskRepository::new((*db).clone())),
            )
            .with_oauth(oauth.clone()),
            ORIGIN,
        );
        let app = router(oauth.clone())
            .merge(mcp)
            .layer(HttpPlatformLayer::new(OriginPolicy::new(ORIGIN)));
        Self {
            db,
            app,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            workspace: setup.workspace_id,
            project: setup.project_id,
            user: setup.user_id,
            oauth,
        }
    }
    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Value,
        session: bool,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "orbit.test")
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json");
        if session {
            req = req.header(header::COOKIE, &self.cookie);
        }
        decode(
            self.app
                .clone()
                .oneshot(req.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap(),
        )
        .await
    }
    async fn register(&self) -> String {
        let (status, body) = self.send("POST", "/oauth/register", json!({"client_name":"Test client","redirect_uris":[CALLBACK],"token_endpoint_auth_method":"none"}), false).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["client_id"].as_str().unwrap().into()
    }
    async fn pending(&self, client: &str) -> String {
        let challenge =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER));
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("client_id", client),
                ("response_type", "code"),
                ("redirect_uri", CALLBACK),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("state", "state-value"),
                ("resource", "https://orbit.test/mcp"),
                ("scope", "tasks:read"),
            ])
            .finish();
        let response = self
            .app
            .clone()
            .oneshot(
                Request::get(format!("/oauth/authorize?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location =
            url::Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        location
            .query_pairs()
            .find(|(key, _)| key == "request")
            .unwrap()
            .1
            .to_string()
    }
    async fn code(&self, client: &str) -> String {
        let id = self.pending(client).await;
        let (status, consent) = self
            .send(
                "GET",
                &format!("/api/v1/oauth/consent/{id}"),
                json!({}),
                true,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{consent}");
        let (status, result) = self.send("POST", &format!("/api/v1/oauth/consent/{id}"), json!({"approved":true,"csrf_token":consent["csrf_token"],"workspace_id":self.workspace,"project_ids":[self.project]}), true).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        let callback = url::Url::parse(result["redirect_uri"].as_str().unwrap()).unwrap();
        assert_eq!(
            callback
                .query_pairs()
                .find(|(key, _)| key == "state")
                .unwrap()
                .1,
            "state-value"
        );
        assert_eq!(
            callback
                .query_pairs()
                .find(|(key, _)| key == "iss")
                .unwrap()
                .1,
            ORIGIN
        );
        callback
            .query_pairs()
            .find(|(key, _)| key == "code")
            .unwrap()
            .1
            .to_string()
    }
    async fn exchange(&self, fields: &[(&str, &str)]) -> (StatusCode, Value) {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().copied())
            .finish();
        let response = self
            .app
            .clone()
            .oneshot(
                Request::post("/oauth/token")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        decode(response).await
    }
    async fn access(&self, token: &str) -> (StatusCode, Value) {
        decode(self.app.clone().oneshot(Request::post("/mcp").header(header::HOST,"orbit.test").header(header::AUTHORIZATION,format!("Bearer {token}")).header(header::CONTENT_TYPE,"application/json").header(header::ACCEPT,"application/json, text/event-stream").header("mcp-protocol-version","2025-11-25").body(Body::from(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}).to_string())).unwrap()).await.unwrap()).await
    }
    async fn issue(&self, client: &str, code: &str) -> Value {
        let (status, result) = self
            .exchange(&[
                ("grant_type", "authorization_code"),
                ("client_id", client),
                ("code", code),
                ("redirect_uri", CALLBACK),
                ("code_verifier", VERIFIER),
                ("resource", "https://orbit.test/mcp"),
            ])
            .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        result
    }
}
async fn decode(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)}))
    };
    (status, value)
}

#[tokio::test]
async fn discovery_and_authorization_code_flow_issue_scoped_read_tokens() {
    let f = Fixture::new().await;
    let response = f
        .app
        .clone()
        .oneshot(Request::post("/mcp").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response.headers()[header::WWW_AUTHENTICATE]
            .to_str()
            .unwrap()
            .contains(
                "resource_metadata=\"https://orbit.test/.well-known/oauth-protected-resource/mcp\""
            )
    );
    let (_, metadata) = f
        .send(
            "GET",
            "/.well-known/oauth-authorization-server",
            json!({}),
            false,
        )
        .await;
    assert_eq!(metadata["issuer"], ORIGIN);
    assert_eq!(
        metadata["code_challenge_methods_supported"],
        json!(["S256"])
    );
    let client = f.register().await;
    let code = f.code(&client).await;
    let tokens = f.issue(&client, &code).await;
    let (status, projects) = f.access(tokens["access_token"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{projects}");
    assert_eq!(
        projects["result"]["structuredContent"]["items"][0]["id"],
        f.project.to_string()
    );
    let grant = f
        .oauth
        .authenticate(tokens["access_token"].as_str().unwrap())
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(grant.project_ids, vec![f.project]);
    assert_eq!(tokens["scope"], "tasks:read");
    let raw: Vec<u8> = sqlx::query_scalar("SELECT access_hash FROM oauth_tokens")
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    assert_eq!(raw.len(), 32);
    assert_ne!(raw, tokens["access_token"].as_str().unwrap().as_bytes());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='oauth.grant.created'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn code_exchange_binds_pkce_client_redirect_resource_and_single_use() {
    let f = Fixture::new().await;
    let client = f.register().await;
    let code = f.code(&client).await;
    for (key, value) in [
        ("code_verifier", "z".repeat(64)),
        ("client_id", "wrong".into()),
        ("redirect_uri", "http://127.0.0.1:6274/other".into()),
        ("resource", "https://evil.test/mcp".into()),
    ] {
        let mut fields = vec![
            ("grant_type", "authorization_code"),
            ("client_id", client.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", CALLBACK),
            ("code_verifier", VERIFIER),
            ("resource", "https://orbit.test/mcp"),
        ];
        fields.iter_mut().find(|(k, _)| *k == key).unwrap().1 = &value;
        let (status, _) = f.exchange(&fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let token = f.issue(&client, &code).await;
    let (status, _) = f
        .exchange(&[
            ("grant_type", "authorization_code"),
            ("client_id", &client),
            ("code", &code),
            ("redirect_uri", CALLBACK),
            ("code_verifier", VERIFIER),
        ])
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        f.access(token["access_token"].as_str().unwrap()).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn refresh_rotation_preserves_projects_and_reuse_revokes_the_family() {
    let f = Fixture::new().await;
    let client = f.register().await;
    let code = f.code(&client).await;
    let first = f.issue(&client, &code).await;
    let refresh = first["refresh_token"].as_str().unwrap();
    let (status, second) = f
        .exchange(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client),
            ("refresh_token", refresh),
        ])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(first["refresh_token"], second["refresh_token"]);
    assert_eq!(
        f.access(second["access_token"].as_str().unwrap()).await.0,
        StatusCode::OK
    );
    let (status, _) = f
        .exchange(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client),
            ("refresh_token", refresh),
        ])
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        f.access(second["access_token"].as_str().unwrap()).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn consent_requires_login_nonce_and_valid_projects_and_supports_denial() {
    let f = Fixture::new().await;
    let client = f.register().await;
    let id = f.pending(&client).await;
    let path = format!("/api/v1/oauth/consent/{id}");
    assert_eq!(
        f.send("GET", &path, json!({}), false).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, consent) = f.send("GET", &path, json!({}), true).await;
    for (nonce, project) in [
        (json!("wrong"), f.project),
        (consent["csrf_token"].clone(), Id::new_v7()),
    ] {
        assert_eq!(f.send("POST",&path,json!({"approved":true,"csrf_token":nonce,"workspace_id":f.workspace,"project_ids":[project]}),true).await.0,StatusCode::BAD_REQUEST);
    }
    let (_, denied) = f
        .send(
            "POST",
            &path,
            json!({"approved":false,"csrf_token":consent["csrf_token"]}),
            true,
        )
        .await;
    assert!(
        denied["redirect_uri"]
            .as_str()
            .unwrap()
            .contains("error=access_denied")
    );
    assert_eq!(
        f.send(
            "POST",
            &path,
            json!({"approved":false,"csrf_token":consent["csrf_token"]}),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn access_expiry_refresh_and_user_disconnect_work_after_state_recreation() {
    let f = Fixture::new().await;
    let client = f.register().await;
    let code = f.code(&client).await;
    let first = f.issue(&client, &code).await;
    sqlx::query("UPDATE oauth_tokens SET access_expires_at=1")
        .execute(f.db.pool())
        .await
        .unwrap();
    assert_eq!(
        f.access(first["access_token"].as_str().unwrap()).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, second) = f
        .exchange(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client),
            ("refresh_token", first["refresh_token"].as_str().unwrap()),
        ])
        .await;
    assert_eq!(status, StatusCode::OK);
    let recreated = OAuthState::new(
        Arc::new(IdentityRepository::new((*f.db).clone())),
        CookieMode::secure(),
        ORIGIN,
    );
    assert!(
        recreated
            .authenticate(second["access_token"].as_str().unwrap())
            .await
            .ok()
            .flatten()
            .is_some()
    );
    let (_, grants) = f.send("GET", "/api/v1/oauth/grants", json!({}), true).await;
    let id = grants["items"][0]["id"].as_str().unwrap();
    assert_eq!(
        f.send(
            "DELETE",
            &format!("/api/v1/oauth/grants/{id}"),
            json!({}),
            true
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.access(second["access_token"].as_str().unwrap()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.exchange(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client),
            ("refresh_token", second["refresh_token"].as_str().unwrap())
        ])
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn registration_rejects_unsafe_callbacks_and_public_cors_does_not_apply_to_consent() {
    let f = Fixture::new().await;
    for uri in [
        "http://evil.test/callback",
        "javascript:alert(1)",
        "https://evil.test/callback#fragment",
        "https://user:password@evil.test/callback",
    ] {
        assert_eq!(
            f.send(
                "POST",
                "/oauth/register",
                json!({"redirect_uris":[uri]}),
                false
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let preflight = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/oauth/token")
                .header(header::ORIGIN, "http://localhost:6274")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        preflight.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "*"
    );
    let consent = f
        .app
        .clone()
        .oneshot(
            Request::post("/api/v1/oauth/consent/bad")
                .header(header::ORIGIN, "https://evil.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(consent.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn user_suspension_invalidates_both_access_and_refresh() {
    let f = Fixture::new().await;
    let client = f.register().await;
    let code = f.code(&client).await;
    let tokens = f.issue(&client, &code).await;
    sqlx::query("UPDATE users SET suspended_at=1 WHERE id=$1")
        .bind(f.user.to_string())
        .execute(f.db.pool())
        .await
        .unwrap();
    assert_eq!(
        f.access(tokens["access_token"].as_str().unwrap()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.exchange(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client),
            ("refresh_token", tokens["refresh_token"].as_str().unwrap())
        ])
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

async fn register_from(f: &Fixture, ip: &str) -> (StatusCode, Value) {
    let mut request = Request::post("/oauth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"client_name":"Test client","redirect_uris":[CALLBACK]}).to_string(),
        ))
        .unwrap();
    let peer: std::net::SocketAddr = format!("{ip}:40000").parse().unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(peer));
    decode(f.app.clone().oneshot(request).await.unwrap()).await
}

async fn client_exists(f: &Fixture, client: &str) -> bool {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_clients WHERE id = $1")
        .bind(client)
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    count == 1
}

#[tokio::test]
async fn one_address_cannot_fill_the_client_registry() {
    let f = Fixture::new().await;
    for _ in 0..20 {
        assert_eq!(
            register_from(&f, "203.0.113.7").await.0,
            StatusCode::CREATED
        );
    }
    let (status, body) = register_from(&f, "203.0.113.7").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"], "temporarily_unavailable");
    // Another address, and the other half of an IPv6 /64 counts as the same client.
    assert_eq!(
        register_from(&f, "198.51.100.1").await.0,
        StatusCode::CREATED
    );
    for _ in 0..20 {
        assert_eq!(
            register_from(&f, "[2001:db8::1]").await.0,
            StatusCode::CREATED
        );
    }
    assert_eq!(
        register_from(&f, "[2001:db8::ffff]").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    // A day later the address may register again.
    sqlx::query("UPDATE auth_throttle_ips SET failed_at = failed_at - 86400001")
        .execute(f.db.pool())
        .await
        .unwrap();
    assert_eq!(
        register_from(&f, "203.0.113.7").await.0,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn unused_clients_are_pruned_and_live_ones_kept() {
    let f = Fixture::new().await;
    let day_ago = TimestampMillis::now().as_millis() - 86_400_001;
    let unused = f.register().await;
    let recent = f.register().await;
    let connected = f.register().await;
    let code = f.code(&connected).await;
    f.issue(&connected, &code).await;
    let revoked = f.register().await;
    let code = f.code(&revoked).await;
    f.issue(&revoked, &code).await;
    for client in [&unused, &connected, &revoked] {
        sqlx::query("UPDATE oauth_clients SET created_at = $1 WHERE id = $2")
            .bind(day_ago)
            .bind(client)
            .execute(f.db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE oauth_grants SET created_at = $1 WHERE client_id = $2")
            .bind(day_ago)
            .bind(client)
            .execute(f.db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE oauth_requests SET expires_at = 1 WHERE client_id = $1")
            .bind(client)
            .execute(f.db.pool())
            .await
            .unwrap();
    }
    sqlx::query("UPDATE oauth_grants SET revoked_at = $1 WHERE client_id = $2")
        .bind(day_ago)
        .bind(&revoked)
        .execute(f.db.pool())
        .await
        .unwrap();

    assert_eq!(orbit_server::oauth::prune_clients(&f.db).await.unwrap(), 2);
    assert!(!client_exists(&f, &unused).await);
    assert!(!client_exists(&f, &revoked).await);
    assert!(client_exists(&f, &recent).await);
    assert!(client_exists(&f, &connected).await);
}

#[tokio::test]
async fn a_full_registry_prunes_unused_clients_before_it_refuses() {
    let f = Fixture::new().await;
    let connected = f.register().await;
    let code = f.code(&connected).await;
    f.issue(&connected, &code).await;
    // Unused clients registered an hour ago fill the registry.
    let hour_ago = TimestampMillis::now().as_millis() - 60 * 60 * 1000;
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 9999) \
         INSERT INTO oauth_clients (id, name, redirect_uris, created_at) \
         SELECT 'filler-' || i, 'Filler', '[]', $1 FROM n",
    )
    .bind(hour_ago)
    .execute(f.db.pool())
    .await
    .unwrap();
    assert_eq!(
        register_from(&f, "198.51.100.2").await.0,
        StatusCode::CREATED
    );
    assert!(client_exists(&f, &connected).await);

    // Clients that may still be in an authorization are not pruned: then registration stops.
    let now = TimestampMillis::now().as_millis();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 9998) \
         INSERT INTO oauth_clients (id, name, redirect_uris, created_at) \
         SELECT 'fresh-' || i, 'Fresh', '[]', $1 FROM n",
    )
    .bind(now)
    .execute(f.db.pool())
    .await
    .unwrap();
    let (status, body) = register_from(&f, "198.51.100.3").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "temporarily_unavailable");
}
