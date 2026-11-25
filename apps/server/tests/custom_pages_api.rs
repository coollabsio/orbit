use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::CookieMode;
use orbit_server::custom_page_routes::{CustomPageState, custom_page_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "https://orbit.test";

struct Fixture {
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    workspace_id: String,
    owner_cookie: String,
    owner_id: Id,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "operator-secret".to_owned(),
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
        let app = custom_page_router(CustomPageState::new(
            Arc::clone(&identity),
            CookieMode::secure(),
        ))
        .merge(workspace_router(WorkspaceState::new(
            Arc::clone(&identity),
            ORIGIN.to_owned(),
            CookieMode::secure(),
        )));
        let fixture = Self {
            database,
            identity,
            app,
            workspace_id: setup.workspace_id.to_string(),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
            owner_id: setup.user_id,
        };
        // the owner of the first workspace is the root user, who saves the origins in Admin
        let (status, settings) = fixture
            .set_origins(
                &fixture.owner_cookie,
                json!([
                    "https://grafana.example.com",
                    "https://BOARD.example.com:443/"
                ]),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{settings}");
        fixture
    }

    async fn set_origins(&self, cookie: &str, origins: Value) -> (StatusCode, Value) {
        self.call(
            cookie,
            "PUT",
            "/api/v1/admin/settings/frame-origins",
            Some(json!({ "origins": origins })),
        )
        .await
    }

    fn uri(&self) -> String {
        format!("/api/v1/workspaces/{}/custom-pages", self.workspace_id)
    }

    fn page_uri(&self, page: &Value) -> String {
        format!("{}/{}", self.uri(), page["id"].as_str().unwrap())
    }

    async fn call(
        &self,
        cookie: &str,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, ORIGIN);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    async fn create(&self, body: Value) -> Value {
        let (status, page) = self
            .call(&self.owner_cookie, "POST", &self.uri(), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page
    }

    async fn list(&self, cookie: &str) -> Value {
        let (status, list) = self.call(cookie, "GET", &self.uri(), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        list
    }

    async fn add_member(&self, workspace_id: &str, email: &str, role: &str) -> String {
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
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 0, $5, $6)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace_id)
        .bind(id.to_string())
        .bind(role)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        let session = self
            .identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email: email.to_owned(),
                    display_name: "Member".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        format!("__Host-orbit_session={}", session.token)
    }
}

fn slugs(list: &Value) -> Vec<&str> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|page| page["slug"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn admins_add_change_and_remove_pages_on_the_allowed_origins() {
    let fixture = Fixture::new().await;
    let list = fixture.list(&fixture.owner_cookie).await;
    assert_eq!(list["items"], json!([]));
    assert_eq!(list["can_manage"], true);
    assert_eq!(
        list["frame_origins"],
        json!(["https://grafana.example.com", "https://board.example.com"]),
        "origins are normalized as the browser writes them"
    );

    let deploys = fixture
        .create(json!({
            "slug": "deploys",
            "name": "  Deploys  ",
            "icon": "🚀",
            "url": "https://grafana.example.com/d/abc?orgId=1",
        }))
        .await;
    assert_eq!(deploys["name"], "Deploys");
    assert_eq!(deploys["url"], "https://grafana.example.com/d/abc?orgId=1");
    assert_eq!(deploys["admins_only"], false);
    assert_eq!(deploys["version"], 0);
    fixture
        .create(json!({"slug": "board", "name": "Board", "url": "https://board.example.com"}))
        .await;
    assert_eq!(
        slugs(&fixture.list(&fixture.owner_cookie).await),
        ["board", "deploys"]
    );

    let (status, updated) = fixture
        .call(
            &fixture.owner_cookie,
            "PATCH",
            &fixture.page_uri(&deploys),
            Some(json!({"expected_version": 0, "slug": "ship", "icon": null, "admins_only": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["slug"], "ship");
    assert_eq!(updated["icon"], Value::Null);
    assert_eq!(updated["admins_only"], true);
    assert_eq!(updated["version"], 1);
    assert_eq!(updated["url"], deploys["url"]);

    let (status, conflict) = fixture
        .call(
            &fixture.owner_cookie,
            "PATCH",
            &fixture.page_uri(&deploys),
            Some(json!({"expected_version": 0, "name": "Stale"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["code"], "conflict");

    let (status, taken) = fixture
        .call(
            &fixture.owner_cookie,
            "PATCH",
            &fixture.page_uri(&deploys),
            Some(json!({"expected_version": 1, "slug": "board"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{taken}");
    assert_eq!(taken["code"], "custom_page_slug_taken");

    let (status, _) = fixture
        .call(
            &fixture.owner_cookie,
            "DELETE",
            &format!("{}?expected_version=1", fixture.page_uri(&deploys)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(slugs(&fixture.list(&fixture.owner_cookie).await), ["board"]);
}

#[tokio::test]
async fn page_urls_must_be_https_on_an_operator_origin() {
    let fixture = Fixture::new().await;
    for (url, code) in [
        (
            "https://evil.example.com/",
            "custom_page_origin_not_allowed",
        ),
        (
            "https://grafana.example.com:8443/",
            "custom_page_origin_not_allowed",
        ),
        (
            "https://grafana.example.com.evil.test/",
            "custom_page_origin_not_allowed",
        ),
        ("http://grafana.example.com/", "validation_failed"),
        ("javascript:alert(1)", "validation_failed"),
        ("https://user:pw@grafana.example.com/", "validation_failed"),
        ("/relative", "validation_failed"),
    ] {
        let (status, problem) = fixture
            .call(
                &fixture.owner_cookie,
                "POST",
                &fixture.uri(),
                Some(json!({"slug": "x", "name": "X", "url": url})),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{url}: {problem}");
        assert_eq!(problem["code"], code, "{url}");
    }
    for slug in ["", "Deploys", "a--b", "-a", "a-", "a/b", &"a".repeat(49)] {
        let (status, problem) = fixture
            .call(
                &fixture.owner_cookie,
                "POST",
                &fixture.uri(),
                Some(json!({"slug": slug, "name": "X", "url": "https://grafana.example.com"})),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{slug}: {problem}"
        );
    }
    // The host is case-insensitive and the default port is implied.
    let page = fixture
        .create(json!({"slug": "x", "name": "X", "url": "https://GRAFANA.example.com:443/a b"}))
        .await;
    assert_eq!(page["url"], "https://grafana.example.com/a%20b");
}

#[tokio::test]
async fn members_see_shared_pages_only_and_cannot_manage_them() {
    let fixture = Fixture::new().await;
    let shared = fixture
        .create(json!({"slug": "kpis", "name": "KPIs", "url": "https://grafana.example.com/kpi"}))
        .await;
    fixture
        .create(json!({
            "slug": "oncall",
            "name": "On-call",
            "url": "https://grafana.example.com/oncall",
            "admins_only": true,
        }))
        .await;
    let workspace_id = fixture.workspace_id.clone();
    let member = fixture
        .add_member(&workspace_id, "member@example.com", "member")
        .await;
    let admin = fixture
        .add_member(&workspace_id, "admin@example.com", "admin")
        .await;
    let guest = fixture
        .add_member(&workspace_id, "guest@example.com", "guest")
        .await;

    let list = fixture.list(&member).await;
    assert_eq!(slugs(&list), ["kpis"]);
    assert_eq!(list["can_manage"], false);
    assert_eq!(slugs(&fixture.list(&admin).await), ["kpis", "oncall"]);

    for (method, uri, body) in [
        (
            "POST",
            fixture.uri(),
            Some(json!({"slug": "mine", "name": "Mine", "url": "https://grafana.example.com"})),
        ),
        (
            "PATCH",
            fixture.page_uri(&shared),
            Some(json!({"expected_version": 0, "name": "Mine"})),
        ),
        (
            "DELETE",
            format!("{}?expected_version=0", fixture.page_uri(&shared)),
            None,
        ),
    ] {
        let (status, problem) = fixture.call(&member, method, &uri, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}: {problem}");
        assert_eq!(problem["code"], "workspace_action_forbidden");
    }

    let (status, problem) = fixture.call(&guest, "GET", &fixture.uri(), None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "guests see no pages: {problem}"
    );
}

#[tokio::test]
async fn pages_are_isolated_between_workspaces() {
    let fixture = Fixture::new().await;
    let page = fixture
        .create(json!({"slug": "kpis", "name": "KPIs", "url": "https://grafana.example.com/kpi"}))
        .await;
    let foreign = WorkspaceRepository::new((*fixture.database).clone())
        .create(
            fixture.owner_id,
            "Foreign".to_owned(),
            "custom-pages-test",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .id
        .to_string();
    let outsider = fixture
        .add_member(&foreign, "outsider@example.com", "admin")
        .await;

    let (status, _) = fixture.call(&outsider, "GET", &fixture.uri(), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The page's id under the outsider's own workspace does not reach it either.
    let (status, problem) = fixture
        .call(
            &outsider,
            "PATCH",
            &format!(
                "/api/v1/workspaces/{foreign}/custom-pages/{}",
                page["id"].as_str().unwrap()
            ),
            Some(json!({"expected_version": 0, "name": "Hijacked"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{problem}");
    assert_eq!(problem["code"], "custom_page_not_found");
    assert_eq!(slugs(&fixture.list(&fixture.owner_cookie).await), ["kpis"]);
    assert_eq!(
        fixture.list(&fixture.owner_cookie).await["items"][0]["name"],
        "KPIs"
    );
}

#[tokio::test]
async fn the_root_user_changes_the_origins_while_the_server_runs() {
    let fixture = Fixture::new().await;
    let page = json!({"slug": "status", "name": "Status", "url": "https://status.example.com/"});
    let (status, problem) = fixture
        .call(
            &fixture.owner_cookie,
            "POST",
            &fixture.uri(),
            Some(page.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["code"], "custom_page_origin_not_allowed");

    let (status, settings) = fixture
        .set_origins(&fixture.owner_cookie, json!(["https://status.example.com"]))
        .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(
        settings["frame_origins"],
        json!(["https://status.example.com"])
    );
    assert_eq!(settings["environment"]["frame_origins"], false);
    assert_eq!(
        orbit_server::frame_origins::saved(&fixture.database)
            .await
            .unwrap(),
        ["https://status.example.com"]
    );
    // the CSP and the page checks share the list: the new origin works at once, the removed one does not
    assert_eq!(
        &*orbit_server::frame_origins::of(&fixture.database).get(),
        ["https://status.example.com".to_owned()]
    );
    assert_eq!(
        fixture.list(&fixture.owner_cookie).await["frame_origins"],
        json!(["https://status.example.com"])
    );
    fixture.create(page).await;
    let (status, problem) = fixture
        .call(
            &fixture.owner_cookie,
            "POST",
            &fixture.uri(),
            Some(json!({"slug": "kpis", "name": "KPIs", "url": "https://grafana.example.com/"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");

    for bad in [
        json!(["http://status.example.com"]),
        json!(["https://status.example.com/d/1"]),
        json!(["https://*.example.com"]),
        json!([ORIGIN]),
    ] {
        let (status, problem) = fixture
            .set_origins(&fixture.owner_cookie, bad.clone())
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {problem}");
        assert_eq!(problem["code"], "invalid_frame_origin");
    }

    // a workspace admin is not the installation's administrator
    let workspace_id = fixture.workspace_id.clone();
    let admin = fixture
        .add_member(&workspace_id, "admin@example.com", "admin")
        .await;
    let (status, problem) = fixture.set_origins(&admin, json!([])).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{problem}");
    assert_eq!(
        &*orbit_server::frame_origins::of(&fixture.database).get(),
        ["https://status.example.com".to_owned()]
    );
}

#[tokio::test]
async fn orbit_frame_origins_replaces_the_saved_list_and_locks_it() {
    let fixture = Fixture::new().await;
    orbit_server::repositories::environment_settings::EnvironmentSettings::from_config(
        &orbit_platform::InstanceConfig {
            frame_origins: Some(vec!["https://env.example.com".to_owned()]),
            ..orbit_platform::InstanceConfig::default()
        },
        &[7; 32],
    )
    .unwrap()
    .install(&fixture.database);
    orbit_server::frame_origins::reload(&fixture.database)
        .await
        .unwrap();
    assert_eq!(
        fixture.list(&fixture.owner_cookie).await["frame_origins"],
        json!(["https://env.example.com"])
    );
    let (status, problem) = fixture
        .set_origins(
            &fixture.owner_cookie,
            json!(["https://grafana.example.com"]),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert_eq!(problem["code"], "set_by_environment");
    // the saved list stays for the day the variable is removed
    assert_eq!(
        orbit_server::frame_origins::saved(&fixture.database)
            .await
            .unwrap(),
        ["https://grafana.example.com", "https://board.example.com"]
    );
}
