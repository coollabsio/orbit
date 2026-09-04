use orbit_server::openapi::{CONTRACT_ID, openapi_json};

#[test]
fn openapi_generation_is_byte_stable_and_covers_public_routes() {
    let first = openapi_json().expect("OpenAPI serializes");
    let second = openapi_json().expect("OpenAPI serializes twice");

    assert_eq!(first, second);
    assert!(first.ends_with('\n'));

    let document: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(document["info"]["version"], CONTRACT_ID);
    assert_eq!(document["paths"].as_object().unwrap().len(), 47);
    for path in [
        "/api/v1/setup/status",
        "/api/v1/auth/me",
        "/api/v1/workspaces",
        "/api/v1/workspaces/{workspace_id}/tasks",
        "/api/v1/workspaces/{workspace_id}/tasks/{task_id}/attachments",
    ] {
        assert!(document["paths"].get(path).is_some(), "missing {path}");
    }
    assert_eq!(
        document["paths"]["/api/v1/workspaces/{workspace_id}/tasks"]["get"]["responses"]["default"]
            ["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/Problem"
    );
}
