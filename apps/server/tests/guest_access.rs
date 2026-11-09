//! A guest only reaches chat. Every workspace operation of the OpenAPI document is called as a
//! guest with the ids of real records; each must fail unless `guest_may_use` names it. A new route
//! fails this test until someone decides whether guests may use it.

use std::collections::{BTreeMap, BTreeSet};

use orbit_platform::{
    AuthenticatedUser, Config, EnvironmentMode, Id, PasswordService, TimestampMillis,
};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const COOKIE: &str = "orbit_session_dev";
const PREFIX: &str = "/api/v1/workspaces/{workspace_id}";

struct Server {
    base: String,
    database: orbit_platform::Database,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
    _root: TempDir,
}

impl Server {
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            app_key: Some(orbit_platform::AppKey::new([1; 32])),
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config.rate_limits.general_per_minute = 100_000;
        let app = App::build(config).await.unwrap();
        let database = app.database().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let task = tokio::spawn(async move {
            app.serve(listener, token).await.unwrap();
        });
        Self {
            base,
            database,
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
        session: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> (u16, Value) {
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("origin", &self.base)
            .header("cookie", format!("{COOKIE}={session}"));
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    /// A call that must succeed; returns its body.
    async fn ok(&self, session: &str, method: &str, path: &str, body: Value) -> Value {
        let body = (method != "GET").then_some(body);
        let (status, response) = self.call(session, method, path, body.as_ref()).await;
        assert!(
            (200..300).contains(&status),
            "{method} {path}: {status} {response}"
        );
        response
    }

    /// A user with a session and, unless `role` is `None`, a membership row.
    async fn person(&self, workspace: &str, name: &str, role: Option<&str>) -> (Id, String) {
        let id = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        let email = format!("{name}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(name)
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        if let Some(role) = role {
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 0, $5, $6)",
            )
            .bind(Id::new_v7().to_string())
            .bind(workspace)
            .bind(id.to_string())
            .bind(role)
            .bind(now)
            .bind(now)
            .execute(self.database.pool())
            .await
            .unwrap();
        }
        let session = IdentityRepository::new(self.database.clone())
            .create_session(
                &AuthenticatedUser {
                    id,
                    email,
                    display_name: name.to_owned(),
                },
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        (id, session.token)
    }
}

/// What a guest may call. Every other chat route names a public record here (`#general`, a
/// message in it) or needs a permission, and the profile routes name a member the guest shares
/// no conversation with, so they must fail too. The member list filters its rows for a guest
/// (see `workspaces_api.rs`).
fn guest_may_use(operation: &str) -> bool {
    matches!(
        operation,
        "get_workspace"
            | "list_members"
            | "list_chat_conversations"
            | "list_chat_categories"
            | "list_chat_emoji"
            | "get_chat_emoji_image"
            | "list_chat_stickers"
            | "get_chat_sticker_image"
            | "upload_chat_file"
            | "download_chat_file"
            | "get_chat_link_preview"
            | "read_all_chat"
            | "restore_chat_read"
            | "list_chat_search"
            | "list_chat_states"
            | "list_followed_chat_threads"
    )
}

/// Real records of the workspace, by the name of the path or body field that holds their id.
struct Records {
    ids: BTreeMap<&'static str, String>,
}

impl Records {
    fn id(&self, name: &str, path: &str) -> String {
        let key = match name {
            "comment_id" if path.contains("/pages/") => "page_comment_id",
            "user_id" | "user_ids" | "member_ids" | "assignee_ids" | "mentioned_user_ids" => {
                "user_id"
            }
            "id" => "random",
            other => other,
        };
        self.ids
            .get(key)
            .cloned()
            .unwrap_or_else(|| Id::new_v7().to_string())
    }
}

fn resolve<'a>(document: &'a Value, schema: &'a Value) -> &'a Value {
    match schema["$ref"].as_str() {
        Some(reference) => {
            let name = reference.rsplit('/').next().unwrap();
            resolve(document, &document["components"]["schemas"][name])
        }
        None => schema,
    }
}

/// A value that passes the schema, with real ids where a field names a record.
fn example(document: &Value, schema: &Value, field: &str, path: &str, records: &Records) -> Value {
    if schema["$ref"] == "#/components/schemas/ViewState" {
        return view_state();
    }
    let schema = resolve(document, schema);
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(variants) = schema[key].as_array() {
            let variant = variants
                .iter()
                .find(|variant| resolve(document, variant)["type"] != "null")
                .unwrap_or(&variants[0]);
            return example(document, variant, field, path, records);
        }
    }
    if let Some(values) = schema["enum"].as_array() {
        return values[0].clone();
    }
    let kind = match &schema["type"] {
        Value::Array(kinds) => kinds
            .iter()
            .find(|kind| *kind != "null")
            .cloned()
            .unwrap_or(Value::Null),
        kind => kind.clone(),
    };
    match kind.as_str().unwrap_or("object") {
        "string" => {
            if schema["format"] == "uuid" || field.ends_with("_id") || field.ends_with("_ids") {
                Value::String(records.id(field, path))
            } else {
                Value::String(
                    match field {
                        "email" => "someone@example.com",
                        "color" => "#336699",
                        "emoji" => "👍",
                        "page_key" => "all",
                        _ => "guest sweep",
                    }
                    .to_owned(),
                )
            }
        }
        "integer" | "number" if field.contains("version") => 0.into(),
        "integer" | "number" => schema["minimum"]
            .clone()
            .as_i64()
            .unwrap_or(1)
            .max(1)
            .into(),
        "boolean" => Value::Bool(false),
        "array" => {
            if schema["minItems"].as_u64().unwrap_or(0) > 0 || field.ends_with("_ids") {
                json!([example(document, &schema["items"], field, path, records)])
            } else {
                json!([])
            }
        }
        _ => {
            let required: BTreeSet<&str> = schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let mut object = serde_json::Map::new();
            for (name, property) in schema["properties"].as_object().into_iter().flatten() {
                if required.contains(name.as_str()) {
                    object.insert(
                        name.clone(),
                        example(document, property, name, path, records),
                    );
                }
            }
            Value::Object(object)
        }
    }
}

fn view_state() -> Value {
    json!({
        "filter": {"op": "and", "children": []},
        "display": {
            "layout": "list",
            "group_by": "status",
            "sub_group_by": "none",
            "order_by": "manual",
            "order_direction": "asc",
            "properties": ["id", "status"],
            "show_completed": "all",
            "show_empty_groups": false,
            "sub_issues": "nested"
        }
    })
}

/// The id of a created record, bare or wrapped in an object.
fn id_of(created: &Value) -> String {
    created["id"]
        .as_str()
        .or_else(|| {
            created
                .as_object()?
                .values()
                .find_map(|value| value["id"].as_str())
        })
        .unwrap_or_else(|| panic!("no id: {created}"))
        .to_owned()
}

/// The id of the first row of a list, bare or wrapped in an object.
fn first_id(list: &Value) -> String {
    let rows = list
        .as_array()
        .or_else(|| list.as_object()?.values().find_map(Value::as_array))
        .unwrap_or_else(|| panic!("not a list: {list}"));
    rows[0]["id"].as_str().unwrap().to_owned()
}

struct Operation {
    method: String,
    template: String,
    id: String,
    path: String,
    body: Option<Value>,
}

/// Every operation in the workspace scope, with real ids in the path and a body that passes its
/// schema. Deletes come last so that the member's run reaches as many records as possible.
fn operations(document: &Value, records: &Records) -> Vec<Operation> {
    let mut operations = Vec::new();
    for (template, item) in document["paths"].as_object().unwrap() {
        if !template.starts_with(PREFIX) {
            continue;
        }
        for (method, operation) in item.as_object().unwrap() {
            if !["get", "post", "patch", "put", "delete"].contains(&method.as_str()) {
                continue;
            }
            let mut path = template.clone();
            let mut query = Vec::new();
            for parameter in operation["parameters"].as_array().into_iter().flatten() {
                let parameter = resolve(document, parameter);
                let name = parameter["name"].as_str().unwrap();
                let value = match example(document, &parameter["schema"], name, template, records) {
                    Value::String(value) => value,
                    other => other.to_string(),
                };
                if parameter["in"] == "path" {
                    path = path.replace(&format!("{{{name}}}"), &value);
                } else if parameter["in"] == "query" && parameter["required"] == true {
                    query.push(format!("{name}={value}"));
                }
            }
            if !query.is_empty() {
                path = format!("{path}?{}", query.join("&"));
            }
            let id = operation["operationId"].as_str().unwrap();
            let body = operation["requestBody"]["content"]["application/json"]["schema"]
                .is_object()
                .then(|| {
                    body_for(id, records).unwrap_or_else(|| {
                        example(
                            document,
                            &operation["requestBody"]["content"]["application/json"]["schema"],
                            "",
                            template,
                            records,
                        )
                    })
                });
            operations.push(Operation {
                method: method.to_uppercase(),
                template: template.clone(),
                id: id.to_owned(),
                path,
                body,
            });
        }
    }
    // Children before parents, so that deleting a task does not hide its comments.
    operations.sort_by_key(|operation| {
        let delete = operation.method == "DELETE";
        (
            delete,
            std::cmp::Reverse(if delete { operation.template.len() } else { 0 }),
        )
    });
    operations
}

/// Bodies whose rules the schema does not state: formats, enums typed as strings, and arrays
/// whose items must name real records.
fn body_for(operation: &str, records: &Records) -> Option<Value> {
    let id = |name| records.id(name, "");
    let rich = json!([{
        "type": "paragraph",
        "content": [{"type": "text", "text": "guest sweep", "styles": {}}]
    }]);
    Some(match operation {
        "create_comment" => json!({"body": "guest sweep"}),
        "update_comment" => json!({"body": "guest sweep", "expected_version": 0}),
        "create_page_thread" | "create_page_comment" | "update_page_comment" => {
            json!({"body": rich})
        }
        "create_api_token" => json!({"name": "guest sweep", "scopes": ["docs:read"]}),
        "forward_chat_message" => {
            json!({"conversation_id": id("conversation_id"), "nonce": "sweep-forward"})
        }
        "create_project" => json!({"name": "Sweep", "key": "SWP", "color": "#336699"}),
        "update_project" => {
            json!({"name": "Sweep", "key": "SWEEP", "color": "#336699", "expected_version": 0})
        }
        "create_status" => json!({"name": "Sweep", "color": "#336699", "category": "unstarted"}),
        "update_status" => json!({
            "name": "Sweep", "color": "#336699", "category": "unstarted",
            "position": 1, "expected_version": 0
        }),
        "reorder_statuses" => {
            json!({"items": [{"id": id("status_id"), "expected_version": 0, "position": 1}]})
        }
        "reorder_tasks" => {
            json!({"items": [{"id": id("task_id"), "expected_version": 0, "position": 1}]})
        }
        "bulk_tasks" => json!({"updates": [{"id": id("task_id"), "expected_version": 0}]}),
        "create_task_relation" => json!({"type": "blocks", "task_id": id("other_task_id")}),
        _ => return None,
    })
}

/// Operations that no caller reaches past their own preconditions in this test, so the sweep
/// cannot prove that their access check runs. Each needs an outside service, a file upload, or
/// a record this test cannot make. The test fails when one of them becomes reachable, so that
/// it moves out of this list, and when any other operation cannot be checked.
const UNCHECKED: &[&str] = &[
    // Billing routes exist only when billing is configured.
    "get_billing",
    "billing_checkout",
    "billing_portal",
    // Multipart uploads, and the records only an upload makes.
    "create_chat_emoji",
    "delete_chat_emoji",
    "create_chat_sticker",
    "delete_chat_sticker",
    "upload_page_file",
    "download_page_file",
    "upload_task_attachments",
    "download_task_attachment",
    "delete_task_attachment",
    "create_attachment_comment",
    "download_comment_attachment",
    "create_obsidian_import",
    "delete_storage_file",
    // Imports check their token with the outside service first.
    "create_asana_import",
    "get_asana_import",
    "cancel_asana_import",
    "start_asana_import",
    "create_linear_import",
    "get_linear_import",
    "cancel_linear_import",
    "start_linear_import",
    "create_notion_import",
    "get_notion_import",
    "cancel_notion_import",
    "start_notion_import",
    "get_obsidian_import",
    "cancel_obsidian_import",
    "start_obsidian_import",
    "create_trello_import",
    "get_trello_import",
    "cancel_trello_import",
    "start_trello_import",
    // GitHub needs a public HTTPS origin and an installed App.
    "start_github_manifest",
    "save_github_project_connection",
    // S3 needs a reachable endpoint.
    "save_workspace_bucket",
    // Records made by background work or by other features: exports, notifications, page
    // versions, message buttons.
    "download_workspace_export",
    "read_notification",
    "get_page_version",
    "restore_page_version",
    "click_chat_action",
    // A restore needs a deleted record; deletes run last.
    "restore_page",
    "restore_project",
    "restore_task",
    "restore_workspace",
];

#[tokio::test]
async fn a_guest_reaches_only_chat() {
    let server = Server::start().await;
    let identity = IdentityRepository::new(server.database.clone());
    let now = TimestampMillis::now();
    identity
        .store_setup_token(
            "guest-sweep",
            TimestampMillis::from_millis(now.as_millis() + 60_000),
        )
        .await
        .unwrap();
    let setup = identity
        .complete_setup(
            SetupRequest {
                token: "guest-sweep".to_owned(),
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
    let owner = setup.session.token.clone();
    let workspace = setup.workspace_id.to_string();
    let base = format!("/api/v1/workspaces/{workspace}");
    let (member_id, _) = server.person(&workspace, "member", Some("member")).await;
    let (_, admin) = server.person(&workspace, "admin", Some("admin")).await;
    let (guest_id, guest) = server.person(&workspace, "guest", Some("guest")).await;

    // Records the guest must not reach, made by the owner.
    let project = first_id(
        &server
            .ok(&owner, "GET", &format!("{base}/projects"), Value::Null)
            .await,
    );
    let status = first_id(
        &server
            .ok(
                &owner,
                "GET",
                &format!("{base}/projects/{project}/statuses"),
                Value::Null,
            )
            .await,
    );
    let task = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/tasks"),
                json!({"project_id": project, "status_id": status, "title": "Secret task"}),
            )
            .await,
    );
    let comment = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/tasks/{task}/comments"),
                json!({"body": "Secret comment"}),
            )
            .await,
    );
    let label = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/projects/{project}/labels"),
                json!({"name": "Secret", "color": "#336699"}),
            )
            .await,
    );
    let teamspace = first_id(
        &server
            .ok(&owner, "GET", &format!("{base}/teamspaces"), Value::Null)
            .await,
    );
    let page = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/pages"),
                json!({"title": "Secret page", "teamspace_id": teamspace}),
            )
            .await,
    );
    let thread = server
        .ok(
            &owner,
            "POST",
            &format!("{base}/pages/{page}/threads"),
            json!({"body": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "Secret thread", "styles": {}}]
            }]}),
        )
        .await;
    let view = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/views"),
                json!({"name": "Secret view", "visibility": "workspace", "state": view_state()}),
            )
            .await,
    );
    let invitation = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/invitations"),
                json!({"email": "invited@example.com", "role": "member", "delivery": "manual"}),
            )
            .await,
    );
    let membership: String =
        sqlx::query_scalar("SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2")
            .bind(&workspace)
            .bind(member_id.to_string())
            .fetch_one(server.database.pool())
            .await
            .unwrap();
    let conversations = server
        .ok(
            &owner,
            "GET",
            &format!("{base}/chat/conversations"),
            Value::Null,
        )
        .await;
    let general = conversations
        .as_array()
        .or_else(|| {
            conversations
                .as_object()?
                .values()
                .find_map(Value::as_array)
        })
        .unwrap()
        .iter()
        .find(|conversation| conversation["name"] == "general")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let message = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/chat/conversations/{general}/messages"),
                json!({"body": "Public news", "nonce": "sweep-1"}),
            )
            .await,
    );
    // The guest's own private channel.
    let private = id_of(&server
        .ok(
            &owner,
            "POST",
            &format!("{base}/chat/conversations"),
            json!({"name": "with-guest", "kind": "private", "member_ids": [guest_id.to_string()]}),
        )
        .await);

    let other_task = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/tasks"),
                json!({"project_id": project, "status_id": status, "title": "Other task"}),
            )
            .await,
    );
    let relation = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/tasks/{task}/relations"),
                json!({"type": "blocks", "task_id": other_task}),
            )
            .await,
    );
    let category = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/chat/categories"),
                json!({"name": "Secret"}),
            )
            .await,
    );
    let webhook = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/chat/conversations/{general}/webhooks"),
                json!({"name": "Secret hook"}),
            )
            .await,
    );
    let token = id_of(
        &server
            .ok(
                &owner,
                "POST",
                &format!("{base}/api-tokens"),
                json!({"name": "Secret token", "scopes": ["docs:read"]}),
            )
            .await,
    );
    server
        .ok(
            &admin,
            "PUT",
            &format!("{base}/views/{view}/favorite"),
            Value::Null,
        )
        .await;
    server
        .ok(
            &admin,
            "PUT",
            &format!("{base}/pages/{page}/favorite"),
            Value::Null,
        )
        .await;
    server
        .ok(
            &admin,
            "PUT",
            &format!("{base}/chat/messages/{message}/reactions/%F0%9F%91%8D"),
            Value::Null,
        )
        .await;
    server
        .ok(
            &admin,
            "PUT",
            &format!("{base}/view-preferences/all"),
            json!({"state": view_state()}),
        )
        .await;

    let records = Records {
        ids: BTreeMap::from([
            ("workspace_id", workspace.clone()),
            ("project_id", project),
            ("status_id", status),
            ("task_id", task.clone()),
            ("task_ids", task),
            ("other_task_id", other_task),
            ("relation_id", relation),
            ("category_id", category),
            ("webhook_id", webhook),
            ("token_id", token),
            ("parent_task_id", Id::new_v7().to_string()),
            ("comment_id", comment),
            ("label_id", label.clone()),
            ("label_ids", label),
            ("teamspace_id", teamspace),
            ("page_id", page.clone()),
            ("parent_id", page),
            ("thread_id", thread["id"].as_str().unwrap().to_owned()),
            (
                "page_comment_id",
                thread["comments"][0]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            ),
            ("view_id", view.clone()),
            ("view_ids", view),
            ("invitation_id", invitation),
            ("membership_id", membership),
            ("user_id", member_id.to_string()),
            ("conversation_id", general),
            ("message_id", message.clone()),
            ("root_id", message),
        ]),
    };
    // A guest sees the avatar of someone in its channel, not of someone it shares nothing with.
    for user in [setup.user_id, member_id] {
        sqlx::query(
            "INSERT INTO user_avatars (user_id, mime_type, bytes, updated_at) VALUES ($1, 'image/png', $2, 1)",
        )
        .bind(user.to_string())
        .bind(vec![1_u8; 8])
        .execute(server.database.pool())
        .await
        .unwrap();
    }
    let avatar = |user: Id| format!("/api/v1/users/{user}/avatar");
    assert_eq!(
        server
            .call(&guest, "GET", &avatar(setup.user_id), None)
            .await
            .0,
        200
    );
    assert_eq!(
        server.call(&guest, "GET", &avatar(member_id), None).await.0,
        404
    );
    assert_eq!(
        server.call(&admin, "GET", &avatar(member_id), None).await.0,
        200
    );
    let document: Value =
        serde_json::from_str(&orbit_server::openapi::openapi_json().unwrap()).unwrap();
    let operations = operations(&document, &records);

    let mut reached = Vec::new();
    for operation in &operations {
        let (status, body) = server
            .call(
                &guest,
                &operation.method,
                &operation.path,
                operation.body.as_ref(),
            )
            .await;
        if status < 400 && !guest_may_use(&operation.id) {
            reached.push(format!(
                "{} {} ({}): {status} {body}",
                operation.method, operation.template, operation.id
            ));
        }
    }

    // The same calls as an admin show that each one reaches its access check: success, or a
    // refusal by a later rule (403, 409). A missing record or a refused body proves nothing.
    let mut unproven = Vec::new();
    for operation in &operations {
        let (status, body) = server
            .call(
                &admin,
                &operation.method,
                &operation.path,
                operation.body.as_ref(),
            )
            .await;
        let checked = status < 400 || status == 403 || status == 409;
        if checked == UNCHECKED.contains(&operation.id.as_str()) && !guest_may_use(&operation.id) {
            unproven.push(format!(
                "{} {} ({}): {status} {}",
                operation.method,
                operation.template,
                operation.id,
                body["code"].as_str().unwrap_or_default()
            ));
        }
    }
    assert!(
        reached.is_empty(),
        "a guest reached {} operations:\n{}",
        reached.len(),
        reached.join("\n")
    );
    assert!(
        unproven.is_empty(),
        "an admin must reach the access check of every operation outside `UNCHECKED`, and of \
         none inside it; fix the fixture or the list:\n{}",
        unproven.join("\n")
    );

    // The guest still sees its workspace, as a guest with no permission.
    let own = server.ok(&guest, "GET", &base, Value::Null).await;
    assert_eq!(own["role"], "guest");
    assert_eq!(own["permissions"], json!([]));
    let listed = server
        .ok(&guest, "GET", "/api/v1/workspaces", Value::Null)
        .await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|listed| listed["id"] == workspace.as_str())
    );
    // And its private channel.
    let (status, _) = server
        .call(
            &guest,
            "GET",
            &format!("{base}/chat/conversations/{private}/messages"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    server.stop().await;
}
