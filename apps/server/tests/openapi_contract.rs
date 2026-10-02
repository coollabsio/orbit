use orbit_server::openapi::{CONTRACT_ID, openapi_json};
use serde_json::Value;

fn operation<'a>(document: &'a Value, path: &str, method: &str) -> &'a Value {
    &document["paths"][path][method]
}

fn parameter_names(operation: &Value, location: &str) -> Vec<String> {
    operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|parameter| parameter["in"] == location)
        .map(|parameter| parameter["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn openapi_generation_is_byte_stable_and_covers_public_routes() {
    let first = openapi_json().expect("OpenAPI serializes");
    let second = openapi_json().expect("OpenAPI serializes twice");

    assert_eq!(first, second);
    assert!(first.ends_with('\n'));

    let document: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(document["info"]["version"], CONTRACT_ID);
    assert_eq!(document["paths"].as_object().unwrap().len(), 139);
    let operation_count: usize = document["paths"]
        .as_object()
        .unwrap()
        .values()
        .map(|path| {
            path.as_object()
                .unwrap()
                .keys()
                .filter(|method| {
                    ["get", "post", "patch", "delete", "put"].contains(&method.as_str())
                })
                .count()
        })
        .sum();
    assert_eq!(operation_count, 184);
    for path in [
        "/api/v1/setup/status",
        "/api/v1/auth/me",
        "/api/v1/admin/backups",
        "/api/v1/workspaces",
        "/api/v1/workspaces/invitations/preview",
        "/api/v1/workspaces/{workspace_id}/tasks",
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/activity",
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments",
        "/api/v1/workspaces/{workspace_id}/notifications",
        "/api/v1/workspaces/{workspace_id}/api-tokens",
        "/api/v1/workspaces/{workspace_id}/api-tokens/{token_id}",
        "/api/v1/integrations/discord/events",
        "/api/v1/integrations/github/manifest/callback",
        "/api/v1/workspaces/{workspace_id}/github",
        "/api/v1/workspaces/{workspace_id}/github/manifest",
        "/api/v1/workspaces/{workspace_id}/projects/{project_id}/github",
        "/api/v1/workspaces/{workspace_id}/pages",
        "/api/v1/workspaces/{workspace_id}/pages/trash",
        "/api/v1/workspaces/{workspace_id}/pages/search",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/move",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/restore",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/favorite",
        "/api/v1/workspaces/{workspace_id}/pages/favorites",
        "/api/v1/workspaces/{workspace_id}/pages/favorites/{page_id}/move",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/files",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/files/{file_id}",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/export",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions/{version_id}",
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions/{version_id}/restore",
        "/api/v1/workspaces/{workspace_id}/teamspaces",
        "/api/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}",
        "/api/v1/workspaces/{workspace_id}/teamspaces/{teamspace_id}/move",
        "/api/v1/workspaces/{workspace_id}/imports/notion",
        "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}",
        "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}/start",
        "/api/v1/workspaces/{workspace_id}/imports/notion/{import_id}/cancel",
    ] {
        assert!(document["paths"].get(path).is_some(), "missing {path}");
    }
    assert_eq!(
        document["paths"]["/api/v1/workspaces/{workspace_id}/tasks"]["get"]["responses"]["default"]
            ["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/TaskProblem"
    );

    let discord = operation(&document, "/api/v1/integrations/discord/events", "post");
    assert_eq!(discord["security"], serde_json::json!([{"bearerAuth": []}]));
    assert!(
        discord["responses"]["401"]["description"]
            .as_str()
            .unwrap()
            .contains("invalid_api_token")
    );

    let preview = operation(&document, "/api/v1/workspaces/invitations/preview", "post");
    assert_eq!(preview["operationId"], "preview_invitation");
    assert_eq!(preview["security"], serde_json::json!([{}]));
    assert_eq!(
        preview["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/InvitationPreview"
    );
    assert!(
        preview["responses"]["404"]["description"]
            .as_str()
            .unwrap()
            .contains("invitation_not_found")
    );
    assert!(preview["responses"].get("401").is_none());

    let tasks = operation(&document, "/api/v1/workspaces/{workspace_id}/tasks", "get");
    assert_eq!(
        parameter_names(tasks, "query"),
        [
            "project_id",
            "status_id",
            "assignee_id",
            "unassigned",
            "label_id",
            "priority",
            "search",
            "view",
            "parent_task_id",
            "sort",
            "order",
            "cursor",
            "limit",
        ]
    );
    let delete_task = operation(
        &document,
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}",
        "delete",
    );
    assert_eq!(parameter_names(delete_task, "query"), ["expected_version"]);
    assert!(
        delete_task["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|parameter| {
                parameter["name"] == "expected_version" && parameter["required"] == true
            })
    );

    for (path, method, expected) in [
        (
            "/api/v1/workspaces/{workspace_id}/projects",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}",
            "delete",
            vec!["expected_version"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/projects/trash",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}",
            "delete",
            vec!["expected_version"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/labels",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/labels/{label_id}",
            "delete",
            vec!["expected_version"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/tasks/trash",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/members",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/members/{membership_id}",
            "delete",
            vec!["expected_version"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/invitations",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}",
            "delete",
            vec!["expected_version"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/audit",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/admin/audit",
            "get",
            vec!["workspace_id", "action", "cursor", "limit"],
        ),
        (
            "/api/v1/admin/audit/export",
            "get",
            vec!["workspace_id", "action", "cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}",
            "delete",
            vec!["expected_version"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments",
            "get",
            vec!["cursor", "limit"],
        ),
        (
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/comments/{comment_id}/attachments",
            "get",
            vec!["cursor", "limit"],
        ),
    ] {
        assert_eq!(
            parameter_names(operation(&document, path, method), "query"),
            expected
        );
    }

    let upload = operation(
        &document,
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments",
        "post",
    );
    assert!(
        operation(
            &document,
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments",
            "get",
        )["responses"]["400"]["description"]
            .as_str()
            .unwrap()
            .contains("invalid_request")
    );
    assert_eq!(
        upload["requestBody"]["content"]["multipart/form-data"]["schema"]["$ref"],
        "#/components/schemas/AttachmentUploadBody"
    );
    assert_eq!(
        upload["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AttachmentRecord"
    );
    let download = operation(
        &document,
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments/{attachment_id}/download",
        "get",
    );
    assert_eq!(
        download["responses"]["200"]["content"]["application/octet-stream"]["schema"]["format"],
        Value::Null
    );
    assert_eq!(
        download["responses"]["200"]["content"]["application/octet-stream"]["schema"]["$ref"],
        "#/components/schemas/AttachmentDownload"
    );
    assert_eq!(
        document["components"]["schemas"]["AttachmentDownload"]["format"],
        "binary"
    );
    let download_media_types: Vec<_> = download["responses"]["200"]["content"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        download_media_types,
        [
            "application/octet-stream",
            "application/pdf",
            "image/avif",
            "image/gif",
            "image/jpeg",
            "image/png",
            "image/svg+xml",
            "image/webp",
            "text/html",
        ]
    );

    assert_eq!(
        operation(&document, "/api/v1/setup/complete", "post")["responses"]["201"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/SetupResponse"
    );
    assert_eq!(
        operation(&document, "/api/v1/auth/recovery/request", "post")["responses"]["202"]["content"]
            ["application/json"]["schema"]["$ref"],
        "#/components/schemas/RecoveryRequestResponse"
    );
    assert_eq!(
        operation(&document, "/api/v1/auth/sessions", "get")["responses"]["200"]["content"]["application/json"]
            ["schema"]["type"],
        "array"
    );
    for (path, method, schema) in [
        (
            "/api/v1/workspaces/{workspace_id}/projects/{project_id}/statuses",
            "get",
            "Page_StatusRecord",
        ),
        (
            "/api/v1/workspaces/{workspace_id}/labels",
            "get",
            "Page_LabelRecord",
        ),
        (
            "/api/v1/workspaces/{workspace_id}/tasks/bulk",
            "post",
            "TaskBulkResponse",
        ),
    ] {
        assert_eq!(
            operation(&document, path, method)["responses"]["200"]["content"]["application/json"]["schema"]
                ["$ref"],
            format!("#/components/schemas/{schema}")
        );
    }
    let login_unauthorized =
        &operation(&document, "/api/v1/auth/login", "post")["responses"]["401"];
    assert!(
        login_unauthorized["description"]
            .as_str()
            .unwrap()
            .contains("invalid_credentials")
    );
    assert_eq!(
        login_unauthorized["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/AuthProblem"
    );
    for field in ["current_version", "current", "refresh", "field"] {
        assert!(
            document["components"]["schemas"]["TaskConflict"]["properties"]
                .get(field)
                .is_some()
        );
    }
    assert!(
        tasks["responses"]["404"]["description"]
            .as_str()
            .unwrap()
            .contains("task_resource_not_found")
    );
    assert_eq!(
        tasks["responses"]["409"]["description"],
        "contract_mismatch"
    );
    assert!(
        operation(
            &document,
            "/api/v1/workspaces/{workspace_id}/tasks/{task_id}",
            "patch",
        )["responses"]["409"]["description"]
            .as_str()
            .unwrap()
            .contains("task_conflict")
    );
    assert!(
        operation(&document, "/api/v1/setup/complete", "post")["responses"]["409"]["description"]
            .as_str()
            .unwrap()
            .contains("setup_unavailable")
    );
    assert!(
        operation(&document, "/api/v1/auth/recovery/complete", "post")["responses"]["400"]
            ["description"]
            .as_str()
            .unwrap()
            .contains("invalid_recovery_token")
    );
    assert_eq!(
        operation(&document, "/api/v1/setup/status", "get")["responses"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["200", "400", "409", "413", "500", "default"]
    );
    assert!(!operation(&document, "/api/v1/admin/audit/export", "get")["responses"]
        ["403"]["description"]
        .as_str()
        .unwrap()
        .contains("origin_forbidden"));
    for path in ["/api/v1/admin/audit", "/api/v1/admin/audit/export"] {
        assert!(
            operation(&document, path, "get")["responses"]
                .get("404")
                .is_none(),
            "global audit does not look up a workspace resource"
        );
    }
    let accept = operation(&document, "/api/v1/workspaces/invitations/accept", "post");
    for status in ["200", "201"] {
        assert_eq!(
            accept["responses"][status]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/AcceptanceRecord"
        );
    }
    assert!(
        accept["responses"]["403"]["description"]
            .as_str()
            .unwrap()
            .contains("invitation_email_mismatch")
    );
    assert!(
        !accept["responses"]["403"]["description"]
            .as_str()
            .unwrap()
            .contains("workspace_action_forbidden")
    );
    let membership_change = operation(
        &document,
        "/api/v1/workspaces/{workspace_id}/members/{membership_id}",
        "patch",
    );
    assert!(
        membership_change["responses"]["409"]["description"]
            .as_str()
            .unwrap()
            .contains("ownership_transfer_required")
    );
    assert!(
        !membership_change["responses"]["403"]["description"]
            .as_str()
            .unwrap()
            .contains("ownership_transfer_required")
    );

    let schemes = &document["components"]["securitySchemes"]["cookieAuth"];
    assert_eq!(schemes["in"], "cookie");
    assert_eq!(schemes["name"], "__Host-orbit_session");
    assert_eq!(tasks["security"][0]["cookieAuth"], serde_json::json!([]));
    assert_eq!(
        operation(&document, "/api/v1/auth/login", "post")["security"][0],
        serde_json::json!({})
    );
    for operation in [tasks, upload] {
        assert!(
            operation["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|parameter| {
                    parameter["name"] == "X-Orbit-Contract" && parameter["in"] == "header"
                })
        );
        let mismatch = &operation["responses"]["409"];
        assert!(
            mismatch["description"]
                .as_str()
                .unwrap()
                .contains("contract_mismatch")
        );
        assert!(
            mismatch["content"]
                .get("application/problem+json")
                .is_some()
        );
    }

    let description =
        &document["components"]["schemas"]["StatusUpdateBody"]["properties"]["description"];
    assert_eq!(description["type"], "string");
    assert!(description.get("$ref").is_none());
    assert!(
        !document["components"]["schemas"]["StatusUpdateBody"]["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("description"))
    );

    for path in document["paths"].as_object().unwrap().values() {
        for (method, operation) in path.as_object().unwrap() {
            if !["get", "post", "patch", "delete", "put"].contains(&method.as_str()) {
                continue;
            }
            assert!(parameter_names(operation, "header").contains(&"X-Orbit-Contract".to_owned()));
            assert!(operation["security"].is_array());
            assert!(
                operation["responses"]["default"]["content"]
                    .get("application/problem+json")
                    .is_some()
            );
        }
    }
}

#[test]
fn task_updates_document_duplicate_of_id() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    for schema in ["TaskUpdateBody", "BulkItem"] {
        let body = &document["components"]["schemas"][schema];
        assert!(
            body["properties"]["duplicate_of_id"].is_object(),
            "{schema} lacks duplicate_of_id"
        );
        assert!(
            !body["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("duplicate_of_id"))
        );
    }
}

#[test]
fn task_records_document_duplicate_and_blocked_fields() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let record = &document["components"]["schemas"]["TaskRecord"];
    let required = record["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("duplicate_of")));
    assert!(required.contains(&serde_json::json!("blocked")));
    assert_eq!(record["properties"]["blocked"]["type"], "boolean");
    assert_eq!(
        document["components"]["schemas"]["TaskRef"]["required"],
        serde_json::json!(["id", "project_id", "project_key", "title"])
    );
}

#[test]
fn task_relation_routes_are_documented() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/relations";
    assert_eq!(
        operation(&document, path, "get")["operationId"],
        "list_task_relations"
    );
    assert_eq!(
        operation(&document, path, "post")["operationId"],
        "create_task_relation"
    );
    assert_eq!(
        operation(&document, &format!("{path}/{{relation_id}}"), "delete")["operationId"],
        "delete_task_relation"
    );
    assert!(
        operation(&document, path, "post")["responses"]["409"]["description"]
            .as_str()
            .unwrap()
            .contains("task_conflict")
    );
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["NewTaskRelationType"]["enum"],
        serde_json::json!(["blocks", "blocked_by", "related"])
    );
    assert_eq!(
        schemas["TaskRelationType"]["enum"],
        serde_json::json!(["blocks", "related", "duplicate"])
    );
    assert_eq!(
        schemas["TaskRelationDirection"]["enum"],
        serde_json::json!(["outgoing", "incoming"])
    );
    assert_eq!(
        schemas["TaskRelationRecord"]["properties"]["type"]["$ref"],
        "#/components/schemas/TaskRelationType"
    );
    assert_eq!(
        schemas["RelatedTask"]["required"],
        serde_json::json!(["id", "project_id", "title", "status_id"])
    );
}

#[test]
fn teamspace_routes_and_page_space_fields_are_documented() {
    let document: serde_json::Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let path = "/api/v1/workspaces/{workspace_id}/teamspaces";
    let item = format!("{path}/{{teamspace_id}}");
    assert_eq!(
        operation(&document, path, "get")["operationId"],
        "list_teamspaces"
    );
    assert_eq!(
        operation(&document, path, "post")["operationId"],
        "create_teamspace"
    );
    assert_eq!(
        operation(&document, &item, "patch")["operationId"],
        "update_teamspace"
    );
    let delete = operation(&document, &item, "delete");
    assert_eq!(delete["operationId"], "delete_teamspace");
    for (status, code) in [
        ("403", "workspace_action_forbidden"),
        ("409", "teamspace_not_empty"),
        ("422", "last_teamspace"),
    ] {
        assert!(
            delete["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{status} {code}"
        );
    }
    let schemas = &document["components"]["schemas"];
    assert!(
        schemas["Teamspace"]["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("is_default"))
    );
    for schema in ["Page", "PageSummary", "TrashedPage", "PageSearchResult"] {
        let required = schemas[schema]["required"].as_array().unwrap();
        for field in ["teamspace_id", "private"] {
            assert!(
                required.contains(&serde_json::json!(field)),
                "{schema}.{field}"
            );
        }
    }
}

#[test]
fn page_favorite_routes_are_documented() {
    let document: serde_json::Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let pages = "/api/v1/workspaces/{workspace_id}/pages";
    let list = operation(&document, &format!("{pages}/favorites"), "get");
    assert_eq!(list["operationId"], "list_page_favorites");
    assert_eq!(
        list["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PageFavoriteList"
    );
    let favorite = format!("{pages}/{{page_id}}/favorite");
    let add = operation(&document, &favorite, "put");
    assert_eq!(add["operationId"], "add_page_favorite");
    assert_eq!(
        add["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PageFavorite"
    );
    let remove = operation(&document, &favorite, "delete");
    assert_eq!(remove["operationId"], "remove_page_favorite");
    assert!(remove["responses"].get("204").is_some());
    let reorder = operation(
        &document,
        &format!("{pages}/favorites/{{page_id}}/move"),
        "post",
    );
    assert_eq!(reorder["operationId"], "move_page_favorite");
    for (status, code) in [
        ("404", "page_not_found"),
        ("422", "validation_failed"),
        ("400", "invalid_request"),
    ] {
        assert!(
            reorder["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{status} {code}"
        );
    }
    for operation_json in [add, remove] {
        assert!(
            operation_json["responses"]["404"]["description"]
                .as_str()
                .unwrap()
                .contains("page_not_found")
        );
    }
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["PageFavorite"]["required"],
        serde_json::json!(["page_id", "position"])
    );
}

#[test]
fn page_version_routes_are_documented() {
    let document: serde_json::Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let versions = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/versions";
    let list = operation(&document, versions, "get");
    assert_eq!(list["operationId"], "list_page_versions");
    assert_eq!(
        list["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PageVersionList"
    );
    assert_eq!(parameter_names(list, "query"), ["cursor", "limit"]);
    let get = operation(&document, &format!("{versions}/{{version_id}}"), "get");
    assert_eq!(get["operationId"], "get_page_version");
    assert_eq!(
        get["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PageVersion"
    );
    let restore = operation(
        &document,
        &format!("{versions}/{{version_id}}/restore"),
        "post",
    );
    assert_eq!(restore["operationId"], "restore_page_version");
    assert_eq!(
        restore["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Page"
    );
    for (operation_json, status, code) in [
        (list, "404", "page_not_found"),
        (list, "400", "invalid_cursor"),
        (list, "422", "validation_failed"),
        (get, "404", "page_version_not_found"),
        (restore, "404", "page_not_found"),
        (restore, "404", "page_version_not_found"),
        (restore, "409", "conflict"),
    ] {
        assert!(
            operation_json["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{} {status} {code}",
            operation_json["operationId"]
        );
    }
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["PageVersionKind"]["enum"],
        serde_json::json!(["auto", "restore", "import"])
    );
}

#[test]
fn page_file_routes_are_documented() {
    let document: serde_json::Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let files = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/files";
    let upload = operation(&document, files, "post");
    assert_eq!(upload["operationId"], "upload_page_file");
    assert_eq!(
        upload["requestBody"]["content"]["multipart/form-data"]["schema"]["$ref"],
        "#/components/schemas/PageFileUploadBody"
    );
    assert_eq!(
        upload["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PageFile"
    );
    for (status, code) in [
        ("400", "invalid_multipart"),
        ("404", "page_file_not_found"),
        ("413", "upload_too_large"),
        ("422", "validation_failed"),
        ("401", "authentication_required"),
        ("403", "origin_forbidden"),
    ] {
        assert!(
            upload["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{status} {code}"
        );
    }
    let download = operation(&document, &format!("{files}/{{file_id}}"), "get");
    assert_eq!(download["operationId"], "download_page_file");
    let content = download["responses"]["200"]["content"].as_object().unwrap();
    assert_eq!(
        content["image/png"]["schema"]["$ref"],
        "#/components/schemas/PageFileDownload"
    );
    assert!(content.contains_key("image/avif"));
    assert_eq!(
        document["components"]["schemas"]["PageFile"]["required"],
        serde_json::json!([
            "id",
            "page_id",
            "url",
            "file_name",
            "mime_type",
            "size_bytes",
            "created_at"
        ])
    );
}

#[test]
fn notion_import_routes_are_documented() {
    let document: serde_json::Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let base = "/api/v1/workspaces/{workspace_id}/imports/notion";
    let create = operation(&document, base, "post");
    assert_eq!(create["operationId"], "create_notion_import");
    assert_eq!(
        create["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/NotionImport"
    );
    for (status, code) in [
        ("409", "app_key_missing"),
        ("409", "import_in_progress"),
        ("422", "notion_token_invalid"),
        ("502", "notion_unavailable"),
        ("404", "notion_import_not_found"),
        ("403", "origin_forbidden"),
    ] {
        assert!(
            create["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{status} {code}"
        );
    }
    assert_eq!(
        operation(&document, base, "get")["operationId"],
        "list_notion_imports"
    );
    let detail = format!("{base}/{{import_id}}");
    assert_eq!(
        operation(&document, &detail, "get")["operationId"],
        "get_notion_import"
    );
    let start = operation(&document, &format!("{detail}/start"), "post");
    assert_eq!(start["operationId"], "start_notion_import");
    assert!(
        start["responses"]["409"]["description"]
            .as_str()
            .unwrap()
            .contains("notion_import_state_conflict")
    );
    assert_eq!(
        operation(&document, &format!("{detail}/cancel"), "post")["operationId"],
        "cancel_notion_import"
    );
    let schema = &document["components"]["schemas"]["NotionImport"];
    let properties = schema["properties"].as_object().unwrap();
    assert!(!properties.contains_key("token"));
    assert!(!properties.contains_key("token_ciphertext"));
    for field in [
        "status",
        "progress",
        "tree",
        "report",
        "destination",
        "root_page_ids",
    ] {
        assert!(properties.contains_key(field), "{field}");
    }
}

#[test]
fn page_purge_duplicate_and_search_highlights_are_documented() {
    let document: serde_json::Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let pages = "/api/v1/workspaces/{workspace_id}/pages";
    let purge = operation(
        &document,
        &format!("{pages}/{{page_id}}/permanent"),
        "delete",
    );
    assert_eq!(purge["operationId"], "purge_page");
    assert_eq!(parameter_names(purge, "query"), ["expected_version"]);
    for (status, code) in [
        ("403", "workspace_action_forbidden"),
        ("404", "page_not_found"),
        ("409", "page_not_trashed"),
        ("409", "conflict"),
    ] {
        assert!(
            purge["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{status} {code}"
        );
    }
    let empty = operation(&document, &format!("{pages}/trash/empty"), "post");
    assert_eq!(empty["operationId"], "empty_page_trash");
    assert_eq!(
        empty["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PageTrashEmptied"
    );
    let duplicate = operation(&document, &format!("{pages}/{{page_id}}/duplicate"), "post");
    assert_eq!(duplicate["operationId"], "duplicate_page");
    assert_eq!(
        duplicate["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Page"
    );
    let schemas = &document["components"]["schemas"];
    let required = schemas["PageSearchResult"]["required"].as_array().unwrap();
    for field in ["snippet", "snippet_highlights", "title_highlights"] {
        assert!(required.contains(&serde_json::json!(field)), "{field}");
    }
    assert_eq!(
        schemas["TextRange"]["required"],
        serde_json::json!(["start", "end"])
    );
}

#[test]
fn page_comment_routes_are_documented() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let base = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/threads";
    let list = operation(&document, base, "get");
    assert_eq!(list["operationId"], "list_page_threads");
    let create = operation(&document, base, "post");
    assert_eq!(create["operationId"], "create_page_thread");
    assert!(create["responses"]["201"].is_object());
    assert!(create["responses"]["422"].is_object());
    let update = operation(
        &document,
        &format!("{base}/{{thread_id}}/comments/{{comment_id}}"),
        "patch",
    );
    assert_eq!(update["operationId"], "update_page_comment");
    assert!(update["responses"]["403"].is_object());
    for (path, method, id) in [
        (
            format!("{base}/{{thread_id}}"),
            "delete",
            "delete_page_thread",
        ),
        (
            format!("{base}/{{thread_id}}/comments"),
            "post",
            "create_page_comment",
        ),
        (
            format!("{base}/{{thread_id}}/comments/{{comment_id}}"),
            "delete",
            "delete_page_comment",
        ),
        (
            format!("{base}/{{thread_id}}/resolve"),
            "post",
            "resolve_page_thread",
        ),
        (
            format!("{base}/{{thread_id}}/reopen"),
            "post",
            "reopen_page_thread",
        ),
    ] {
        let found = operation(&document, &path, method);
        assert_eq!(found["operationId"], id);
        assert!(found["responses"]["404"].is_object(), "{id}");
    }
    let notification = &document["components"]["schemas"]["NotificationRecord"]["properties"];
    for field in ["page_id", "page_thread_id", "page_comment_id"] {
        assert!(notification[field].is_object(), "{field}");
    }
}

#[test]
fn page_export_route_is_documented() {
    let document: serde_json::Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let export = operation(
        &document,
        "/api/v1/workspaces/{workspace_id}/pages/{page_id}/export",
        "get",
    );
    assert_eq!(export["operationId"], "export_page");
    assert_eq!(parameter_names(export, "query"), vec!["format", "children"]);
    assert_eq!(
        export["responses"]["200"]["content"]["application/zip"]["schema"]["$ref"],
        "#/components/schemas/PageExportArchive"
    );
    for (status, code) in [
        ("400", "invalid_request"),
        ("404", "page_not_found"),
        ("413", "export_too_many_pages"),
        ("413", "export_too_large"),
        ("401", "authentication_required"),
    ] {
        assert!(
            export["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{status} {code}"
        );
    }
}

#[test]
fn task_query_route_is_documented() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let query = operation(
        &document,
        "/api/v1/workspaces/{workspace_id}/tasks/query",
        "post",
    );
    assert_eq!(query["operationId"], "query_tasks");
    assert_eq!(
        query["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/TaskQueryBody"
    );
    assert_eq!(
        query["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Page_TaskRecord"
    );
    for (status, code) in [
        ("400", "invalid_cursor"),
        ("400", "invalid_request"),
        ("404", "task_resource_not_found"),
        ("422", "invalid_filter"),
    ] {
        assert!(
            query["responses"][status]["description"]
                .as_str()
                .unwrap()
                .contains(code),
            "{status} lacks {code}"
        );
    }
    let schemas = &document["components"]["schemas"];
    let body = &schemas["TaskQueryBody"];
    assert_eq!(
        body["properties"]["filter"]["$ref"],
        "#/components/schemas/FilterGroup"
    );
    assert_eq!(
        body["required"],
        serde_json::json!(["filter", "order_by", "order_direction", "show_completed"])
    );
    assert_eq!(
        schemas["FilterGroup"]["properties"]["children"]["items"]["$ref"],
        "#/components/schemas/FilterNode"
    );
    assert_eq!(schemas["FilterNode"]["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(
        schemas["FilterOperator"]["enum"],
        serde_json::json!([
            "is",
            "is_not",
            "is_empty",
            "is_not_empty",
            "includes_any",
            "includes_all",
            "excludes",
            "before",
            "after",
            "between",
            "contains"
        ])
    );
    assert_eq!(
        schemas["ShowCompleted"]["enum"],
        serde_json::json!(["all", "past_week", "past_month", "none"])
    );
    let problem = &schemas["TaskProblem"];
    assert!(problem["properties"]["path"].is_object());
    assert!(
        !problem["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("path"))
    );
}

fn codes(operation: &Value, status: &str) -> Vec<String> {
    operation["responses"][status]["description"]
        .as_str()
        .unwrap_or_default()
        .split(", ")
        .map(str::to_owned)
        .collect()
}

#[test]
fn saved_view_routes_are_documented() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let views = "/api/v1/workspaces/{workspace_id}/views";
    let view = "/api/v1/workspaces/{workspace_id}/views/{view_id}";
    for (path, method, id) in [
        (views, "get", "list_views"),
        (views, "post", "create_view"),
        (view, "get", "get_view"),
        (view, "patch", "update_view"),
        (view, "delete", "delete_view"),
    ] {
        let operation = operation(&document, path, method);
        assert_eq!(operation["operationId"], id);
        assert_eq!(
            operation["responses"]["default"]["content"]["application/problem+json"]["schema"]["$ref"],
            "#/components/schemas/TaskProblem"
        );
        assert!(codes(operation, "404").contains(&"task_resource_not_found".to_owned()));
    }
    let create = operation(&document, views, "post");
    assert_eq!(
        create["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SavedViewRecord"
    );
    assert!(codes(create, "422").contains(&"invalid_filter".to_owned()));
    assert!(codes(create, "422").contains(&"validation_failed".to_owned()));
    assert!(codes(create, "400").contains(&"invalid_request".to_owned()));
    let update = operation(&document, view, "patch");
    assert!(codes(update, "409").contains(&"conflict".to_owned()));
    assert!(codes(update, "403").contains(&"task_action_forbidden".to_owned()));
    assert!(
        codes(operation(&document, view, "delete"), "403")
            .contains(&"task_action_forbidden".to_owned())
    );
    assert_eq!(
        operation(&document, views, "get")["responses"]["200"]["content"]["application/json"]["schema"]
            ["items"]["$ref"],
        "#/components/schemas/SavedViewRecord"
    );

    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Visibility"]["enum"],
        serde_json::json!(["personal", "workspace"])
    );
    let required = schemas["SavedViewRecord"]["required"].as_array().unwrap();
    for field in [
        "id",
        "owner",
        "name",
        "visibility",
        "state",
        "state_error",
        "version",
        "is_favorite",
        "favorite_position",
        "can_edit",
        "icon",
        "color",
    ] {
        assert!(required.contains(&serde_json::json!(field)), "{field}");
    }
    assert_eq!(
        schemas["ViewUpdateBody"]["required"],
        serde_json::json!(["expected_version"])
    );
    let create_required = schemas["ViewCreateBody"]["required"].as_array().unwrap();
    assert!(!create_required.contains(&serde_json::json!("description")));
    assert!(create_required.contains(&serde_json::json!("state")));
}

#[test]
fn view_favorite_routes_are_documented() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let favorite = "/api/v1/workspaces/{workspace_id}/views/{view_id}/favorite";
    let order = "/api/v1/workspaces/{workspace_id}/view-favorites/order";
    for (path, method, id) in [
        (favorite, "put", "favorite_view"),
        (favorite, "delete", "unfavorite_view"),
        (order, "put", "reorder_view_favorites"),
    ] {
        let operation = operation(&document, path, method);
        assert_eq!(operation["operationId"], id);
        assert!(operation["responses"].get("204").is_some());
        assert_eq!(
            operation["responses"]["default"]["content"]["application/problem+json"]["schema"]["$ref"],
            "#/components/schemas/TaskProblem"
        );
        assert!(codes(operation, "404").contains(&"task_resource_not_found".to_owned()));
    }
    let reorder = operation(&document, order, "put");
    assert!(codes(reorder, "422").contains(&"validation_failed".to_owned()));
    assert!(codes(reorder, "400").contains(&"invalid_request".to_owned()));
    assert_eq!(
        document["components"]["schemas"]["ViewFavoritesOrderBody"]["required"],
        serde_json::json!(["view_ids"])
    );
}

#[test]
fn view_preference_routes_are_documented() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let path = "/api/v1/workspaces/{workspace_id}/view-preferences/{page_key}";
    let get = operation(&document, path, "get");
    assert_eq!(get["operationId"], "get_view_preference");
    assert_eq!(
        get["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ViewPreferenceRecord"
    );
    assert!(codes(get, "404").contains(&"task_resource_not_found".to_owned()));
    assert!(codes(get, "422").contains(&"validation_failed".to_owned()));
    let put = operation(&document, path, "put");
    assert_eq!(put["operationId"], "put_view_preference");
    assert_eq!(
        put["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ViewPreferenceBody"
    );
    assert!(codes(put, "422").contains(&"invalid_filter".to_owned()));
    assert!(codes(put, "400").contains(&"invalid_request".to_owned()));
    assert_eq!(
        put["responses"]["default"]["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/TaskProblem"
    );
    let required = document["components"]["schemas"]["ViewPreferenceRecord"]["required"]
        .as_array()
        .unwrap();
    for field in ["page_key", "state", "state_error", "updated_at"] {
        assert!(required.contains(&serde_json::json!(field)), "{field}");
    }
}

#[test]
fn sub_issue_contract_is_documented() {
    let document: Value = serde_json::from_str(&openapi_json().unwrap()).unwrap();
    let schemas = &document["components"]["schemas"];
    let record = &schemas["TaskRecord"];
    let required = record["required"].as_array().unwrap();
    for field in [
        "parent_task_id",
        "parent",
        "sub_issue_count",
        "sub_issue_closed_count",
    ] {
        assert!(required.contains(&serde_json::json!(field)), "{field}");
    }
    for field in ["ancestors", "trashed_descendant_count"] {
        assert!(record["properties"][field].is_object(), "{field}");
        assert!(!required.contains(&serde_json::json!(field)), "{field}");
    }
    assert_eq!(
        schemas["TaskRef"]["required"],
        serde_json::json!(["id", "project_id", "project_key", "title"])
    );
    for schema in [
        "CreateTaskBody",
        "TaskUpdateBody",
        "BulkItem",
        "TaskQueryBody",
    ] {
        assert!(
            schemas[schema]["properties"]["parent_task_id"].is_object(),
            "{schema}"
        );
        assert!(
            !schemas[schema]["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("parent_task_id")),
            "{schema}"
        );
    }
    assert!(
        schemas["TaskQueryBody"]["properties"]["sub_issues"]
            .to_string()
            .contains("SubIssuesDisplay")
    );
    assert!(
        schemas["DisplayOptions"]["properties"]["sub_issues"]
            .to_string()
            .contains("SubIssuesDisplay")
    );
    assert_eq!(
        schemas["SubIssuesDisplay"]["enum"],
        serde_json::json!(["nested", "flat", "hidden"])
    );
    let fields = schemas["FilterField"]["enum"].as_array().unwrap();
    assert!(fields.contains(&serde_json::json!("parent")));
    assert!(fields.contains(&serde_json::json!("sub_issues")));
    assert!(
        schemas["TaskProperty"]["enum"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("sub_issue_progress"))
    );

    let list = operation(&document, "/api/v1/workspaces/{workspace_id}/tasks", "get");
    assert!(parameter_names(list, "query").contains(&"parent_task_id".to_owned()));

    let task_path = "/api/v1/workspaces/{workspace_id}/tasks/{task_id}";
    assert_eq!(
        operation(&document, task_path, "patch")["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/TaskUpdateResponse"
    );
    assert_eq!(
        operation(&document, "/api/v1/workspaces/{workspace_id}/tasks", "post")["responses"]["201"]
            ["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/TaskUpdateResponse"
    );
    let update = &schemas["TaskUpdateResponse"]["allOf"];
    assert_eq!(update[0]["$ref"], "#/components/schemas/TaskRecord");
    assert_eq!(update[1]["required"], serde_json::json!(["auto_closed"]));
    assert_eq!(
        update[1]["properties"]["auto_closed"]["items"]["$ref"],
        "#/components/schemas/AutoClosed"
    );
    let bulk_required = schemas["TaskBulkResponse"]["required"].as_array().unwrap();
    assert!(bulk_required.contains(&serde_json::json!("items")));
    assert!(bulk_required.contains(&serde_json::json!("auto_closed")));
    assert_eq!(
        schemas["AutoClosed"]["required"],
        serde_json::json!(["id", "status_id"])
    );

    let project_required = schemas["ProjectRecord"]["required"].as_array().unwrap();
    for field in ["auto_close_parent", "auto_close_sub_issues"] {
        assert!(
            project_required.contains(&serde_json::json!(field)),
            "{field}"
        );
        assert!(
            schemas["ProjectUpdateBody"]["properties"][field].is_object(),
            "{field}"
        );
        assert!(
            !schemas["ProjectUpdateBody"]["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(field)),
            "{field}"
        );
    }

    for (path, method) in [
        ("/api/v1/workspaces/{workspace_id}/tasks", "post"),
        (task_path, "patch"),
        ("/api/v1/workspaces/{workspace_id}/tasks/bulk", "post"),
    ] {
        let codes = codes(operation(&document, path, method), "422");
        for code in ["validation_failed", "parent_cycle", "parent_invalid"] {
            assert!(
                codes.contains(&code.to_owned()),
                "{method} {path} lacks {code}"
            );
        }
    }
}
