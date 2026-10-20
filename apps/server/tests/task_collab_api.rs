//! Live co-editing of task descriptions: the task room, its projection into `description_json` and
//! the markdown copy, access, GitHub locks, and delete/restore while a room is open.

use orbit_platform::{Id, TimestampMillis};
use orbit_server::collab::blocknote::doc_to_blocks;
use orbit_server::repositories::tasks::{GithubWorkItem, TaskRepository};
use serde_json::{Value, json};
use tempfile::TempDir;

mod collab_support;
#[allow(dead_code)]
mod shared_postgres;

use collab_support::*;

/// Creates a task in the workspace's first project with a markdown description.
async fn create_task(server: &Server, users: &Users, description: &str) -> Value {
    let pool = server.database.pool();
    let project_id: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE workspace_id = $1 ORDER BY created_at")
            .bind(&users.workspace)
            .fetch_one(pool)
            .await
            .unwrap();
    let status_id: String = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
    )
    .bind(&project_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let (status, task) = server
        .api(
            &users.owner,
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", users.workspace),
            Some(json!({
                "project_id": project_id,
                "status_id": status_id,
                "title": "Task",
                "description": description,
            })),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    task
}

async fn get_task(server: &Server, users: &Users, id: &str) -> Value {
    let (status, task) = server
        .api(
            &users.owner,
            "GET",
            &format!("/api/v1/workspaces/{}/tasks/{id}", users.workspace),
            None,
        )
        .await;
    assert_eq!(status, 200, "{task}");
    task
}

async fn open(server: &Server, users: &Users, token: &str, id: &str) -> Client {
    Client::open_doc(server, users, token, "tasks", id).await
}

async fn wait_description(server: &Server, users: &Users, id: &str, expected: &str) -> Value {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let task = get_task(server, users, id).await;
        if task["description"] == expected {
            return task;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "description is {:?}, expected {expected:?}",
            task["description"]
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn edit_projects_json_and_markdown() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();
    let version = get_task(&server, &users, id).await["version"].clone();

    let mut client = open(&server, &users, &users.owner, id).await;
    client.type_text(0, "Hello ").await;
    let task = wait_description(&server, &users, id, "Hello Start\n").await;
    assert_eq!(task["description_json"][0]["type"], "paragraph");
    assert_eq!(
        task["description_json"][0]["content"][0]["text"],
        "Hello Start"
    );
    assert_eq!(
        task["version"], version,
        "co-editing does not bump the version"
    );
    server.stop().await;
}

#[tokio::test]
async fn old_markdown_task_opens_as_blocks() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "# Title\n\n- a").await;
    let id = task["id"].as_str().unwrap();

    let client = open(&server, &users, &users.owner, id).await;
    let blocks = doc_to_blocks(&client.doc);
    assert_eq!(blocks[0]["type"], "heading");
    assert_eq!(blocks[0]["content"][0]["text"], "Title");
    assert_eq!(blocks[1]["type"], "bulletListItem");
    assert_eq!(blocks[1]["content"][0]["text"], "a");
    server.stop().await;
}

#[tokio::test]
async fn two_sockets_same_user_merge() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();

    let mut first = open(&server, &users, &users.owner, id).await;
    let mut second = open(&server, &users, &users.owner, id).await;
    first.type_text(0, "A").await;
    second.wait_text("AStart").await;
    second.type_text(0, "B").await;
    first.wait_text("BAStart").await;
    server.stop().await;
}

#[tokio::test]
async fn github_linked_task_is_read_only() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();
    sqlx::query(
        "INSERT INTO github_issue_links (workspace_id, repository, issue_number, task_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&users.workspace)
    .bind("owner/repo")
    .bind(7)
    .bind(id)
    .execute(server.database.pool())
    .await
    .unwrap();

    let mut client = open(&server, &users, &users.owner, id).await;
    client.type_text(0, "Edit ").await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let other = open(&server, &users, &users.member, id).await;
    assert_eq!(other.text(), "Start");
    assert_eq!(get_task(&server, &users, id).await["description"], "Start");
    server.stop().await;
}

#[tokio::test]
async fn deleted_task_closes_room_and_restore_reopens() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();

    let mut client = open(&server, &users, &users.owner, id).await;
    client.type_text(0, "Kept ").await;
    let task = wait_description(&server, &users, id, "Kept Start\n").await;
    let (status, body) = server
        .api(
            &users.owner,
            "DELETE",
            &format!(
                "/api/v1/workspaces/{}/tasks/{id}?expected_version={}",
                users.workspace, task["version"]
            ),
            None,
        )
        .await;
    assert_eq!(status, 204, "{body}");
    assert_eq!(client.wait_close().await, 4404);

    let version: i64 = scalar_i64(&server, "SELECT version FROM tasks WHERE id = $1", id).await;
    let (status, body) = server
        .api(
            &users.owner,
            "POST",
            &format!("/api/v1/workspaces/{}/tasks/{id}/restore", users.workspace),
            Some(json!({"expected_version": version})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let reopened = open(&server, &users, &users.owner, id).await;
    assert_eq!(reopened.text(), "Kept Start");
    server.stop().await;
}

#[tokio::test]
async fn non_member_is_refused() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();
    let (_, outsider) = add_user(&server, None, "outsider@example.com", "Otto Outsider").await;

    let url = server.doc_ws_url(&users.workspace, "tasks", id, "v=1&epoch=x");
    let refused = server
        .socket(&url, Some(&outsider), Some(&server.base))
        .await
        .err();
    assert_eq!(refused, Some(404));
    server.stop().await;
}

#[tokio::test]
async fn audit_at_most_once_per_interval() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();

    let mut client = open(&server, &users, &users.owner, id).await;
    client.type_text(0, "A").await;
    wait_description(&server, &users, id, "AStart\n").await;
    client.type_text(0, "B").await;
    wait_description(&server, &users, id, "BAStart\n").await;
    let audits = scalar_i64(
        &server,
        "SELECT COUNT(*) FROM audit_events WHERE resource_id = $1 AND action = 'task.updated'",
        id,
    )
    .await;
    assert_eq!(audits, 1);
    server.stop().await;
}

#[tokio::test]
async fn rest_markdown_write_reaches_live_client() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();

    let mut client = open(&server, &users, &users.member, id).await;
    let version = get_task(&server, &users, id).await["version"].clone();
    let (status, body) = server
        .api(
            &users.owner,
            "PATCH",
            &format!("/api/v1/workspaces/{}/tasks/{id}", users.workspace),
            Some(json!({"expected_version": version, "description": "New text"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    client.wait_text("New text").await;
    let task = get_task(&server, &users, id).await;
    assert_eq!(task["description"], "New text");
    assert_eq!(
        task["description_json"][0]["content"][0]["text"],
        "New text"
    );
    server.stop().await;
}

#[tokio::test]
async fn github_sync_replaces_doc() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let project_id: Id =
        sqlx::query_scalar::<_, String>("SELECT id FROM projects WHERE workspace_id = $1")
            .bind(&users.workspace)
            .fetch_one(server.database.pool())
            .await
            .unwrap()
            .parse()
            .unwrap();
    let workspace_id: Id = users.workspace.parse().unwrap();
    let tasks = TaskRepository::new(server.database.clone());
    let issue = |description: &str| GithubWorkItem {
        repository: "owner/repo".to_owned(),
        number: 7,
        project_id,
        title: "Issue".to_owned(),
        description: description.to_owned(),
        kind: "issue",
        state: "open",
        state_changed: false,
    };
    let id = tasks
        .sync_github_work_item(
            workspace_id,
            users.owner_id,
            issue("First"),
            "sync-1",
            TimestampMillis::now(),
        )
        .await
        .unwrap()
        .to_string();

    let mut client = open(&server, &users, &users.owner, &id).await;
    assert_eq!(client.text(), "First");
    tasks
        .sync_github_work_item(
            workspace_id,
            users.owner_id,
            issue("Second"),
            "sync-2",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    client.wait_text("Second").await;
    assert_eq!(
        get_task(&server, &users, &id).await["description"],
        "Second"
    );
    server.stop().await;
}

async fn description_mentions(server: &Server, recipient: Id) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE recipient_user_id = $1 AND kind = 'description_mentioned'",
    )
    .bind(recipient.to_string())
    .fetch_one(server.database.pool())
    .await
    .unwrap()
}

fn mention_paragraph(user: Id, name: &str, text: &str) -> Value {
    json!([{"type": "paragraph", "content": [
        {"type": "text", "text": text, "styles": {}},
        {"type": "mention", "props": {"userId": user.to_string(), "name": name}}
    ]}])
}

#[tokio::test]
async fn new_description_mention_notifies_once() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();
    let path = format!("/api/v1/workspaces/{}/tasks/{id}", users.workspace);

    let (status, task) = server
        .api(
            &users.owner,
            "PATCH",
            &path,
            Some(json!({
                "expected_version": task["version"],
                "description_json": mention_paragraph(users.member_id, "Mia Member", "Ask "),
            })),
        )
        .await;
    assert_eq!(status, 200, "{task}");
    assert_eq!(description_mentions(&server, users.member_id).await, 1);
    let (status, body) = server
        .api(
            &users.owner,
            "PATCH",
            &path,
            Some(json!({
                "expected_version": task["version"],
                "description_json": mention_paragraph(users.member_id, "Mia Member", "Still ask "),
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(description_mentions(&server, users.member_id).await, 1);
    server.stop().await;
}

#[tokio::test]
async fn self_mention_is_ignored() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();
    let (status, body) = server
        .api(
            &users.owner,
            "PATCH",
            &format!("/api/v1/workspaces/{}/tasks/{id}", users.workspace),
            Some(json!({
                "expected_version": task["version"],
                "description_json": mention_paragraph(users.owner_id, "Olivia Owner", "Me "),
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(description_mentions(&server, users.owner_id).await, 0);
    server.stop().await;
}

#[tokio::test]
async fn imported_mentions_do_not_notify_on_first_projection() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Ask @Mia Member").await;
    let id = task["id"].as_str().unwrap();

    let mut client = open(&server, &users, &users.owner, id).await;
    client.type_text(0, "Please ").await;
    let task = wait_description(&server, &users, id, "Please Ask @Mia Member\n").await;
    assert_eq!(task["description_json"][0]["content"][1]["type"], "mention");
    assert_eq!(description_mentions(&server, users.member_id).await, 0);
    server.stop().await;
}

#[tokio::test]
async fn live_mention_notifies_after_projection() {
    use orbit_server::collab::blocknote::FRAGMENT;
    use yrs::{ReadTxn, Xml, XmlElementPrelim, XmlFragment, XmlOut};

    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();

    let mut client = open(&server, &users, &users.owner, id).await;
    let member = users.member_id.to_string();
    client
        .edit(|_, txn| {
            let fragment = txn.get_xml_fragment(FRAGMENT).expect("fragment");
            let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
                panic!("group")
            };
            let Some(XmlOut::Element(container)) = group.get(txn, 0) else {
                panic!("block")
            };
            let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
                panic!("paragraph")
            };
            let mention = paragraph.push_back(txn, XmlElementPrelim::empty("mention"));
            mention.insert_attribute(txn, "userId", member.as_str());
            mention.insert_attribute(txn, "name", "Mia Member");
        })
        .await;
    wait_until("the mention notification", || async {
        description_mentions(&server, users.member_id).await == 1
    })
    .await;
    server.stop().await;
}

#[tokio::test]
async fn mentions_in_a_new_task_description_notify() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let base = create_task(&server, &users, "").await;
    let (status, task) = server
        .api(
            &users.owner,
            "POST",
            &format!("/api/v1/workspaces/{}/tasks", users.workspace),
            Some(json!({
                "project_id": base["project_id"],
                "status_id": base["status_id"],
                "title": "Mentions",
                "description_json": mention_paragraph(users.member_id, "Mia Member", "Ask "),
            })),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    assert_eq!(description_mentions(&server, users.member_id).await, 1);
    server.stop().await;
}

#[tokio::test]
async fn a_mention_after_ten_minutes_notifies_again() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Start").await;
    let id = task["id"].as_str().unwrap();
    let path = format!("/api/v1/workspaces/{}/tasks/{id}", users.workspace);
    let mut version = task["version"].clone();
    let mut write = async |blocks: Value| {
        let (status, body) = server
            .api(
                &users.owner,
                "PATCH",
                &path,
                Some(json!({"expected_version": version, "description_json": blocks})),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        version = body["version"].clone();
    };
    let plain = json!([{"type": "paragraph", "content": [{"type": "text", "text": "No one", "styles": {}}]}]);

    write(mention_paragraph(users.member_id, "Mia Member", "Ask ")).await;
    write(plain.clone()).await;
    write(mention_paragraph(users.member_id, "Mia Member", "Ask ")).await;
    assert_eq!(
        description_mentions(&server, users.member_id).await,
        1,
        "within ten minutes: one notification"
    );

    sqlx::query("UPDATE notifications SET created_at = created_at - 660000 WHERE kind = 'description_mentioned'")
        .execute(server.database.pool())
        .await
        .unwrap();
    write(plain).await;
    write(mention_paragraph(users.member_id, "Mia Member", "Ask ")).await;
    assert_eq!(description_mentions(&server, users.member_id).await, 2);
    server.stop().await;
}

#[tokio::test]
async fn a_renamed_member_is_not_notified_for_an_old_mention() {
    let root = TempDir::new().unwrap();
    let server = Server::start(&root, fast_config()).await;
    let users = setup(&server).await;
    let task = create_task(&server, &users, "Ask @Mia Member").await;
    let id = task["id"].as_str().unwrap();

    // the first open converts the markdown, with the mention
    let first = open(&server, &users, &users.owner, id).await;
    drop(first);
    sqlx::query("UPDATE users SET display_name = 'Mia Renamed' WHERE id = $1")
        .bind(users.member_id.to_string())
        .execute(server.database.pool())
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let mut client = open(&server, &users, &users.owner, id).await;
    client.type_text(0, "Please ").await;
    wait_until("the projection", || async {
        get_task(&server, &users, id).await["description"]
            .as_str()
            .is_some_and(|text| text.starts_with("Please"))
    })
    .await;
    assert_eq!(description_mentions(&server, users.member_id).await, 0);
    server.stop().await;
}
