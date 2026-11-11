//! Reading pull requests with the member's own GitHub token (plan 2 of PR review).
#![recursion_limit = "256"]

mod fake_github;

use axum::http::StatusCode;
use fake_github::{GithubFixture, fixture, json_body};
use serde_json::{Value, json};

const PULL: &str = "acme/repo/12";

fn pull_path(fixture: &GithubFixture, rest: &str) -> String {
    format!("/api/v1/workspaces/{}/github/pulls/{PULL}{rest}", fixture.workspace())
}

/// The app, the `acme/repo` link to the project, and the owner connected as GitHub user `octo` (token `ghu_user`).
async fn connected() -> GithubFixture {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    fixture
        .connect(fixture.user_id, (1, "octo"), "ghu_user", None, None)
        .await;
    fixture
}

fn detail(merge_state: &str) -> Value {
    json!({
        "viewer": { "login": "octo" },
        "repository": {
            "mergeCommitAllowed": true, "squashMergeAllowed": true, "rebaseMergeAllowed": false,
            "viewerDefaultMergeMethod": "SQUASH", "viewerPermission": "WRITE",
            "pullRequest": {
                "id": "PR_1", "number": 12, "title": "Fix it", "body": "Body", "state": "OPEN", "isDraft": false,
                "merged": false, "locked": false, "url": "https://github.com/acme/repo/pull/12",
                "additions": 3, "deletions": 1, "changedFiles": 2, "baseRefName": "main", "headRefName": "fix",
                "baseRefOid": "b0", "headRefOid": "h1", "mergeable": "MERGEABLE", "mergeStateStatus": merge_state,
                "author": { "login": "hubot", "avatarUrl": "https://avatars.test/hubot" },
                "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "SUCCESS", "contexts": { "nodes": [
                    { "__typename": "CheckRun", "name": "ci", "conclusion": "SUCCESS", "status": "COMPLETED", "detailsUrl": "https://ci.test/1" },
                    { "__typename": "StatusContext", "context": "lint", "state": "PENDING", "targetUrl": null }
                ] } } } }] }
            }
        }
    })
}

#[tokio::test]
async fn pull_detail_uses_user_token() {
    let fixture = connected().await;
    fixture.github.graphql("query PullDetail", detail("CLEAN"));
    let response = fixture.get(&pull_path(&fixture, "")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["title"], "Fix it");
    assert_eq!(body["state"], "open");
    assert_eq!(body["head_sha"], "h1");
    assert_eq!(body["repository"], "acme/repo");
    assert_eq!(body["merge"]["methods"], json!(["merge", "squash"]));
    assert_eq!(body["merge"]["default_method"], "squash");
    assert_eq!(body["merge"]["blocked_reason"], Value::Null);
    assert_eq!(body["viewer"]["can_merge"], true);
    assert_eq!(body["viewer"]["is_author"], false);
    assert_eq!(body["checks"]["state"], "pending");
    assert_eq!(body["checks"]["runs"][0]["name"], "ci");
    assert_eq!(body["checks"]["runs"][1]["state"], "pending");
    let requests = fixture.github.with(|state| state.api_requests.clone());
    assert!(requests.iter().all(|request| request.2 == "Bearer ghu_user"));
}

#[tokio::test]
async fn merge_info_maps_state_status() {
    let fixture = connected().await;
    fixture.github.graphql("query PullDetail", detail("DIRTY"));
    let body = json_body(fixture.get(&pull_path(&fixture, "")).await).await;
    assert_eq!(body["merge"]["blocked_reason"], "This branch has conflicts.");
}

#[tokio::test]
async fn repo_not_connected_is_404() {
    let fixture = connected().await;
    let response = fixture
        .get(&format!("/api/v1/workspaces/{}/github/pulls/acme/other/12", fixture.workspace()))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(fixture.github.with(|state| state.api_requests.is_empty()));
}

#[tokio::test]
async fn bad_names_are_404() {
    let fixture = connected().await;
    for path in ["acme/repo/0", "acme/re%2Fpo/1", "acme/..%2F..%2Fuser/1"] {
        let response = fixture
            .get(&format!("/api/v1/workspaces/{}/github/pulls/{path}", fixture.workspace()))
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn private_project_hidden_is_404() {
    let fixture = connected().await;
    let (member, cookie) = fixture.add_member("member@example.com").await;
    fixture.connect(member, (2, "member"), "ghu_member", None, None).await;
    fixture.github.graphql("query PullDetail", detail("CLEAN"));
    assert_eq!(
        fixture.send("GET", &pull_path(&fixture, ""), &cookie, None).await.status(),
        StatusCode::OK
    );
    sqlx::query("UPDATE projects SET private = 1 WHERE id = $1")
        .bind(fixture.project_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(
        fixture.send("GET", &pull_path(&fixture, ""), &cookie, None).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn github_404_is_pull_not_found() {
    let fixture = connected().await;
    let response = fixture.get(&pull_path(&fixture, "")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["code"], "pull_not_found");
}

#[tokio::test]
async fn github_401_marks_broken() {
    let fixture = connected().await;
    fixture.github.with(|state| state.tokens.clear());
    let response = fixture.get(&pull_path(&fixture, "")).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["code"], "github_reconnect");
    let me = format!("/api/v1/workspaces/{}/github/me", fixture.workspace());
    assert_eq!(json_body(fixture.get(&me).await).await["status"], "broken");
}

#[tokio::test]
async fn not_connected_is_409_github_not_connected() {
    let fixture = fixture().await;
    fixture.seed_app(true).await;
    let response = fixture.get(&pull_path(&fixture, "")).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["code"], "github_not_connected");
}

fn search_q(request: &str) -> String {
    let url = url::Url::parse(&format!("https://x/{request}")).unwrap();
    url.query_pairs().find(|(key, _)| key == "q").unwrap().1.into_owned()
}

fn file(index: usize, patch: bool) -> Value {
    let mut value = json!({ "filename": format!("src/file{index}.rs"), "status": "modified", "additions": 1, "deletions": 0 });
    if patch {
        value["patch"] = json!("@@ -1 +1,2 @@\n a\n+b");
    }
    value
}

#[tokio::test]
async fn files_paginate_and_keep_missing_patch() {
    let fixture = connected().await;
    let first: Vec<Value> = (0..100).map(|index| file(index, true)).collect();
    fixture.github.respond("GET", "repos/acme/repo/pulls/12/files?per_page=100&page=1", 200, json!(first));
    fixture.github.respond("GET", "repos/acme/repo/pulls/12/files?per_page=100&page=2", 200, json!([
        { "filename": "big.lock", "previous_filename": "old.lock", "status": "renamed", "additions": 9000, "deletions": 0 }
    ]));
    fixture.github.graphql("query PullFiles", json!({ "repository": { "pullRequest": { "files": {
        "nodes": [{ "path": "src/file3.rs", "viewerViewedState": "VIEWED" }],
        "pageInfo": { "hasNextPage": false, "endCursor": null }
    } } } }));
    let response = fixture.get(&pull_path(&fixture, "/files")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let files = json_body(response).await;
    assert_eq!(files.as_array().unwrap().len(), 101);
    assert_eq!(files[3]["viewed"], true);
    assert_eq!(files[4]["viewed"], false);
    assert_eq!(files[100]["path"], "big.lock");
    assert_eq!(files[100]["previous_path"], "old.lock");
    assert_eq!(files[100]["patch"], Value::Null);
    assert_eq!(files[0]["patch"], "@@ -1 +1,2 @@\n a\n+b");
}

#[tokio::test]
async fn list_filters_build_search_query() {
    let fixture = connected().await;
    fixture.github.respond("GET", "search/issues", 200, json!({ "total_count": 1, "items": [{
        "number": 12, "title": "Fix it", "state": "open", "draft": false, "comments": 2, "updated_at": "2026-11-10T10:00:00Z",
        "user": { "login": "hubot", "avatar_url": "https://avatars.test/hubot" },
        "repository_url": "https://api.github.com/repos/acme/repo", "pull_request": { "merged_at": null }
    }] }));
    let path = format!(
        "/api/v1/workspaces/{}/projects/{}/github/pulls?filter=review",
        fixture.workspace(),
        fixture.project_id
    );
    // One project on the repository: every PR of the repository, no label filter.
    assert_eq!(fixture.get(&path).await.status(), StatusCode::OK);
    let single = fixture.with_requests(|requests| requests[0].1.clone());
    assert!(!search_q(&single).contains("label:"));
    // A second project on the same repository: the label tells them apart.
    let other = orbit_platform::Id::new_v7().to_string();
    sqlx::query("INSERT INTO projects (id, workspace_id, name, project_key, color, created_at, updated_at) VALUES ($1, $2, 'Docs', 'DOC', '#ff0000', 0, 0)")
        .bind(&other).bind(fixture.workspace()).execute(fixture.database.pool()).await.unwrap();
    sqlx::query("INSERT INTO github_project_connections (project_id, workspace_id, installation_id, repository, label) VALUES ($1, $2, 1234, 'acme/repo', 'Docs')")
        .bind(&other).bind(fixture.workspace()).execute(fixture.database.pool()).await.unwrap();
    fixture.github.with(|state| state.api_requests.clear());
    let response = fixture.get(&path).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["items"][0]["title"], "Fix it");
    assert_eq!(body["items"][0]["state"], "open");
    assert_eq!(body["items"][0]["repository"], "acme/repo");
    assert_eq!(body["next_cursor"], Value::Null);
    let request = fixture.with_requests(|requests| requests[0].1.clone());
    let q = search_q(&request);
    for part in ["is:pr", "repo:acme/repo", "label:\"Orbit\"", "review-requested:@me", "is:open"] {
        assert!(q.contains(part), "{q} lacks {part}");
    }
}

#[tokio::test]
async fn outdated_thread_has_null_line() {
    let fixture = connected().await;
    fixture.github.graphql("query PullThreads", json!({ "repository": { "pullRequest": { "reviewThreads": {
        "nodes": [{
            "id": "T_1", "path": "src/a.rs", "line": null, "startLine": null, "diffSide": "RIGHT", "startDiffSide": null,
            "isResolved": false, "isOutdated": true, "viewerCanResolve": true, "viewerCanReply": true,
            "comments": { "nodes": [{
                "id": "C_1", "databaseId": 55, "body": "Why?", "createdAt": "2026-11-10T10:00:00Z", "state": "SUBMITTED",
                "viewerCanUpdate": false, "author": { "login": "hubot", "avatarUrl": "https://avatars.test/hubot" }
            }] }
        }],
        "pageInfo": { "hasNextPage": false, "endCursor": null }
    } } } }));
    let response = fixture.get(&pull_path(&fixture, "/threads")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let thread = &body["threads"][0];
    assert_eq!(thread["line"], Value::Null);
    assert_eq!(thread["is_outdated"], true);
    assert_eq!(thread["side"], "RIGHT");
    assert_eq!(thread["comments"][0]["database_id"], 55);
    assert_eq!(thread["comments"][0]["pending"], false);
    assert_eq!(body["pending_review"], Value::Null);
}

#[tokio::test]
async fn contents_are_text_or_nothing() {
    let fixture = connected().await;
    fixture.github.respond("GET", "repos/acme/repo/contents/src/a.rs?ref=h1", 200, json!({
        "type": "file", "encoding": "base64", "size": 6, "content": "aGVsbG8K"
    }));
    fixture.github.respond("GET", "repos/acme/repo/contents/logo.png?ref=h1", 200, json!({
        "type": "file", "encoding": "base64", "size": 4, "content": "/9j/4A=="
    }));
    let text = json_body(fixture.get(&pull_path(&fixture, "/contents?path=src/a.rs&ref=h1")).await).await;
    assert_eq!(text["text"], "hello\n");
    let binary = json_body(fixture.get(&pull_path(&fixture, "/contents?path=logo.png&ref=h1")).await).await;
    assert_eq!(binary["text"], Value::Null);
}

#[tokio::test]
async fn marking_a_file_viewed_uses_the_pull_id() {
    let fixture = connected().await;
    fixture.github.graphql("query PullId", json!({ "repository": { "pullRequest": { "id": "PR_1" } } }));
    fixture.github.graphql("mutation MarkViewed", json!({ "markFileAsViewed": { "clientMutationId": null } }));
    let response = fixture
        .send("PUT", &pull_path(&fixture, "/viewed"), &fixture.session_cookie, Some(json!({ "path": "src/a.rs" })))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let requests = fixture.github.graphql_requests();
    let (_, variables) = requests.iter().find(|(query, _)| query.contains("mutation MarkViewed")).unwrap();
    assert_eq!(variables["pullId"], "PR_1");
    assert_eq!(variables["path"], "src/a.rs");
}

#[tokio::test]
async fn conversation_merges_comments_and_reviews_by_time() {
    let fixture = connected().await;
    fixture.github.graphql("query PullConversation", json!({ "repository": { "pullRequest": {
        "comments": { "nodes": [
            { "body": "Second", "createdAt": "2026-11-10T11:00:00Z", "author": { "login": "a", "avatarUrl": "https://x/a" } }
        ] },
        "reviews": { "nodes": [
            { "body": "First", "state": "APPROVED", "submittedAt": "2026-11-10T10:00:00Z", "author": { "login": "b", "avatarUrl": "https://x/b" } }
        ] }
    } } }));
    let body = json_body(fixture.get(&pull_path(&fixture, "/conversation")).await).await;
    assert_eq!(body[0]["kind"], "review");
    assert_eq!(body[0]["review_state"], "approved");
    assert_eq!(body[1]["body"], "Second");
}

fn search_item(number: i64, repository: &str) -> Value {
    json!({
        "number": number, "title": format!("PR {number}"), "state": "open", "draft": false, "comments": 0,
        "updated_at": "2026-11-10T10:00:00Z", "user": { "login": "hubot", "avatar_url": "" },
        "repository_url": format!("https://api.github.com/repos/{repository}"), "pull_request": { "merged_at": null }
    })
}

#[tokio::test]
async fn search_text_cannot_reach_other_repositories() {
    let fixture = connected().await;
    fixture.github.respond("GET", "search/issues", 200, json!({ "total_count": 2, "items": [
        search_item(1, "acme/repo"), search_item(2, "acme/secret")
    ] }));
    let path = format!(
        "/api/v1/workspaces/{}/projects/{}/github/pulls?q=repo:acme/secret%20login",
        fixture.workspace(),
        fixture.project_id
    );
    let body = json_body(fixture.get(&path).await).await;
    let numbers: Vec<i64> = body["items"].as_array().unwrap().iter().map(|item| item["number"].as_i64().unwrap()).collect();
    assert_eq!(numbers, vec![1]);
    let q = search_q(&fixture.with_requests(|requests| requests[0].1.clone()));
    assert!(!q.contains("acme/secret"), "{q}");
    assert!(q.contains("login"), "{q}");
}

#[tokio::test]
async fn a_trashed_project_does_not_turn_on_the_label_filter() {
    let fixture = connected().await;
    fixture.github.respond("GET", "search/issues", 200, json!({ "total_count": 0, "items": [] }));
    let other = orbit_platform::Id::new_v7().to_string();
    sqlx::query("INSERT INTO projects (id, workspace_id, name, project_key, color, created_at, updated_at, deleted_at) VALUES ($1, $2, 'Old', 'OLD', '#ff0000', 0, 0, 1)")
        .bind(&other).bind(fixture.workspace()).execute(fixture.database.pool()).await.unwrap();
    sqlx::query("INSERT INTO github_project_connections (project_id, workspace_id, installation_id, repository, label) VALUES ($1, $2, 1234, 'acme/repo', 'Old')")
        .bind(&other).bind(fixture.workspace()).execute(fixture.database.pool()).await.unwrap();
    let path = format!("/api/v1/workspaces/{}/projects/{}/github/pulls", fixture.workspace(), fixture.project_id);
    assert_eq!(fixture.get(&path).await.status(), StatusCode::OK);
    assert!(!search_q(&fixture.with_requests(|requests| requests[0].1.clone())).contains("label:"));
}

#[tokio::test]
async fn merge_base_comes_from_the_compare_of_base_and_head() {
    let fixture = connected().await;
    fixture.github.graphql("query PullDetail", detail("CLEAN"));
    fixture.github.respond("GET", "repos/acme/repo/compare/b0...h1?per_page=1", 200, json!({ "merge_base_commit": { "sha": "m0" }, "files": [] }));
    let response = fixture.get(&pull_path(&fixture, "/merge-base")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["sha"], "m0");
}

#[tokio::test]
async fn graphql_asks_for_merge_state_previews() {
    let fixture = connected().await;
    fixture.github.graphql("query PullDetail", detail("CLEAN"));
    assert_eq!(fixture.get(&pull_path(&fixture, "")).await.status(), StatusCode::OK);
    let accepts = fixture.github.with(|state| state.graphql_accepts.clone());
    assert!(accepts.iter().all(|accept| accept.contains("application/vnd.github.merge-info-preview+json")), "{accepts:?}");
}
