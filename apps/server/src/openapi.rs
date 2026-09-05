use utoipa::openapi::path::{ParameterBuilder, ParameterIn};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityRequirement, SecurityScheme};
use utoipa::openapi::{Content, OpenApi as OpenApiDocument, Ref, Required, ResponseBuilder};
use utoipa::{Modify, OpenApi};

pub const CONTRACT_ID: &str = "orbit-api-v1";

#[derive(OpenApi)]
#[openapi(
    info(title = "Orbit API", version = "orbit-api-v1"),
    components(schemas(
        orbit_platform::Problem,
        orbit_platform::ConflictMetadata,
        crate::task_routes::ProblemBody,
        crate::workspace_routes::ProblemBody,
        crate::attachment_routes::AttachmentProblem
    )),
    modifiers(&ProblemDetails),
    paths(
        crate::auth_routes::setup_status,
        crate::auth_routes::setup_complete,
        crate::auth_routes::login,
        crate::auth_routes::logout,
        crate::auth_routes::me,
        crate::auth_routes::recovery_request,
        crate::auth_routes::recovery_complete,
        crate::auth_routes::list_sessions,
        crate::auth_routes::revoke_session,
        crate::workspace_routes::create_workspace,
        crate::workspace_routes::list_workspaces,
        crate::workspace_routes::get_workspace,
        crate::workspace_routes::rename_workspace,
        crate::workspace_routes::list_members,
        crate::workspace_routes::change_member_role,
        crate::workspace_routes::remove_member,
        crate::workspace_routes::transfer_ownership,
        crate::workspace_routes::create_invitation,
        crate::workspace_routes::list_invitations,
        crate::workspace_routes::revoke_invitation,
        crate::workspace_routes::accept_invitation,
        crate::workspace_routes::delete_workspace,
        crate::workspace_routes::restore_workspace,
        crate::workspace_routes::list_trash,
        crate::workspace_routes::list_audit,
        crate::workspace_routes::set_account_suspension,
        crate::workspace_routes::list_global_audit,
        crate::workspace_routes::export_global_audit,
        crate::task_routes::list_projects,
        crate::task_routes::create_project,
        crate::task_routes::update_project,
        crate::task_routes::delete_project,
        crate::task_routes::restore_project,
        crate::task_routes::list_project_trash,
        crate::task_routes::list_statuses,
        crate::task_routes::create_status,
        crate::task_routes::update_status,
        crate::task_routes::delete_status,
        crate::task_routes::reorder_statuses,
        crate::task_routes::list_labels,
        crate::task_routes::create_label,
        crate::task_routes::update_label,
        crate::task_routes::delete_label,
        crate::task_routes::list_tasks,
        crate::task_routes::get_task,
        crate::task_routes::create_task,
        crate::task_routes::update_task,
        crate::task_routes::bulk_tasks,
        crate::task_routes::reorder_tasks,
        crate::task_routes::delete_task,
        crate::task_routes::restore_task,
        crate::task_routes::list_task_trash,
        crate::task_routes::list_comments,
        crate::task_routes::create_comment,
        crate::task_routes::update_comment,
        crate::task_routes::delete_comment,
        crate::attachment_routes::list_task_attachments,
        crate::attachment_routes::list_comment_attachments,
        crate::attachment_routes::upload_task_attachments,
        crate::attachment_routes::upload_comment_attachments,
        crate::attachment_routes::create_attachment_comment,
        crate::attachment_routes::delete_task_attachment,
        crate::attachment_routes::delete_comment_attachment,
        crate::attachment_routes::download_task_attachment,
        crate::attachment_routes::download_comment_attachment,
    )
)]
struct ApiDocument;

struct ProblemDetails;

impl Modify for ProblemDetails {
    fn modify(&self, openapi: &mut OpenApiDocument) {
        openapi
            .components
            .as_mut()
            .expect("OpenAPI components exist")
            .add_security_scheme(
                "cookieAuth",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("__Host-orbit_session"))),
            );

        for (route, path) in &mut openapi.paths.paths {
            for operation in [
                &mut path.get,
                &mut path.put,
                &mut path.post,
                &mut path.delete,
                &mut path.options,
                &mut path.head,
                &mut path.patch,
                &mut path.trace,
            ]
            .into_iter()
            .flatten()
            {
                let schema = problem_schema(route);
                let operation_id = operation.operation_id.as_deref().unwrap_or_default();
                operation.security = Some(if public_operation(operation_id) {
                    vec![SecurityRequirement::default()]
                } else if operation_id == "accept_invitation" {
                    vec![
                        SecurityRequirement::default(),
                        SecurityRequirement::new("cookieAuth", Vec::<String>::new()),
                    ]
                } else {
                    vec![SecurityRequirement::new("cookieAuth", Vec::<String>::new())]
                });
                operation.parameters.get_or_insert_default().push(
                    ParameterBuilder::new()
                        .name("X-Orbit-Contract")
                        .parameter_in(ParameterIn::Header)
                        .required(Required::False)
                        .description(Some(format!(
                            "Frontend contract identifier. Unsupported values return contract_mismatch; current value: {CONTRACT_ID}."
                        )))
                        .schema(Some(
                            ObjectBuilder::new().schema_type(Type::String),
                        ))
                        .build(),
                );

                for (status, description) in problem_responses(route, operation_id) {
                    operation
                        .responses
                        .responses
                        .insert(status.to_owned(), problem_response(description, schema));
                }
                operation.responses.responses.insert(
                    "default".to_owned(),
                    problem_response("internal_error or another documented stable code", schema),
                );
            }
        }
    }
}

fn problem_schema(route: &str) -> &'static str {
    if route.contains("/attachments") {
        "AttachmentProblem"
    } else if route.contains("/tasks") || route.contains("/projects") || route.contains("/labels") {
        "TaskProblem"
    } else if route.starts_with("/api/v1/auth") || route.starts_with("/api/v1/setup") {
        "AuthProblem"
    } else {
        "WorkspaceProblem"
    }
}

fn public_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "setup_status" | "setup_complete" | "login" | "recovery_request" | "recovery_complete"
    )
}

fn problem_responses(route: &str, operation_id: &str) -> Vec<(&'static str, &'static str)> {
    let bad_request = if operation_id == "recovery_complete" {
        "invalid_proxy_headers, invalid_request, or invalid_recovery_token"
    } else if route.contains("/attachments") {
        "invalid_proxy_headers, invalid_request, invalid_cursor, or invalid_multipart"
    } else if route.contains("/tasks") || route.contains("/projects") || route.contains("/labels") {
        "invalid_proxy_headers, invalid_request, or invalid_cursor"
    } else {
        "invalid_proxy_headers or invalid_request"
    };
    let conflict = if operation_id == "setup_complete" {
        "contract_mismatch or setup_unavailable"
    } else {
        "contract_mismatch"
    };
    let too_large = if route.contains("/attachments") {
        "request_too_large or upload_too_large"
    } else {
        "request_too_large"
    };
    let forbidden = if route.starts_with("/api/v1/workspaces") || route.starts_with("/api/v1/admin")
    {
        "origin_forbidden, workspace_action_forbidden, ownership_transfer_required, or installation_admin_required"
    } else {
        "origin_forbidden"
    };
    let mut responses = vec![
        ("400", bad_request),
        ("403", forbidden),
        ("409", conflict),
        ("413", too_large),
        ("500", "internal_error"),
    ];
    if !public_operation(operation_id) && operation_id != "accept_invitation" {
        responses.push(("401", "authentication_required"));
    }
    if route.contains("/attachments") {
        responses.extend([
            ("404", "attachment_not_found"),
            ("422", "validation_failed"),
        ]);
    } else if route.contains("/tasks") || route.contains("/projects") || route.contains("/labels") {
        responses.extend([
            ("404", "task_resource_not_found"),
            (
                "409",
                "contract_mismatch, task_conflict, conflict, or restore_conflict",
            ),
            ("422", "validation_failed"),
        ]);
    } else if route.starts_with("/api/v1/workspaces") || route.starts_with("/api/v1/admin") {
        responses.extend([
            (
                "404",
                "workspace_resource_not_found, invitation_not_found, or user_not_found",
            ),
            ("409", "contract_mismatch, workspace_conflict, or conflict"),
            (
                "422",
                "invalid_workspace_name, invalid_email, invalid_registration, or invalid_password",
            ),
        ]);
    }
    if operation_id == "accept_invitation" {
        responses.extend([
            ("401", "authentication_required"),
            ("429", "invitation_registration_throttled"),
        ]);
    }
    responses
}

fn problem_response(
    description: &str,
    schema: &str,
) -> utoipa::openapi::RefOr<utoipa::openapi::Response> {
    ResponseBuilder::new()
        .description(description)
        .content(
            "application/problem+json",
            Content::new(Some(Ref::from_schema_name(schema))),
        )
        .build()
        .into()
}

pub fn openapi_json() -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(&ApiDocument::openapi())?;
    json.push('\n');
    Ok(json)
}
