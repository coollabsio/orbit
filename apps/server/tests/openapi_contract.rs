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
    assert_eq!(document["paths"].as_object().unwrap().len(), 56);
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
    assert_eq!(operation_count, 76);
    for path in [
        "/api/v1/setup/status",
        "/api/v1/auth/me",
        "/api/v1/admin/backups",
        "/api/v1/workspaces",
        "/api/v1/workspaces/invitations/preview",
        "/api/v1/workspaces/{workspace_id}/tasks",
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/activity",
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/duplicate-of",
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments",
        "/api/v1/workspaces/{workspace_id}/notifications",
    ] {
        assert!(document["paths"].get(path).is_some(), "missing {path}");
    }
    assert_eq!(
        document["paths"]["/api/v1/workspaces/{workspace_id}/tasks"]["get"]["responses"]["default"]
            ["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/TaskProblem"
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
            "label_id",
            "identifier",
            "parent_id",
            "nesting",
            "priority",
            "search",
            "view",
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
            "Page_TaskRecord",
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
