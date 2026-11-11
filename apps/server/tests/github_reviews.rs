//! Commenting, reviewing and merging with the member's own GitHub token (plan 3 of PR review).
#![recursion_limit = "256"]

mod fake_github;

use axum::http::StatusCode;
use fake_github::{GithubFixture, fixture, json_body};
use serde_json::{Value, json};

fn pull_path(fixture: &GithubFixture, rest: &str) -> String {
    format!("/api/v1/workspaces/{}/github/pulls/acme/repo/12{rest}", fixture.workspace())
}

async fn connected() -> GithubFixture {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_user", None, None)
        .await;
    fixture
}

/// `query PullBasics`: the pull request id, head commit and who wrote it.
fn basics(author: &str) -> Value {
    json!({ "viewer": { "login": "octo" }, "repository": { "pullRequest": {
        "id": "PR_1", "headRefOid": "h1", "author": { "login": author }
    } } })
}

fn pending(id: Option<&str>, count: i64) -> Value {
    let nodes = id.map_or_else(Vec::new, |id| vec![json!({ "id": id, "viewerDidAuthor": true, "comments": { "totalCount": count } })]);
    json!({ "repository": { "pullRequest": { "reviews": { "nodes": nodes } } } })
}

fn thread_node(id: &str, body: &str, state: &str) -> Value {
    json!({
        "id": id, "path": "src/a.rs", "line": 3, "startLine": null, "diffSide": "RIGHT", "startDiffSide": null,
        "isResolved": false, "isOutdated": false, "viewerCanResolve": true, "viewerCanReply": true,
        "comments": { "nodes": [{ "id": "C_1", "databaseId": 77, "body": body, "createdAt": "2026-11-10T10:00:00Z",
            "state": state, "viewerCanUpdate": true, "author": { "login": "octo", "avatarUrl": "" } }] }
    })
}

fn count(fixture: &GithubFixture, needle: &str) -> usize {
    fixture
        .github
        .graphql_requests()
        .iter()
        .filter(|(query, _)| query.contains(needle))
        .count()
}

fn comment_body(mode: &str) -> Value {
    json!({ "path": "src/a.rs", "side": "RIGHT", "line": 3, "body": "Nit", "mode": mode })
}

#[tokio::test]
async fn pending_comment_creates_review_once() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(None, 0));
    fixture.github.graphql("query PendingReview", pending(Some("R_1"), 1));
    fixture.github.graphql("query PendingReview", pending(Some("R_1"), 2));
    fixture.github.graphql("mutation AddReview", json!({ "addPullRequestReview": { "pullRequestReview": { "id": "R_1" } } }));
    fixture.github.graphql("mutation AddThread", json!({ "addPullRequestReviewThread": { "thread": thread_node("T_1", "Nit", "PENDING") } }));
    for _ in 0..2 {
        let response = fixture
            .send("POST", &pull_path(&fixture, "/comments"), &fixture.session_cookie, Some(comment_body("pending")))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let thread = json_body(response).await;
        assert_eq!(thread["comments"][0]["pending"], true);
    }
    assert_eq!(count(&fixture, "mutation AddReview"), 1);
    assert_eq!(count(&fixture, "mutation AddThread"), 2);
    let added = fixture.github.graphql_requests();
    let (_, variables) = added.iter().find(|(query, _)| query.contains("mutation AddReview")).unwrap();
    assert_eq!(variables["pullId"], "PR_1");
    assert_eq!(variables["commit"], "h1");
    fixture.github.graphql("query PullThreads", json!({ "repository": { "pullRequest": { "reviewThreads": {
        "nodes": [], "pageInfo": { "hasNextPage": false, "endCursor": null } } } } }));
    let threads = json_body(fixture.get(&pull_path(&fixture, "/threads")).await).await;
    assert_eq!(threads["pending_review"], json!({ "id": "R_1", "comment_count": 2 }));
}

#[tokio::test]
async fn pending_comment_reuses_review_started_on_github() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(Some("R_GH"), 3));
    fixture.github.graphql("mutation AddThread", json!({ "addPullRequestReviewThread": { "thread": thread_node("T_1", "Nit", "PENDING") } }));
    let response = fixture
        .send("POST", &pull_path(&fixture, "/comments"), &fixture.session_cookie, Some(comment_body("pending")))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(count(&fixture, "mutation AddReview"), 0);
    let requests = fixture.github.graphql_requests();
    let (_, variables) = requests.iter().find(|(query, _)| query.contains("mutation AddThread")).unwrap();
    assert_eq!(variables["reviewId"], "R_GH");
}

#[tokio::test]
async fn range_on_the_old_side_keeps_github_coordinates() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(Some("R_1"), 0));
    fixture.github.graphql("mutation AddThread", json!({ "addPullRequestReviewThread": { "thread": thread_node("T_1", "Nit", "PENDING") } }));
    let body = json!({ "path": "src/a.rs", "side": "LEFT", "line": 9, "start_side": "LEFT", "start_line": 7, "body": "Why gone?", "mode": "pending" });
    let response = fixture.send("POST", &pull_path(&fixture, "/comments"), &fixture.session_cookie, Some(body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let requests = fixture.github.graphql_requests();
    let (_, variables) = requests.iter().find(|(query, _)| query.contains("mutation AddThread")).unwrap();
    assert_eq!(variables["side"], "LEFT");
    assert_eq!(variables["line"], 9);
    assert_eq!(variables["startSide"], "LEFT");
    assert_eq!(variables["startLine"], 7);
}

#[tokio::test]
async fn single_comment_uses_rest_with_head_sha() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.respond("POST", "repos/acme/repo/pulls/12/comments", 201, json!({ "id": 77, "node_id": "C_1" }));
    fixture.github.graphql("query PullThreads", json!({ "repository": { "pullRequest": { "reviewThreads": {
        "nodes": [thread_node("T_9", "Now", "SUBMITTED")], "pageInfo": { "hasNextPage": false, "endCursor": null } } } } }));
    let body = json!({ "path": "src/a.rs", "side": "LEFT", "line": 4, "start_side": "LEFT", "start_line": 2, "body": "Now", "mode": "single" });
    let response = fixture.send("POST", &pull_path(&fixture, "/comments"), &fixture.session_cookie, Some(body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["id"], "T_9");
    let rest = fixture.with_requests(|requests| requests.iter().find(|request| request.0 == "POST" && request.1.ends_with("/comments")).unwrap().3.clone());
    assert_eq!(rest["commit_id"], "h1");
    assert_eq!(rest["side"], "LEFT");
    assert_eq!(rest["start_line"], 2);
    assert_eq!(rest["start_side"], "LEFT");
    assert_eq!(rest["line"], 4);
}

#[tokio::test]
async fn reply_goes_into_the_pending_review() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(Some("R_1"), 1));
    fixture.github.graphql("mutation AddReply", json!({ "addPullRequestReviewThreadReply": { "comment": {
        "id": "C_2", "databaseId": 78, "body": "Agreed", "createdAt": "2026-11-10T10:00:00Z", "state": "PENDING",
        "viewerCanUpdate": true, "author": { "login": "octo", "avatarUrl": "" } } } }));
    let response = fixture
        .send("POST", &pull_path(&fixture, "/threads/T_1/replies"), &fixture.session_cookie, Some(json!({ "body": "Agreed" })))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["pending"], true);
    let requests = fixture.github.graphql_requests();
    let (_, variables) = requests.iter().find(|(query, _)| query.contains("mutation AddReply")).unwrap();
    assert_eq!(variables["threadId"], "T_1");
    assert_eq!(variables["reviewId"], "R_1");
}

#[tokio::test]
async fn resolve_and_unresolve_a_thread() {
    let fixture = connected().await;
    let resolved = { let mut node = thread_node("T_1", "x", "SUBMITTED"); node["isResolved"] = json!(true); node };
    fixture.github.graphql("mutation ResolveThread", json!({ "resolveReviewThread": { "thread": resolved } }));
    fixture.github.graphql("mutation UnresolveThread", json!({ "unresolveReviewThread": { "thread": thread_node("T_1", "x", "SUBMITTED") } }));
    let path = pull_path(&fixture, "/threads/T_1/resolved");
    let response = fixture.send("PUT", &path, &fixture.session_cookie, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["is_resolved"], true);
    let response = fixture.send("DELETE", &path, &fixture.session_cookie, None).await;
    assert_eq!(json_body(response).await["is_resolved"], false);
}

#[tokio::test]
async fn edit_and_delete_own_comment() {
    let fixture = connected().await;
    fixture.github.graphql("mutation UpdateComment", json!({ "updatePullRequestReviewComment": { "pullRequestReviewComment": {
        "id": "C_1", "databaseId": 77, "body": "Better", "createdAt": "2026-11-10T10:00:00Z", "state": "PENDING",
        "viewerCanUpdate": true, "author": { "login": "octo", "avatarUrl": "" } } } }));
    fixture.github.graphql("mutation DeleteComment", json!({ "deletePullRequestReviewComment": { "pullRequestReview": { "id": "R_1" } } }));
    let path = pull_path(&fixture, "/comments/C_1");
    let response = fixture.send("PATCH", &path, &fixture.session_cookie, Some(json!({ "body": "Better" }))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["body"], "Better");
    assert_eq!(fixture.send("DELETE", &path, &fixture.session_cookie, None).await.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn submit_review_submits_pending() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(Some("R_1"), 2));
    fixture.github.graphql("mutation SubmitReview", json!({ "submitPullRequestReview": { "pullRequestReview": { "id": "R_1" } } }));
    let response = fixture
        .send("POST", &pull_path(&fixture, "/review"), &fixture.session_cookie, Some(json!({ "event": "APPROVE", "body": "Ship it" })))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let requests = fixture.github.graphql_requests();
    let (_, variables) = requests.iter().find(|(query, _)| query.contains("mutation SubmitReview")).unwrap();
    assert_eq!(variables, &json!({ "reviewId": "R_1", "event": "APPROVE", "body": "Ship it" }));
    assert_eq!(count(&fixture, "mutation AddReview"), 0);
}

#[tokio::test]
async fn review_without_pending_comments_is_one_new_review() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(None, 0));
    fixture.github.graphql("mutation AddReview", json!({ "addPullRequestReview": { "pullRequestReview": { "id": "R_2" } } }));
    let response = fixture
        .send("POST", &pull_path(&fixture, "/review"), &fixture.session_cookie, Some(json!({ "event": "REQUEST_CHANGES", "body": "Fix the test" })))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let requests = fixture.github.graphql_requests();
    let (_, variables) = requests.iter().find(|(query, _)| query.contains("mutation AddReview")).unwrap();
    assert_eq!(variables["event"], "REQUEST_CHANGES");
    assert_eq!(variables["body"], "Fix the test");
}

#[tokio::test]
async fn author_cannot_approve_own_pull() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("octo"));
    let response = fixture
        .send("POST", &pull_path(&fixture, "/review"), &fixture.session_cookie, Some(json!({ "event": "APPROVE", "body": "" })))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(response).await["code"], "cannot_review_own_pull");
    assert_eq!(count(&fixture, "mutation"), 0);
}

#[tokio::test]
async fn empty_comment_review_is_refused() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(None, 0));
    let response = fixture
        .send("POST", &pull_path(&fixture, "/review"), &fixture.session_cookie, Some(json!({ "event": "COMMENT", "body": "  " })))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(response).await["code"], "review_empty");
}

#[tokio::test]
async fn discard_pending_review() {
    let fixture = connected().await;
    fixture.github.graphql("query PullBasics", basics("hubot"));
    fixture.github.graphql("query PendingReview", pending(Some("R_1"), 2));
    fixture.github.graphql("mutation DeleteReview", json!({ "deletePullRequestReview": { "pullRequestReview": { "id": "R_1" } } }));
    assert_eq!(fixture.send("DELETE", &pull_path(&fixture, "/review"), &fixture.session_cookie, None).await.status(), StatusCode::NO_CONTENT);
    let fixture2 = connected().await;
    fixture2.github.graphql("query PullBasics", basics("hubot"));
    fixture2.github.graphql("query PendingReview", pending(None, 0));
    assert_eq!(fixture2.send("DELETE", &pull_path(&fixture2, "/review"), &fixture2.session_cookie, None).await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn merge_sends_expected_sha_and_method() {
    let fixture = connected().await;
    fixture.github.respond("PUT", "repos/acme/repo/pulls/12/merge", 200, json!({ "sha": "m1", "merged": true, "message": "Merged" }));
    let body = json!({ "method": "squash", "commit_title": "T (#12)", "commit_message": "Body", "expected_head_sha": "abc" });
    let response = fixture.send("POST", &pull_path(&fixture, "/merge"), &fixture.session_cookie, Some(body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["sha"], "m1");
    let sent = fixture.with_requests(|requests| requests.iter().find(|request| request.0 == "PUT").unwrap().3.clone());
    assert_eq!(sent, json!({ "merge_method": "squash", "sha": "abc", "commit_title": "T (#12)", "commit_message": "Body" }));
}

#[tokio::test]
async fn merge_head_changed_is_409() {
    let fixture = connected().await;
    fixture.github.respond("PUT", "repos/acme/repo/pulls/12/merge", 409, json!({ "message": "Head branch was modified." }));
    let body = json!({ "method": "merge", "expected_head_sha": "abc" });
    let response = fixture.send("POST", &pull_path(&fixture, "/merge"), &fixture.session_cookie, Some(body)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["code"], "pull_head_changed");
}

#[tokio::test]
async fn merge_blocked_is_422_with_github_message() {
    let fixture = connected().await;
    fixture.github.respond("PUT", "repos/acme/repo/pulls/12/merge", 405, json!({ "message": "Required status check \"ci\" is expected." }));
    let body = json!({ "method": "merge", "expected_head_sha": "abc" });
    let response = fixture.send("POST", &pull_path(&fixture, "/merge"), &fixture.session_cookie, Some(body)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["code"], "merge_blocked");
    assert!(body["detail"].as_str().unwrap().contains("Required status check"));
}

fn rest_pull(merged: bool, head_repo: &str) -> Value {
    json!({ "merged": merged, "head": { "ref": "fix/login", "repo": { "full_name": head_repo } }, "base": { "repo": { "full_name": "acme/repo" } } })
}

#[tokio::test]
async fn delete_branch_after_merge() {
    let fixture = connected().await;
    fixture.github.respond("GET", "repos/acme/repo/pulls/12", 200, rest_pull(true, "acme/repo"));
    fixture.github.respond("DELETE", "repos/acme/repo/git/refs/heads/fix/login", 204, json!({}));
    let response = fixture.send("DELETE", &pull_path(&fixture, "/branch"), &fixture.session_cookie, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_branch_refuses_fork_and_unmerged() {
    for (merged, repo) in [(false, "acme/repo"), (true, "someone/fork")] {
        let fixture = connected().await;
        fixture.github.respond("GET", "repos/acme/repo/pulls/12", 200, rest_pull(merged, repo));
        let response = fixture.send("DELETE", &pull_path(&fixture, "/branch"), &fixture.session_cookie, None).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY, "{merged} {repo}");
        assert!(fixture.with_requests(|requests| requests.iter().all(|request| request.0 != "DELETE")));
    }
}

#[tokio::test]
async fn write_routes_respect_access() {
    let fixture = connected().await;
    let (member, cookie) = fixture.add_member("member@example.com").await;
    fixture.connect(member, (2, "member"), "ghu_member", None, None).await;
    sqlx::query("UPDATE projects SET private = 1 WHERE id = $1")
        .bind(fixture.project_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let writes: [(&str, &str, Option<Value>); 8] = [
        ("POST", "/comments", Some(comment_body("pending"))),
        ("POST", "/threads/T_1/replies", Some(json!({ "body": "x" }))),
        ("PATCH", "/comments/C_1", Some(json!({ "body": "x" }))),
        ("PUT", "/threads/T_1/resolved", None),
        ("POST", "/review", Some(json!({ "event": "COMMENT", "body": "x" }))),
        ("DELETE", "/review", None),
        ("POST", "/merge", Some(json!({ "method": "merge", "expected_head_sha": "abc" }))),
        ("DELETE", "/branch", None),
    ];
    for (method, rest, body) in writes {
        let response = fixture.send(method, &pull_path(&fixture, rest), &cookie, body).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {rest}");
    }
    assert!(fixture.with_requests(|requests| requests.is_empty()));
}

#[tokio::test]
async fn bad_node_ids_are_404() {
    let fixture = connected().await;
    let response = fixture
        .send("PUT", &pull_path(&fixture, "/threads/T%201%3B/resolved"), &fixture.session_cookie, None)
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
