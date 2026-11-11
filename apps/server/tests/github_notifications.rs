//! GitHub pull request notices in the Orbit inbox (plan 4 of PR review).
#![recursion_limit = "256"]

mod fake_github;

use fake_github::{GithubFixture, fixture};
use orbit_platform::{Id, Row};
use serde_json::{Value, json};

/// The workspace app, the owner connected as GitHub user 1 (`octo`) and a member as GitHub user 2 (`mia`).
async fn setup() -> (GithubFixture, Id) {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture.connect(fixture.user_id, (1, "octo"), "ghu_owner", None, None).await;
    let (member, _) = fixture.add_member("mia@example.com").await;
    fixture.connect(member, (2, "mia"), "ghu_mia", None, None).await;
    (fixture, member)
}

/// A pull request as GitHub sends it in a delivery (the fields the task sync reads too).
fn pull(author: (i64, &str)) -> Value {
    json!({
        "number": 12, "title": "Fix the login", "body": "", "state": "open", "merged": false, "draft": false,
        "html_url": "https://github.com/acme/repo/pull/12", "labels": [],
        "user": { "id": author.0, "login": author.1 }
    })
}

fn delivery(action: &str, extra: Value) -> Value {
    let mut body = json!({
        "action": action,
        "repository": { "full_name": "acme/repo" },
        "installation": { "id": 1234 },
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

async fn notices(fixture: &GithubFixture) -> Vec<(String, String, String, Option<String>, String)> {
    sqlx::query("SELECT recipient_user_id, kind, github_actor_login, github_review_state, dedupe_key FROM notifications WHERE kind LIKE 'github_%' ORDER BY created_at, id")
        .fetch_all(fixture.database.pool())
        .await
        .unwrap()
        .into_iter()
        .map(|row| (row.get("recipient_user_id"), row.get("kind"), row.get("github_actor_login"), row.get("github_review_state"), row.get("dedupe_key")))
        .collect()
}

#[tokio::test]
async fn github_notice_columns_are_checked() {
    let (fixture, member) = setup().await;
    let insert = |kind: &'static str, state: Option<&'static str>| {
        sqlx::query("INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, dedupe_key, created_at, github_repository, github_pull_number, github_actor_login, github_review_state) VALUES ($1, $2, $3, $3, $4, $5, 1, 'acme/repo', 12, 'octo', $6)")
            .bind(Id::new_v7().to_string())
            .bind(fixture.workspace())
            .bind(member.to_string())
            .bind(kind)
            .bind(Id::new_v7().to_string())
            .bind(state)
            .execute(fixture.database.pool())
    };
    assert!(insert("github_review", Some("approved")).await.is_ok());
    assert!(insert("github_merged", None).await.is_ok());
    assert!(insert("github_review", None).await.is_err(), "a review notice needs its state");
    assert!(insert("task_assigned", None).await.is_err(), "only GitHub notices carry a repository");
}

#[tokio::test]
async fn review_request_notifies_connected_reviewer() {
    let (fixture, member) = setup().await;
    let body = delivery("review_requested", json!({ "pull_request": pull((1, "octo")), "requested_reviewer": { "id": 2, "login": "mia" }, "sender": { "id": 1, "login": "octo" } }));
    assert!(fixture.webhook("pull_request", "d-1", body).await.status().is_success());
    let rows = notices(&fixture).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, member.to_string());
    assert_eq!(rows[0].1, "github_review_requested");
    assert_eq!(rows[0].2, "octo");
    let actor: String = sqlx::query_scalar("SELECT actor_user_id FROM notifications WHERE kind = 'github_review_requested'")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(actor, fixture.user_id.to_string(), "the actor is the Orbit user of the GitHub account");
}

#[tokio::test]
async fn review_request_for_unconnected_user_is_ignored() {
    let (fixture, _) = setup().await;
    let body = delivery("review_requested", json!({ "pull_request": pull((1, "octo")), "requested_reviewer": { "id": 99, "login": "stranger" }, "sender": { "id": 1, "login": "octo" } }));
    assert!(fixture.webhook("pull_request", "d-1", body).await.status().is_success());
    assert!(notices(&fixture).await.is_empty());
}

#[tokio::test]
async fn team_review_request_is_ignored() {
    let (fixture, _) = setup().await;
    let body = delivery("review_requested", json!({ "pull_request": pull((1, "octo")), "requested_team": { "id": 5, "slug": "core" }, "sender": { "id": 1, "login": "octo" } }));
    assert!(fixture.webhook("pull_request", "d-1", body).await.status().is_success());
    assert!(notices(&fixture).await.is_empty());
}

fn review(id: i64, state: &str, reviewer: (i64, &str)) -> Value {
    json!({ "id": id, "state": state, "body": "", "user": { "id": reviewer.0, "login": reviewer.1 } })
}

#[tokio::test]
async fn review_and_its_comments_make_one_notice() {
    let (fixture, member) = setup().await;
    let submitted = delivery("submitted", json!({ "pull_request": pull((2, "mia")), "review": review(500, "approved", (1, "octo")), "sender": { "id": 1, "login": "octo" } }));
    assert!(fixture.webhook("pull_request_review", "d-1", submitted).await.status().is_success());
    for (index, delivery_id) in ["d-2", "d-3"].iter().enumerate() {
        let comment = delivery("created", json!({ "pull_request": pull((2, "mia")), "comment": { "id": 900 + index, "pull_request_review_id": 500, "user": { "id": 1, "login": "octo" } }, "sender": { "id": 1, "login": "octo" } }));
        assert!(fixture.webhook("pull_request_review_comment", delivery_id, comment).await.status().is_success());
    }
    let rows = notices(&fixture).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, member.to_string());
    assert_eq!(rows[0].1, "github_review");
    assert_eq!(rows[0].3.as_deref(), Some("approved"));
}

#[tokio::test]
async fn self_review_is_not_notified() {
    let (fixture, _) = setup().await;
    let submitted = delivery("submitted", json!({ "pull_request": pull((2, "mia")), "review": review(501, "commented", (2, "mia")), "sender": { "id": 2, "login": "mia" } }));
    assert!(fixture.webhook("pull_request_review", "d-1", submitted).await.status().is_success());
    assert!(notices(&fixture).await.is_empty());
}

#[tokio::test]
async fn duplicate_delivery_is_ignored() {
    let (fixture, _) = setup().await;
    let submitted = delivery("submitted", json!({ "pull_request": pull((2, "mia")), "review": review(502, "changes_requested", (1, "octo")), "sender": { "id": 1, "login": "octo" } }));
    for _ in 0..2 {
        assert!(fixture.webhook("pull_request_review", "d-1", submitted.clone()).await.status().is_success());
    }
    let rows = notices(&fixture).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].3.as_deref(), Some("changes_requested"));
}

#[tokio::test]
async fn recipient_without_project_access_is_skipped() {
    let (fixture, _) = setup().await;
    sqlx::query("UPDATE projects SET private = 1 WHERE id = $1")
        .bind(fixture.project_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let body = delivery("review_requested", json!({ "pull_request": pull((1, "octo")), "requested_reviewer": { "id": 2, "login": "mia" }, "sender": { "id": 1, "login": "octo" } }));
    assert!(fixture.webhook("pull_request", "d-1", body).await.status().is_success());
    assert!(notices(&fixture).await.is_empty());
}

#[tokio::test]
async fn merged_notifies_author() {
    let (fixture, member) = setup().await;
    let mut merged = pull((2, "mia"));
    merged["merged"] = json!(true);
    merged["state"] = json!("closed");
    merged["merged_by"] = json!({ "id": 1, "login": "octo" });
    let body = delivery("closed", json!({ "pull_request": merged, "sender": { "id": 1, "login": "octo" } }));
    assert!(fixture.webhook("pull_request", "d-1", body).await.status().is_success());
    let rows = notices(&fixture).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, member.to_string());
    assert_eq!(rows[0].1, "github_merged");
}

#[tokio::test]
async fn closed_without_merge_is_not_a_notice() {
    let (fixture, _) = setup().await;
    let body = delivery("closed", json!({ "pull_request": pull((2, "mia")), "sender": { "id": 1, "login": "octo" } }));
    assert!(fixture.webhook("pull_request", "d-1", body).await.status().is_success());
    assert!(notices(&fixture).await.is_empty());
}

#[tokio::test]
async fn a_push_tells_open_viewers_which_pull_request_changed() {
    let (fixture, _) = setup().await;
    let body = delivery("synchronize", json!({ "pull_request": pull((2, "mia")), "sender": { "id": 2, "login": "mia" } }));
    assert!(fixture.webhook("pull_request", "d-1", body).await.status().is_success());
    let check = delivery("completed", json!({ "check_suite": { "pull_requests": [{ "number": 12 }, { "number": 13 }] } }));
    assert!(fixture.webhook("check_suite", "d-2", check).await.status().is_success());
    let events = orbit_server::realtime::replay(&fixture.database, fixture.workspace_id, Some(0)).await.unwrap();
    assert_eq!(events.github_pulls, vec!["acme/repo#12".to_owned(), "acme/repo#13".to_owned()]);
}

#[tokio::test]
async fn live_refresh_rows_stay_out_of_the_audit_log_and_check_runs_make_none() {
    let (fixture, _) = setup().await;
    let run = delivery("completed", json!({ "check_run": { "pull_requests": [{ "number": 12 }] } }));
    assert!(fixture.webhook("check_run", "d-1", run).await.status().is_success());
    let events = orbit_server::realtime::replay(&fixture.database, fixture.workspace_id, Some(0)).await.unwrap();
    assert!(events.github_pulls.is_empty(), "a check run makes no live event (its check suite does)");
    let suite = delivery("completed", json!({ "check_suite": { "pull_requests": [{ "number": 12 }] } }));
    assert!(fixture.webhook("check_suite", "d-2", suite).await.status().is_success());
    let (listed, _) = orbit_server::audit::list(&fixture.database, fixture.workspace_id, None, 100).await.unwrap();
    assert!(listed.iter().all(|event| event.resource_type != "github_pull"), "the audit log hides live refresh rows");
    let (global, _) = orbit_server::audit::list_global(&fixture.database, Some(fixture.workspace_id), None, None, 100).await.unwrap();
    assert!(global.iter().all(|event| event.resource_type != "github_pull"));
}
