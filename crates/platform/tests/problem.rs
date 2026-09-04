use std::str::FromStr;

use orbit_platform::{Id, Problem, TimestampMillis};
use serde_json::json;

#[test]
fn problem_has_stable_machine_code() {
    let problem = Problem::validation("req-1", "/api/v1/tasks", [("title", "Title is required.")]);

    assert_eq!(problem.code, "validation_failed");
    assert_eq!(problem.status, 422);
}

#[test]
fn validation_problem_serializes_rfc_9457_fields_and_field_errors() {
    let problem = Problem::validation(
        "req-1",
        "/api/v1/tasks",
        [
            ("title", "Title is required."),
            ("title", "Title is too short."),
        ],
    );

    let value = serde_json::to_value(problem).unwrap();
    assert!(value["type"].is_string());
    assert_eq!(value["title"], "Validation failed");
    assert_eq!(value["status"], 422);
    assert_eq!(value["code"], "validation_failed");
    assert_eq!(value["detail"], "One or more fields are invalid.");
    assert_eq!(value["instance"], "/api/v1/tasks");
    assert_eq!(value["request_id"], "req-1");
    assert_eq!(
        value["errors"]["title"],
        json!(["Title is required.", "Title is too short."])
    );
    assert!(value.get("conflict").is_none());
}

#[test]
fn internal_problem_exposes_only_a_fixed_safe_detail() {
    let problem = Problem::internal("req-2", "/api/v1/tasks/task-1");

    assert_eq!(problem.status, 500);
    assert_eq!(problem.code, "internal_error");
    assert_eq!(
        problem.detail,
        "An unexpected error occurred. Use the request ID when contacting support."
    );
}

#[test]
fn conflict_problem_includes_safe_refresh_metadata() {
    let problem = Problem::conflict(
        "req-3",
        "/api/v1/tasks/task-1",
        "The task changed since it was loaded.",
        7,
        Some("Reload the task and retry your changes.".to_owned()),
    );

    let value = serde_json::to_value(problem).unwrap();
    assert_eq!(value["status"], 409);
    assert_eq!(value["conflict"]["current_version"], 7);
    assert_eq!(
        value["conflict"]["refresh"],
        "Reload the task and retry your changes."
    );
    assert!(value.get("errors").is_none());
}

#[test]
fn uuid_v7_has_a_canonical_string_round_trip() {
    let id = Id::new_v7();
    let text = id.to_string();

    assert_eq!(text.len(), 36);
    assert_eq!(text.as_bytes()[14], b'7');
    assert_eq!(Id::from_str(&text).unwrap(), id);
    assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{text}\""));
}

#[test]
fn id_rejects_non_v7_and_noncanonical_uuid_strings() {
    assert!(Id::from_str("550e8400-e29b-41d4-a716-446655440000").is_err());
    assert!(Id::from_str("00000000-0000-7000-0000-000000000000").is_err());
    assert!(Id::from_str("00000000-0000-7000-c000-000000000000").is_err());

    let id = Id::new_v7().to_string();
    assert!(Id::from_str(&id.replace('-', "")).is_err());
}

#[test]
fn problem_type_is_a_stable_documentation_url() {
    let problem = Problem::internal("req-4", "/api/v1/tasks");

    assert!(problem.type_uri.starts_with("https://"));
    assert!(problem.type_uri.contains("/problems/internal-error"));
}

#[test]
fn timestamp_serializes_as_rfc_3339_utc_and_round_trips_milliseconds() {
    let timestamp = TimestampMillis::from_millis(1_767_225_600_123);

    let json = serde_json::to_string(&timestamp).unwrap();
    assert_eq!(json, "\"2026-01-01T00:00:00.123Z\"");
    assert_eq!(
        serde_json::from_str::<TimestampMillis>(&json).unwrap(),
        timestamp
    );
    assert_eq!(timestamp.as_millis(), 1_767_225_600_123);
}
