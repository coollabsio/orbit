use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key};
use hmac::{Hmac, Mac};
use orbit_platform::{
    HttpPlatformLayer, OriginPolicy, PasswordService, TestDatabase, TimestampMillis,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::integration_routes::{IntegrationState, integration_router};
use orbit_server::repositories::api_tokens::ApiTokenRepository;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::TaskRepository;
use serde_json::{Value, json};
use sha2::Sha256;
use tower::ServiceExt;

#[tokio::test]
async fn database_github_app_routes_issues_without_orbit_tokens() {
    let fixture = fixture().await;
    let key = [7u8; 32];
    let state = IntegrationState::new(
        Arc::new(fixture.tokens.clone()),
        Arc::new(TaskRepository::new((*fixture.database).clone())),
    )
    .with_github_settings(
        Arc::new(IdentityRepository::new((*fixture.database).clone())),
        CookieMode::secure(),
        "https://orbit.test".to_owned(),
        false,
        Some(key),
    );
    let app = integration_router(state).layer(HttpPlatformLayer::new(OriginPolicy::new(
        "https://orbit.test",
    )));
    let path = format!(
        "/api/v1/workspaces/{}/projects/{}/github",
        fixture.workspace_id, fixture.project_id
    );
    let workspace_path = format!("/api/v1/workspaces/{}/github", fixture.workspace_id);
    let register = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{workspace_path}/manifest"))
                .header(header::COOKIE, &fixture.session_cookie)
                .header(header::ORIGIN, "https://orbit.test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"organization":"acme"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(register.status(), StatusCode::OK);
    let manifest = response_json(register).await;
    assert!(
        manifest["action"]
            .as_str()
            .unwrap()
            .starts_with("https://github.com/organizations/acme/settings/apps/new?state=")
    );
    assert_eq!(
        manifest["manifest"]["default_events"],
        json!(["issues", "pull_request"])
    );
    assert_eq!(manifest["manifest"]["public"], false);
    assert_eq!(
        manifest["manifest"]["hook_attributes"]["url"],
        "https://orbit.test/api/v1/integrations/github/webhook"
    );
    assert_eq!(
        manifest["manifest"]["setup_url"],
        format!(
            "https://orbit.test/settings/github?workspace={}&github=installed",
            fixture.workspace_id
        )
    );
    let repeated_register = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{workspace_path}/manifest"))
                .header(header::COOKIE, &fixture.session_cookie)
                .header(header::ORIGIN, "https://orbit.test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(repeated_register.status(), StatusCode::OK);
    let retry = response_json(repeated_register).await;
    assert_ne!(retry["action"], manifest["action"]);
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM github_app_registrations WHERE workspace_id = ?")
            .bind(fixture.workspace_id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(pending, 1);

    let old_project_manifest = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{path}/manifest"))
                .header(header::ORIGIN, "https://orbit.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(old_project_manifest.status(), StatusCode::NOT_FOUND);

    let unauthorized_workspace = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&workspace_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized_workspace.status(), StatusCode::UNAUTHORIZED);

    let workspace_settings = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&workspace_path)
                .header(header::COOKIE, &fixture.session_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(workspace_settings.status(), StatusCode::OK);
    let workspace_settings = response_json(workspace_settings).await;
    assert_eq!(workspace_settings["app_slug"], Value::Null);
    assert_eq!(workspace_settings["repositories"], json!([]));
    assert_eq!(workspace_settings["key_configured"], true);

    let unauthorized = app
        .clone()
        .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        app.clone()
            .oneshot(github_request("ping", json!({"zen":"Ready"}), true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let secret = encrypt_for_test(&key, "hook-secret");
    let private_key = encrypt_for_test(&key, "private-key");
    sqlx::query("INSERT INTO github_apps (workspace_id, app_id, slug, private_key_encrypted, webhook_secret_encrypted, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(fixture.workspace_id.to_string()).bind(77).bind("orbit-test")
        .bind(&private_key).bind(&secret).bind(TimestampMillis::now().as_millis())
        .execute(fixture.database.pool()).await.unwrap();
    assert!(!secret.windows(11).any(|part| part == b"hook-secret"));

    let install = json!({"action":"created","installation":{"id":1234,"app_id":77,"account":{"login":"acme"}},"repositories":[{"full_name":"acme/repo"}]});
    assert_eq!(
        app.clone()
            .oneshot(github_request("installation", install, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let settings = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .header(header::COOKIE, &fixture.session_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(settings.status(), StatusCode::OK);
    let settings = response_json(settings).await;
    assert_eq!(settings["repositories"][0]["repository"], "acme/repo");
    assert_eq!(settings["app_slug"], "orbit-test");
    assert!(settings.get("private_key").is_none());

    let workspace_settings = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&workspace_path)
                .header(header::COOKIE, &fixture.session_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(workspace_settings.status(), StatusCode::OK);
    let workspace_settings = response_json(workspace_settings).await;
    assert_eq!(
        workspace_settings["repositories"][0]["repository"],
        "acme/repo"
    );

    let uninstalled = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&path)
                .header(header::COOKIE, &fixture.session_cookie)
                .header(header::ORIGIN, "https://orbit.test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"installation_id":1234,"repository":"acme/other","label":"Orbit"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(uninstalled.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let linked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&path)
                .header(header::COOKIE, &fixture.session_cookie)
                .header(header::ORIGIN, "https://orbit.test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"installation_id":1234,"repository":"acme/repo","label":"Orbit"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(linked.status(), StatusCode::OK);

    let second_project = TaskRepository::new((*fixture.database).clone())
        .create_project(
            fixture.workspace_id,
            fixture.user_id,
            "Platform".to_owned(),
            "PLAT".to_owned(),
            "#7c3aed".to_owned(),
            "create-project",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let second_path = format!(
        "/api/v1/workspaces/{}/projects/{}/github",
        fixture.workspace_id, second_project.id
    );
    let duplicate_label = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&second_path)
                .header(header::COOKIE, &fixture.session_cookie)
                .header(header::ORIGIN, "https://orbit.test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"installation_id":1234,"repository":"acme/repo","label":"Orbit"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_label.status(), StatusCode::CONFLICT);
    let second_link = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&second_path)
                .header(header::COOKIE, &fixture.session_cookie)
                .header(header::ORIGIN, "https://orbit.test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"installation_id":1234,"repository":"acme/repo","label":"Platform"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_link.status(), StatusCode::OK);
    let connection_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE workspace_id = ? AND action = 'github.connection.updated'")
        .bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(connection_events, 2);

    let issue = json!({"action":"labeled","installation":{"id":1234,"app_id":77},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"GitHub task","body":"Issue body","state":"open","labels":[{"name":"Orbit"}]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", issue, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let task_id: String = sqlx::query_scalar("SELECT task_id FROM github_issue_links WHERE workspace_id = ? AND repository = 'acme/repo' AND issue_number = 12")
        .bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    let service_account_id: String = sqlx::query_scalar(
        "SELECT id FROM service_accounts WHERE workspace_id = ? AND name = 'GitHub'",
    )
    .bind(fixture.workspace_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let created_task = TaskRepository::new((*fixture.database).clone())
        .get_task(
            fixture.workspace_id,
            task_id.parse().unwrap(),
            fixture.user_id,
        )
        .await
        .unwrap();
    assert_eq!(created_task.creator_id, None);
    assert_eq!(
        created_task.creator_service_account_id.unwrap().to_string(),
        service_account_id
    );
    assert_eq!(
        created_task.creator_service_account_name.as_deref(),
        Some("GitHub")
    );
    let (actor_id, metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_type = 'task' AND resource_id = ? AND action = 'task.created'",
    ).bind(&task_id).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(actor_id, None);
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["actor_service_account_id"], service_account_id);
    assert_eq!(metadata["actor_service_account_name"], "GitHub");
    let (label_actor, label_metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT audit_events.actor_id, audit_events.metadata_json FROM audit_events JOIN labels ON labels.id = audit_events.resource_id WHERE labels.workspace_id = ? AND labels.name = 'GitHub' AND audit_events.action = 'label.created'",
    ).bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(label_actor, None);
    let label_metadata: Value = serde_json::from_str(&label_metadata).unwrap();
    assert_eq!(
        label_metadata["actor_service_account_id"],
        service_account_id
    );
    let task_project: String = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = ?")
        .bind(&task_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(task_project, fixture.project_id.to_string());
    // Simulate records written before GitHub used a service account.
    sqlx::query("UPDATE tasks SET creator_service_account_id = NULL WHERE id = ?")
        .bind(&task_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE audit_events SET actor_id = ?, metadata_json = '{}' WHERE resource_type = 'task' AND resource_id = ? AND action = 'task.created'")
        .bind(fixture.user_id.to_string()).bind(&task_id)
        .execute(fixture.database.pool()).await.unwrap();
    sqlx::query("INSERT INTO audit_events (id, workspace_id, actor_id, action, outcome, resource_type, resource_id, request_id, metadata_json, occurred_at) VALUES (?, ?, ?, 'github.work_item.link.updated', 'success', 'task', ?, 'old-github-webhook', '{}', ?)")
        .bind(orbit_platform::Id::new_v7().to_string()).bind(fixture.workspace_id.to_string())
        .bind(fixture.user_id.to_string()).bind(&task_id).bind(TimestampMillis::now().as_millis())
        .execute(fixture.database.pool()).await.unwrap();
    sqlx::query("INSERT INTO audit_events (id, workspace_id, actor_id, action, outcome, resource_type, resource_id, request_id, metadata_json, occurred_at) VALUES (?, ?, ?, 'task.restored', 'success', 'task', ?, 'user-restore', '{}', ?)")
        .bind(orbit_platform::Id::new_v7().to_string()).bind(fixture.workspace_id.to_string())
        .bind(fixture.user_id.to_string()).bind(&task_id).bind(TimestampMillis::now().as_millis())
        .execute(fixture.database.pool()).await.unwrap();
    let repository = TaskRepository::new((*fixture.database).clone());
    repository
        .reconcile_github_attribution(TimestampMillis::now())
        .await
        .unwrap();
    repository
        .reconcile_github_attribution(TimestampMillis::now())
        .await
        .unwrap();
    let repaired_creator: String =
        sqlx::query_scalar("SELECT creator_service_account_id FROM tasks WHERE id = ?")
            .bind(&task_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(repaired_creator, service_account_id);
    let repaired_actor: Option<String> = sqlx::query_scalar(
        "SELECT actor_id FROM audit_events WHERE resource_id = ? AND action = 'task.created'",
    )
    .bind(&task_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(repaired_actor, None);
    let restored_actor: String = sqlx::query_scalar(
        "SELECT actor_id FROM audit_events WHERE resource_id = ? AND action = 'task.restored'",
    )
    .bind(&task_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(restored_actor, fixture.user_id.to_string());
    let before_pause: i64 =
        sqlx::query_scalar("SELECT sequence FROM realtime_sequences WHERE workspace_id = ?")
            .bind(fixture.workspace_id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();

    let unlabeled = json!({"action":"unlabeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"Ignored while paused","body":"Changed body","state":"open","labels":[]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", unlabeled, true))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let paused: i64 =
        sqlx::query_scalar("SELECT sync_paused FROM github_issue_links WHERE task_id = ?")
            .bind(&task_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id = ?")
        .bind(&task_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(paused, 1);
    assert_eq!(title, "GitHub task");
    let after_pause: i64 =
        sqlx::query_scalar("SELECT sequence FROM realtime_sequences WHERE workspace_id = ?")
            .bind(fixture.workspace_id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(after_pause, before_pause + 1);
    let event =
        orbit_server::realtime::replay(&fixture.database, fixture.workspace_id, Some(before_pause))
            .await
            .unwrap();
    assert_eq!(event.kind, "workspace.changed");
    assert_eq!(event.sequence, after_pause.to_string());

    let relabeled = json!({"action":"labeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"Updated after relabeling","body":"Changed body","state":"open","labels":[{"name":"Orbit"}]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", relabeled, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let link: (String, i64) = sqlx::query_as("SELECT task_id, sync_paused FROM github_issue_links WHERE workspace_id = ? AND repository = 'acme/repo' AND issue_number = 12")
        .bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    let title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id = ?")
        .bind(&task_id)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(link, (task_id.clone(), 0));
    assert_eq!(title, "Updated after relabeling");
    let restored_creator: String =
        sqlx::query_scalar("SELECT creator_service_account_id FROM tasks WHERE id = ?")
            .bind(&task_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(restored_creator, service_account_id);
    let legacy_activity: Vec<(Option<String>, String)> = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_type = 'task' AND resource_id = ? AND action IN ('task.created', 'github.work_item.link.updated') ORDER BY action",
    ).bind(&task_id).fetch_all(fixture.database.pool()).await.unwrap();
    assert_eq!(legacy_activity.len(), 2);
    for (actor, metadata) in legacy_activity {
        assert_eq!(actor, None);
        let metadata: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["actor_service_account_id"], service_account_id);
    }
    let (actor_id, metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_type = 'task' AND resource_id = ? AND action = 'task.updated' ORDER BY occurred_at DESC LIMIT 1",
    ).bind(&task_id).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(actor_id, None);
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["actor_service_account_id"], service_account_id);
    let (description, source_url): (String, Option<String>) =
        sqlx::query_as("SELECT description, source_url FROM tasks WHERE id = ?")
            .bind(&task_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(description, "Changed body");
    assert_eq!(
        source_url.as_deref(),
        Some("https://github.com/acme/repo/issues/12")
    );
    let same_issue = json!({"installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"Updated after relabeling","body":"Changed body","state":"open","labels":[]}});
    let mut same_issue_unlabeled = same_issue.clone();
    same_issue_unlabeled["action"] = json!("unlabeled");
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", same_issue_unlabeled, true))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let mut same_issue_relabeled = same_issue;
    same_issue_relabeled["action"] = json!("labeled");
    same_issue_relabeled["issue"]["labels"] = json!([{"name":"Orbit"}]);
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", same_issue_relabeled, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let (link_actor, link_metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_type = 'task' AND resource_id = ? AND action = 'github.work_item.link.updated' ORDER BY occurred_at DESC LIMIT 1",
    ).bind(&task_id).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(link_actor, None);
    let link_metadata: Value = serde_json::from_str(&link_metadata).unwrap();
    assert_eq!(
        link_metadata["actor_service_account_id"],
        service_account_id
    );

    let github_task_id: orbit_platform::Id = task_id.parse().unwrap();
    let current = repository
        .get_task(fixture.workspace_id, github_task_id, fixture.user_id)
        .await
        .unwrap();
    repository
        .delete_task(
            fixture.workspace_id,
            github_task_id,
            fixture.user_id,
            current.version,
            "user-delete",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let unlabeled_after_delete = json!({"action":"unlabeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"While deleted","body":"Current GitHub body","state":"closed","labels":[]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", unlabeled_after_delete, true))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let paused_while_deleted: i64 =
        sqlx::query_scalar("SELECT sync_paused FROM github_issue_links WHERE task_id = ?")
            .bind(&task_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(paused_while_deleted, 1);
    let relabeled_after_delete = json!({"action":"labeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"Restored from GitHub","body":"Current GitHub body","state":"closed","labels":[{"name":"Orbit"}]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", relabeled_after_delete, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let restored: (String, Option<i64>, i64, String, String) = sqlx::query_as(
        "SELECT github_issue_links.task_id, tasks.deleted_at, github_issue_links.sync_paused, tasks.title, task_statuses.category FROM github_issue_links JOIN tasks ON tasks.id = github_issue_links.task_id JOIN task_statuses ON task_statuses.id = tasks.status_id WHERE github_issue_links.repository = 'acme/repo' AND github_issue_links.issue_number = 12",
    ).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(
        restored,
        (
            task_id.clone(),
            None,
            0,
            "Restored from GitHub".to_owned(),
            "completed".to_owned()
        )
    );
    let issue_task_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE source_url = 'https://github.com/acme/repo/issues/12'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(issue_task_count, 1);
    let restore_actor: (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_id = ? AND action = 'task.restored' ORDER BY occurred_at DESC LIMIT 1",
    ).bind(&task_id).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(restore_actor.0, None);
    assert_eq!(
        serde_json::from_str::<Value>(&restore_actor.1).unwrap()["actor_service_account_name"],
        "GitHub"
    );
    let unlabeled_after_restore = json!({"action":"unlabeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"Restored from GitHub","body":"Current GitHub body","state":"closed","labels":[]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", unlabeled_after_restore, true))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let paused_after_restore: i64 =
        sqlx::query_scalar("SELECT sync_paused FROM github_issue_links WHERE task_id = ?")
            .bind(&task_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(paused_after_restore, 1);
    let relabeled_again = json!({"action":"labeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":12,"title":"Restored from GitHub","body":"Current GitHub body","state":"closed","labels":[{"name":"Orbit"}]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", relabeled_again, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let resumed_link: (String, i64) = sqlx::query_as("SELECT task_id, sync_paused FROM github_issue_links WHERE repository = 'acme/repo' AND issue_number = 12")
        .fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(resumed_link, (task_id.clone(), 0));

    let other_issue = json!({"action":"labeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":13,"title":"Platform task","body":"","state":"open","labels":[{"name":"Platform"}]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", other_issue.clone(), false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(github_request("issues", other_issue, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let second_task: String = sqlx::query_scalar("SELECT task_id FROM github_issue_links WHERE workspace_id = ? AND repository = 'acme/repo' AND issue_number = 13")
        .bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    let second_task_project: String =
        sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = ?")
            .bind(&second_task)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(second_task_project, second_project.id.to_string());

    let ambiguous = json!({"action":"labeled","installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "issue":{"number":14,"title":"Ambiguous","body":"","state":"open","labels":[{"name":"Orbit"},{"name":"Platform"}]}});
    let response = app
        .clone()
        .oneshot(github_request("issues", ambiguous, true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(response).await["code"],
        "github_labels_ambiguous"
    );

    let pull = |action: &str, title: &str, labels: Vec<&str>, state: &str, merged: bool| {
        json!({
            "action":action,"number":8,"installation":{"id":1234},"repository":{"full_name":"acme/repo"},
            "pull_request":{"number":8,"title":title,"body":"Pull request body","state":state,
                "merged":merged,"labels":labels.into_iter().map(|name| json!({"name":name})).collect::<Vec<_>>()}
        })
    };
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("labeled", "Fix", vec!["Orbit"], "open", false),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let pr_link: (String, String, String) = sqlx::query_as(
        "SELECT task_id, kind, pull_state FROM github_issue_links WHERE repository = 'acme/repo' AND issue_number = 8",
    ).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(pr_link.1, "pull_request");
    assert_eq!(pr_link.2, "open");
    assert_ne!(pr_link.0, task_id);
    let pr_service_account_id: String =
        sqlx::query_scalar("SELECT creator_service_account_id FROM tasks WHERE id = ?")
            .bind(&pr_link.0)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(pr_service_account_id, service_account_id);
    let (pr_actor, pr_metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_type = 'task' AND resource_id = ? AND action = 'task.created'",
    ).bind(&pr_link.0).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(pr_actor, None);
    let pr_metadata: Value = serde_json::from_str(&pr_metadata).unwrap();
    assert_eq!(pr_metadata["actor_service_account_id"], service_account_id);
    let account_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM service_accounts WHERE workspace_id = ? AND lower(name) = 'github'",
    )
    .bind(fixture.workspace_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(account_count, 1);
    let (pr_title, pr_body, pr_url, pr_project): (String, String, Option<String>, String) =
        sqlx::query_as("SELECT title, description, source_url, project_id FROM tasks WHERE id = ?")
            .bind(&pr_link.0)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(
        (
            pr_title.as_str(),
            pr_body.as_str(),
            pr_url.as_deref(),
            pr_project.as_str()
        ),
        (
            "Fix",
            "Pull request body",
            Some("https://github.com/acme/repo/pull/8"),
            fixture.project_id.to_string().as_str()
        )
    );
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("edited", "Fix again", vec!["Orbit"], "open", false),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let updated_title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id = ?")
        .bind(&pr_link.0)
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(updated_title, "Fix again");
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("unlabeled", "Ignored", vec![], "open", false),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let paused: i64 =
        sqlx::query_scalar("SELECT sync_paused FROM github_issue_links WHERE task_id = ?")
            .bind(&pr_link.0)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(paused, 1);
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("labeled", "Resumed", vec!["Orbit"], "open", false),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let resumed: (String, i64) = sqlx::query_as("SELECT task_id, sync_paused FROM github_issue_links WHERE repository = 'acme/repo' AND issue_number = 8").fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(resumed, (pr_link.0.clone(), 0));
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("closed", "Resumed", vec!["Orbit"], "closed", true),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let merged_category: String = sqlx::query_scalar("SELECT category FROM task_statuses JOIN tasks ON tasks.status_id = task_statuses.id WHERE tasks.id = ?").bind(&pr_link.0).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(merged_category, "completed");
    let pr_task_id: orbit_platform::Id = pr_link.0.parse().unwrap();
    let current = repository
        .get_task(fixture.workspace_id, pr_task_id, fixture.user_id)
        .await
        .unwrap();
    repository
        .delete_task(
            fixture.workspace_id,
            pr_task_id,
            fixture.user_id,
            current.version,
            "user-delete-pr",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("unlabeled", "Ignored while deleted", vec![], "closed", true),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let paused: i64 =
        sqlx::query_scalar("SELECT sync_paused FROM github_issue_links WHERE task_id = ?")
            .bind(&pr_link.0)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(paused, 1);
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("labeled", "Restored PR", vec!["Orbit"], "closed", true),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let restored_pr: (String, Option<i64>, i64, String) = sqlx::query_as(
        "SELECT github_issue_links.task_id, tasks.deleted_at, github_issue_links.sync_paused, task_statuses.category FROM github_issue_links JOIN tasks ON tasks.id = github_issue_links.task_id JOIN task_statuses ON task_statuses.id = tasks.status_id WHERE github_issue_links.repository = 'acme/repo' AND github_issue_links.issue_number = 8",
    ).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(
        restored_pr,
        (pr_link.0.clone(), None, 0, "completed".to_owned())
    );
    let pr_task_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE source_url = 'https://github.com/acme/repo/pull/8'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(pr_task_count, 1);
    assert_eq!(
        app.clone()
            .oneshot(github_request(
                "pull_request",
                pull("labeled", "No project", vec![], "open", false),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let existing_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM github_issue_links WHERE repository = 'acme/repo' AND issue_number = 8").fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(existing_count, 1);

    let closed = json!({"action":"labeled","number":9,"installation":{"id":1234},"repository":{"full_name":"acme/repo"},
        "pull_request":{"number":9,"title":"Unmerged pull request","body":"","state":"open","merged":false,"labels":[{"name":"Orbit"}]}});
    assert_eq!(
        app.clone()
            .oneshot(github_request("pull_request", closed.clone(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let unmerged_id: String = sqlx::query_scalar("SELECT task_id FROM github_issue_links WHERE repository = 'acme/repo' AND issue_number = 9").fetch_one(fixture.database.pool()).await.unwrap();
    let mut closed = closed;
    closed["action"] = json!("closed");
    closed["pull_request"]["state"] = json!("closed");
    assert_eq!(
        app.clone()
            .oneshot(github_request("pull_request", closed.clone(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let category: String = sqlx::query_scalar("SELECT category FROM task_statuses JOIN tasks ON tasks.status_id = task_statuses.id WHERE tasks.id = ?")
        .bind(&unmerged_id).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(category, "cancelled");
    closed["action"] = json!("reopened");
    closed["pull_request"]["state"] = json!("open");
    assert_eq!(
        app.clone()
            .oneshot(github_request("pull_request", closed, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let category: String = sqlx::query_scalar("SELECT category FROM task_statuses JOIN tasks ON tasks.status_id = task_statuses.id WHERE tasks.id = ?")
        .bind(&unmerged_id).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(category, "unstarted");
    let removed = json!({"action":"removed","installation":{"id":1234,"account":{"login":"acme"}},"repositories_added":[],"repositories_removed":[{"full_name":"acme/repo"}]});
    assert_eq!(
        app.clone()
            .oneshot(github_request("installation_repositories", removed, true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM github_project_connections WHERE workspace_id = ?",
    )
    .bind(fixture.workspace_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(remaining, 0);
    let installation_event: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE workspace_id = ? AND action = 'github.installation.updated'")
        .bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    assert!(installation_event >= 2);
}

#[tokio::test]
async fn development_github_registration_uses_the_browser_https_origin() {
    let fixture = fixture().await;
    let state = IntegrationState::new(
        Arc::new(fixture.tokens.clone()),
        Arc::new(TaskRepository::new((*fixture.database).clone())),
    )
    .with_github_settings(
        Arc::new(IdentityRepository::new((*fixture.database).clone())),
        CookieMode::secure(),
        "http://127.0.0.1:18888".to_owned(),
        true,
        Some([7u8; 32]),
    );
    let app = integration_router(state).layer(HttpPlatformLayer::new(
        OriginPolicy::new("http://127.0.0.1:18888").allow_any_http_origin(),
    ));
    let path = format!(
        "/api/v1/workspaces/{}/github/manifest",
        fixture.workspace_id
    );
    let request = |origin| {
        Request::builder()
            .method("POST")
            .uri(&path)
            .header(header::COOKIE, &fixture.session_cookie)
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap()
    };
    let insecure = app
        .clone()
        .oneshot(request("http://127.0.0.1:18888"))
        .await
        .unwrap();
    assert_eq!(insecure.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let registered = app
        .oneshot(request("https://orbit-tunnel.example"))
        .await
        .unwrap();
    assert_eq!(registered.status(), StatusCode::OK);
    let manifest = response_json(registered).await["manifest"].clone();
    assert_eq!(manifest["url"], "https://orbit-tunnel.example");
    assert_eq!(
        manifest["hook_attributes"]["url"],
        "https://orbit-tunnel.example/api/v1/integrations/github/webhook"
    );
    assert_eq!(
        manifest["redirect_url"],
        "https://orbit-tunnel.example/api/v1/integrations/github/manifest/callback"
    );
    let saved_origin: String = sqlx::query_scalar(
        "SELECT public_origin FROM github_app_registrations WHERE workspace_id = ?",
    )
    .bind(fixture.workspace_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(saved_origin, "https://orbit-tunnel.example");
}

fn encrypt_for_test(key: &[u8; 32], value: &str) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let mut encrypted = nonce.to_vec();
    encrypted.extend(cipher.encrypt(&nonce, value.as_bytes()).unwrap());
    encrypted
}

fn github_request(event: &str, body: Value, valid_signature: bool) -> Request<Body> {
    let body = body.to_string();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(b"hook-secret").unwrap();
    mac.update(body.as_bytes());
    let digest = mac.finalize().into_bytes();
    let signature = if valid_signature {
        format!(
            "sha256={}",
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    } else {
        "sha256=invalid".to_owned()
    };
    Request::builder()
        .method("POST")
        .uri("/api/v1/integrations/github/webhook")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", event)
        .header("x-hub-signature-256", signature)
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn discord_events_create_one_labeled_task_and_retries_are_idempotent() {
    let fixture = fixture().await;
    let issued = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Discord".to_owned(),
            vec![fixture.project_id],
            true,
            true,
            true,
            None,
            "create-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let rotated = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "discord".to_owned(),
            vec![fixture.project_id],
            true,
            true,
            true,
            None,
            "rotate-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert_eq!(
        issued.api_token.service_account_id,
        rotated.api_token.service_account_id
    );
    sqlx::query("UPDATE users SET suspended_at = ? WHERE id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(fixture.user_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let body = json!({
        "event_id": "discord-message-123",
        "message": "A very long Discord message title that needs truncation\nMore details from Discord.",
        "message_url": "https://discord.com/channels/1/2/3"
    });

    let created = fixture
        .app
        .clone()
        .oneshot(request(Some(&issued.token), body.clone()))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = response_json(created).await;
    assert_eq!(created["duplicate"], false);
    assert_eq!(
        created["task"]["creator_service_account_id"],
        issued.api_token.service_account_id.unwrap().to_string()
    );
    assert_eq!(created["task"]["creator_service_account_name"], "Discord");
    assert_eq!(created["task"]["creator_id"], Value::Null);
    assert_eq!(
        created["task"]["title"],
        "A very long Discord message title tha..."
    );
    assert_eq!(
        created["task"]["project_id"],
        fixture.project_id.to_string()
    );
    assert_eq!(
        created["task"]["description"],
        "A very long Discord message title that needs truncation\nMore details from Discord."
    );
    assert_eq!(
        created["task"]["source_url"],
        "https://discord.com/channels/1/2/3"
    );
    let task_id = created["task"]["id"].as_str().unwrap().to_owned();

    let repeated = fixture
        .app
        .clone()
        .oneshot(request(Some(&issued.token), body.clone()))
        .await
        .unwrap();
    assert_eq!(repeated.status(), StatusCode::OK);
    let repeated = response_json(repeated).await;
    assert_eq!(repeated["duplicate"], true);
    assert_eq!(repeated["task"]["id"], task_id);
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM tasks WHERE id = ?), (SELECT COUNT(*) FROM integration_events WHERE external_event_id = 'discord-message-123'), (SELECT COUNT(*) FROM labels WHERE workspace_id = ? AND lower(name) = 'discord')")
        .bind(&task_id).bind(fixture.workspace_id.to_string()).fetch_one(fixture.database.pool()).await.unwrap();
    assert_eq!(counts, (1, 1, 1));

    let conflict = fixture.app.clone().oneshot(request(Some(&issued.token), json!({ "event_id": "discord-message-123", "message": "Changed", "message_url": "https://discord.com/channels/1/2/3" }))).await.unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(conflict).await["code"],
        "integration_event_conflict"
    );

    sqlx::query("UPDATE tasks SET deleted_at = ?, version = version + 1 WHERE id = ?")
        .bind(TimestampMillis::now().as_millis())
        .bind(&task_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();

    let recreated = fixture
        .app
        .oneshot(request(Some(&issued.token), body))
        .await
        .unwrap();
    assert_eq!(recreated.status(), StatusCode::CREATED);
    let recreated = response_json(recreated).await;
    assert_eq!(recreated["duplicate"], false);
    assert_ne!(recreated["task"]["id"], task_id);
    assert_eq!(
        recreated["task"]["creator_service_account_id"],
        issued.api_token.service_account_id.unwrap().to_string()
    );
    assert_eq!(recreated["task"]["creator_service_account_name"], "Discord");
    assert_eq!(recreated["task"]["creator_id"], Value::Null);

    let replacement_id = recreated["task"]["id"].as_str().unwrap();
    let mapped_task_id: String = sqlx::query_scalar(
        "SELECT task_id FROM integration_events WHERE workspace_id = ? AND provider = 'discord' AND external_event_id = 'discord-message-123'",
    )
    .bind(fixture.workspace_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(mapped_task_id, replacement_id);

    let (actor_id, metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_type = 'task' AND resource_id = ? AND action = 'task.created'",
    )
    .bind(replacement_id)
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(actor_id, None);
    let metadata: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(
        metadata["actor_service_account_id"],
        issued.api_token.service_account_id.unwrap().to_string()
    );
    assert_eq!(metadata["actor_service_account_name"], "Discord");

    let (label_actor_id, label_metadata): (Option<String>, String) = sqlx::query_as(
        "SELECT actor_id, metadata_json FROM audit_events WHERE resource_type = 'label' AND action = 'label.created'",
    )
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(label_actor_id, None);
    let label_metadata: Value = serde_json::from_str(&label_metadata).unwrap();
    assert_eq!(label_metadata["actor_service_account_name"], "Discord");
}

#[tokio::test]
async fn discord_events_require_an_active_write_token_and_valid_url() {
    let fixture = fixture().await;
    let missing = fixture
        .app
        .clone()
        .oneshot(request(None, valid_body("missing")))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response_json(missing).await["code"], "api_token_required");

    let now = TimestampMillis::now();
    let expired = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Expired".to_owned(),
            vec![fixture.project_id],
            false,
            true,
            false,
            Some(now),
            "expired-token",
            now,
        )
        .await
        .unwrap();
    let expired_response = fixture
        .app
        .clone()
        .oneshot(request(Some(&expired.token), valid_body("expired")))
        .await
        .unwrap();
    assert_eq!(expired_response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response_json(expired_response).await["code"],
        "invalid_api_token"
    );

    let read_only = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Read".to_owned(),
            vec![fixture.project_id],
            true,
            false,
            false,
            None,
            "read-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let forbidden = fixture
        .app
        .clone()
        .oneshot(request(Some(&read_only.token), valid_body("read")))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(forbidden).await["code"],
        "api_token_scope_forbidden"
    );

    let revoked = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Revoked".to_owned(),
            vec![fixture.project_id],
            false,
            true,
            false,
            None,
            "revoked-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    fixture
        .tokens
        .revoke(
            fixture.workspace_id,
            revoked.api_token.id,
            fixture.user_id,
            "revoke-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let unauthorized = fixture
        .app
        .clone()
        .oneshot(request(Some(&revoked.token), valid_body("revoked")))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response_json(unauthorized).await["code"],
        "invalid_api_token"
    );

    let write = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Write".to_owned(),
            vec![fixture.project_id],
            false,
            true,
            false,
            None,
            "write-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let invalid = fixture.app.oneshot(request(Some(&write.token), json!({ "event_id": "bad-url", "message": "Message", "message_url": "https://example.com/message" }))).await.unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response_json(invalid).await["code"], "validation_failed");
}

#[tokio::test]
async fn discord_events_select_one_project_allowed_by_a_multi_project_token() {
    let fixture = fixture().await;
    let second_project = TaskRepository::new((*fixture.database).clone())
        .create_project(
            fixture.workspace_id,
            fixture.user_id,
            "Platform".to_owned(),
            "PLAT".to_owned(),
            "#7c3aed".to_owned(),
            "create-project",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let issued = fixture
        .tokens
        .create(
            fixture.workspace_id,
            fixture.user_id,
            "Multi-project".to_owned(),
            vec![fixture.project_id, second_project.id],
            false,
            true,
            false,
            None,
            "create-token",
            TimestampMillis::now(),
        )
        .await
        .unwrap();

    let selected = fixture
        .app
        .clone()
        .oneshot(request(
            Some(&issued.token),
            json!({
                "event_id": "selected-project",
                "message": "Discord message",
                "message_url": "https://discord.com/channels/1/2/3",
                "project_id": second_project.id,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::CREATED);
    assert_eq!(
        response_json(selected).await["task"]["project_id"],
        second_project.id.to_string()
    );

    let missing = fixture
        .app
        .oneshot(request(Some(&issued.token), valid_body("missing-project")))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

struct Fixture {
    app: axum::Router,
    database: TestDatabase,
    tokens: ApiTokenRepository,
    user_id: orbit_platform::Id,
    workspace_id: orbit_platform::Id,
    project_id: orbit_platform::Id,
    session_cookie: String,
}

async fn fixture() -> Fixture {
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
    let tokens = ApiTokenRepository::new((*database).clone());
    let tasks = Arc::new(TaskRepository::new((*database).clone()));
    let app = integration_router(IntegrationState::new(Arc::new(tokens.clone()), tasks)).layer(
        HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")),
    );
    Fixture {
        app,
        database,
        tokens,
        user_id: setup.user_id,
        workspace_id: setup.workspace_id,
        project_id: setup.project_id,
        session_cookie: format!("__Host-orbit_session={}", setup.session.token),
    }
}

fn valid_body(event_id: &str) -> Value {
    json!({ "event_id": event_id, "message": "Discord message", "message_url": "https://discord.com/channels/1/2/3" })
}

fn request(token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/integrations/discord/events")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
