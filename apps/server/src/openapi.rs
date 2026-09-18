use std::collections::BTreeMap;
use utoipa::openapi::path::{ParameterBuilder, ParameterIn};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityRequirement, SecurityScheme};
use utoipa::openapi::{Content, OpenApi as OpenApiDocument, Ref, RefOr, Required, ResponseBuilder};
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
        crate::auth_routes::update_me,
        crate::auth_routes::change_password,
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
        crate::workspace_routes::preview_invitation,
        crate::workspace_routes::delete_workspace,
        crate::workspace_routes::restore_workspace,
        crate::workspace_routes::list_trash,
        crate::workspace_routes::list_audit,
        crate::workspace_routes::set_account_suspension,
        crate::workspace_routes::list_global_audit,
        crate::workspace_routes::export_global_audit,
        crate::workspace_routes::create_backup,
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
        crate::task_routes::list_task_activity,
        crate::task_routes::create_task,
        crate::task_routes::update_task,
        crate::task_routes::bulk_tasks,
        crate::task_routes::resolve_tasks,
        crate::task_routes::reorder_tasks,
        crate::task_routes::delete_task,
        crate::task_routes::restore_task,
        crate::task_routes::list_task_trash,
        crate::task_routes::list_comments,
        crate::task_routes::create_comment,
        crate::task_routes::update_comment,
        crate::task_routes::delete_comment,
        crate::task_routes::list_notifications,
        crate::task_routes::read_notification,
        crate::task_routes::read_all_notifications,
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
                let operation_id = operation.operation_id.clone().unwrap_or_default();
                operation.security = Some(if public_operation(&operation_id) {
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

                for (status, description) in problem_responses(&operation_id) {
                    operation
                        .responses
                        .responses
                        .insert(status.to_owned(), problem_response(&description, schema));
                }
                add_download_media_types(&operation_id, operation);
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
        "setup_status"
            | "setup_complete"
            | "login"
            | "recovery_request"
            | "recovery_complete"
            | "preview_invitation"
    )
}

fn problem_responses(operation_id: &str) -> BTreeMap<&'static str, String> {
    let mut responses = BTreeMap::<&str, Vec<&str>>::new();
    add_code(&mut responses, "400", "invalid_proxy_headers");
    add_code(&mut responses, "409", "contract_mismatch");
    add_code(&mut responses, "413", "request_too_large");
    add_code(&mut responses, "500", "internal_error");
    if unsafe_operation(operation_id) {
        add_code(&mut responses, "403", "origin_forbidden");
    }
    if !public_operation(operation_id) {
        add_code(&mut responses, "401", "authentication_required");
    }
    if invalid_request_operation(operation_id) {
        add_code(&mut responses, "400", "invalid_request");
    }

    match operation_id {
        "setup_complete" => {
            add_code(&mut responses, "401", "invalid_setup_token");
            add_code(&mut responses, "409", "setup_unavailable");
            add_code(&mut responses, "422", "invalid_password");
        }
        "login" => {
            add_code(&mut responses, "401", "invalid_credentials");
            add_code(&mut responses, "429", "authentication_throttled");
        }
        "recovery_complete" => {
            add_code(&mut responses, "400", "invalid_recovery_token");
            add_code(&mut responses, "422", "invalid_password");
        }
        "update_me" => add_code(&mut responses, "422", "invalid_display_name"),
        "change_password" => {
            add_code(&mut responses, "401", "invalid_credentials");
            add_code(&mut responses, "422", "invalid_password");
        }
        "revoke_session" => add_code(&mut responses, "404", "session_not_found"),
        "preview_invitation" => add_code(&mut responses, "404", "invitation_not_found"),
        "accept_invitation" => {
            add_code(&mut responses, "403", "invitation_email_mismatch");
            add_code(&mut responses, "404", "invitation_not_found");
            add_code(&mut responses, "409", "workspace_conflict");
            add_code(&mut responses, "422", "invalid_registration");
            add_code(&mut responses, "422", "invalid_password");
            add_code(&mut responses, "429", "invitation_registration_throttled");
        }
        "create_workspace" => add_code(&mut responses, "422", "invalid_workspace_name"),
        "get_workspace" => add_code(&mut responses, "404", "workspace_resource_not_found"),
        "rename_workspace" => {
            workspace_resource_errors(&mut responses);
            add_code(&mut responses, "422", "invalid_workspace_name");
        }
        "list_members" => add_code(&mut responses, "404", "workspace_resource_not_found"),
        "change_member_role" | "remove_member" => {
            workspace_resource_errors(&mut responses);
            add_code(&mut responses, "409", "ownership_transfer_required");
        }
        "transfer_ownership" | "delete_workspace" | "restore_workspace" => {
            workspace_resource_errors(&mut responses);
        }
        "create_invitation" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "409", "workspace_conflict");
            add_code(&mut responses, "422", "invalid_email");
        }
        "list_invitations" | "list_audit" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
        }
        "revoke_invitation" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
        }
        "set_account_suspension" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "404", "user_not_found");
        }
        "list_global_audit" | "export_global_audit" => {
            add_code(&mut responses, "403", "installation_admin_required");
        }
        id if attachment_list_operation(id) => {
            add_code(&mut responses, "400", "invalid_cursor");
            add_code(&mut responses, "404", "attachment_not_found");
        }
        id if attachment_upload_operation(id) => {
            add_code(&mut responses, "400", "invalid_multipart");
            add_code(&mut responses, "404", "attachment_not_found");
            add_code(&mut responses, "413", "upload_too_large");
            add_code(&mut responses, "422", "validation_failed");
        }
        id if attachment_operation(id) => {
            add_code(&mut responses, "404", "attachment_not_found");
        }
        id if task_operation(id) => task_errors(id, &mut responses),
        _ => {}
    }

    responses
        .into_iter()
        .map(|(status, codes)| (status, codes.join(", ")))
        .collect()
}

fn add_code(
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
    status: &'static str,
    code: &'static str,
) {
    let codes = responses.entry(status).or_default();
    if !codes.contains(&code) {
        codes.push(code);
    }
}

fn unsafe_operation(operation_id: &str) -> bool {
    !(operation_id.starts_with("list_")
        || operation_id.starts_with("get_")
        || operation_id.starts_with("download_")
        || matches!(operation_id, "me" | "setup_status" | "export_global_audit"))
}

fn invalid_request_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "setup_complete"
            | "login"
            | "update_me"
            | "change_password"
            | "recovery_request"
            | "recovery_complete"
            | "create_workspace"
            | "rename_workspace"
            | "list_members"
            | "change_member_role"
            | "remove_member"
            | "transfer_ownership"
            | "create_invitation"
            | "list_invitations"
            | "accept_invitation"
            | "preview_invitation"
            | "delete_workspace"
            | "restore_workspace"
            | "list_audit"
            | "set_account_suspension"
            | "list_global_audit"
            | "export_global_audit"
            | "list_projects"
            | "create_project"
            | "update_project"
            | "delete_project"
            | "restore_project"
            | "list_project_trash"
            | "list_statuses"
            | "create_status"
            | "update_status"
            | "delete_status"
            | "reorder_statuses"
            | "list_labels"
            | "create_label"
            | "update_label"
            | "delete_label"
            | "list_tasks"
            | "create_task"
            | "update_task"
            | "bulk_tasks"
            | "resolve_tasks"
            | "reorder_tasks"
            | "delete_task"
            | "restore_task"
            | "list_task_trash"
            | "list_comments"
            | "create_comment"
            | "update_comment"
            | "delete_comment"
            | "list_task_attachments"
            | "list_comment_attachments"
    )
}

fn workspace_resource_errors(responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "403", "workspace_action_forbidden");
    add_code(responses, "404", "workspace_resource_not_found");
    add_code(responses, "409", "workspace_conflict");
    add_code(responses, "409", "conflict");
}

fn attachment_list_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_task_attachments" | "list_comment_attachments"
    )
}

fn attachment_upload_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "upload_task_attachments" | "upload_comment_attachments" | "create_attachment_comment"
    )
}

fn attachment_operation(operation_id: &str) -> bool {
    operation_id.contains("attachment")
}

fn task_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_projects"
            | "create_project"
            | "update_project"
            | "delete_project"
            | "restore_project"
            | "list_project_trash"
            | "list_statuses"
            | "create_status"
            | "update_status"
            | "delete_status"
            | "reorder_statuses"
            | "list_labels"
            | "create_label"
            | "update_label"
            | "delete_label"
            | "list_tasks"
            | "get_task"
            | "create_task"
            | "update_task"
            | "bulk_tasks"
            | "resolve_tasks"
            | "reorder_tasks"
            | "delete_task"
            | "restore_task"
            | "list_task_trash"
            | "list_comments"
            | "create_comment"
            | "update_comment"
            | "delete_comment"
    )
}

fn task_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "task_resource_not_found");
    if matches!(
        operation_id,
        "list_projects"
            | "list_project_trash"
            | "list_statuses"
            | "list_labels"
            | "list_tasks"
            | "list_task_trash"
            | "list_comments"
    ) {
        add_code(responses, "400", "invalid_cursor");
    }
    if matches!(
        operation_id,
        "create_project"
            | "update_project"
            | "create_status"
            | "update_status"
            | "reorder_statuses"
            | "create_label"
            | "update_label"
            | "create_task"
            | "update_task"
            | "bulk_tasks"
            | "resolve_tasks"
            | "reorder_tasks"
            | "create_comment"
            | "update_comment"
    ) || operation_id == "list_tasks"
    {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "create_project" | "create_status" | "create_label" | "create_task" | "create_comment"
    ) {
        add_code(responses, "409", "task_conflict");
    }
    if matches!(
        operation_id,
        "update_project"
            | "delete_project"
            | "update_status"
            | "delete_status"
            | "reorder_statuses"
            | "update_label"
            | "delete_label"
            | "update_task"
            | "bulk_tasks"
            | "reorder_tasks"
            | "delete_task"
            | "update_comment"
            | "delete_comment"
    ) {
        add_code(responses, "409", "task_conflict");
        add_code(responses, "409", "conflict");
    }
    if matches!(operation_id, "restore_project" | "restore_task") {
        add_code(responses, "409", "task_conflict");
        add_code(responses, "409", "conflict");
        add_code(responses, "409", "restore_conflict");
    }
}

fn add_download_media_types(operation_id: &str, operation: &mut utoipa::openapi::path::Operation) {
    if !matches!(
        operation_id,
        "download_task_attachment" | "download_comment_attachment"
    ) {
        return;
    }
    let Some(RefOr::T(response)) = operation.responses.responses.get_mut("200") else {
        return;
    };
    for media_type in [
        "application/pdf",
        "image/gif",
        "image/jpeg",
        "image/png",
        "image/svg+xml",
        "image/webp",
        "text/html",
    ] {
        response.content.insert(
            media_type.to_owned(),
            Content::new(Some(Ref::from_schema_name("AttachmentDownload"))),
        );
    }
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
