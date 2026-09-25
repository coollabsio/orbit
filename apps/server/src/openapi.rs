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
        crate::workspace_routes::list_api_tokens,
        crate::workspace_routes::create_api_token,
        crate::workspace_routes::revoke_api_token,
        crate::workspace_routes::set_account_suspension,
        crate::workspace_routes::list_global_audit,
        crate::workspace_routes::export_global_audit,
        crate::workspace_routes::create_backup,
        crate::integration_routes::create_discord_event,
        crate::integration_routes::github_webhook,
        crate::integration_routes::github_manifest_callback,
        crate::integration_routes::github_workspace_settings,
        crate::integration_routes::github_project_settings,
        crate::integration_routes::save_github_project_connection,
        crate::integration_routes::delete_github_project_connection,
        crate::integration_routes::start_github_manifest,
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
        crate::task_routes::list_task_relations,
        crate::task_routes::create_task_relation,
        crate::task_routes::delete_task_relation,
        crate::task_routes::list_github_links,
        crate::task_routes::list_task_activity,
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
        crate::page_routes::list_pages,
        crate::page_routes::create_page,
        crate::page_routes::get_page,
        crate::page_routes::update_page,
        crate::page_routes::move_page,
        crate::page_routes::delete_page,
        crate::page_routes::restore_page,
        crate::page_routes::list_page_trash,
        crate::page_routes::purge_page,
        crate::page_routes::empty_page_trash,
        crate::page_routes::duplicate_page,
        crate::page_routes::search_pages,
        crate::page_routes::list_page_favorites,
        crate::page_routes::add_page_favorite,
        crate::page_routes::remove_page_favorite,
        crate::page_routes::move_page_favorite,
        crate::page_routes::list_page_versions,
        crate::page_routes::get_page_version,
        crate::page_routes::restore_page_version,
        crate::page_file_routes::upload_page_file,
        crate::page_file_routes::download_page_file,
        crate::teamspace_routes::list_teamspaces,
        crate::teamspace_routes::create_teamspace,
        crate::teamspace_routes::update_teamspace,
        crate::teamspace_routes::delete_teamspace,
        crate::teamspace_routes::move_teamspace,
        crate::import_routes::create_notion_import,
        crate::import_routes::list_notion_imports,
        crate::import_routes::get_notion_import,
        crate::import_routes::start_notion_import,
        crate::import_routes::cancel_notion_import,
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
        openapi
            .components
            .as_mut()
            .expect("OpenAPI components exist")
            .add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(utoipa::openapi::security::Http::new(
                    utoipa::openapi::security::HttpAuthScheme::Bearer,
                )),
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
                operation.security = Some(if operation_id == "create_discord_event" {
                    vec![SecurityRequirement::new("bearerAuth", Vec::<String>::new())]
                } else if public_operation(&operation_id) {
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
    if route.contains("/integrations/discord") {
        "TaskProblem"
    } else if route.contains("/attachments") {
        "AttachmentProblem"
    } else if route.contains("/tasks")
        || route.contains("/projects")
        || route.contains("/labels")
        || route.contains("/pages")
        || route.contains("/teamspaces")
        || route.contains("/imports/")
    {
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
            | "github_webhook"
            | "github_manifest_callback"
    )
}

fn problem_responses(operation_id: &str) -> BTreeMap<&'static str, String> {
    let mut responses = BTreeMap::<&str, Vec<&str>>::new();
    add_code(&mut responses, "400", "invalid_proxy_headers");
    add_code(&mut responses, "409", "contract_mismatch");
    add_code(&mut responses, "413", "request_too_large");
    add_code(&mut responses, "500", "internal_error");
    if unsafe_operation(operation_id)
        && !matches!(operation_id, "create_discord_event" | "github_webhook")
    {
        add_code(&mut responses, "403", "origin_forbidden");
    }
    if !public_operation(operation_id) && operation_id != "create_discord_event" {
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
        "create_discord_event" => {
            add_code(&mut responses, "401", "api_token_required");
            add_code(&mut responses, "401", "invalid_api_token");
            add_code(&mut responses, "403", "api_token_scope_forbidden");
            add_code(&mut responses, "409", "integration_event_conflict");
            add_code(&mut responses, "422", "validation_failed");
            add_code(&mut responses, "422", "integration_project_unavailable");
        }
        "github_webhook" => {
            add_code(&mut responses, "401", "invalid_github_signature");
            add_code(&mut responses, "503", "app_key_missing");
            add_code(&mut responses, "503", "app_key_invalid");
            add_code(&mut responses, "409", "github_labels_ambiguous");
        }
        "github_workspace_settings" => {
            add_code(&mut responses, "404", "github_workspace_not_found");
        }
        "github_project_settings" => add_code(&mut responses, "404", "github_project_not_found"),
        "save_github_project_connection" | "delete_github_project_connection" => {
            add_code(&mut responses, "403", "github_manager_required");
            add_code(&mut responses, "404", "github_project_not_found");
            add_code(&mut responses, "422", "github_repository_not_installed");
            add_code(&mut responses, "422", "invalid_github_connection");
            add_code(&mut responses, "409", "github_label_conflict");
        }
        "start_github_manifest" => {
            add_code(&mut responses, "403", "github_manager_required");
            add_code(&mut responses, "404", "github_workspace_not_found");
            add_code(&mut responses, "409", "github_app_exists");
            add_code(&mut responses, "422", "github_https_required");
            add_code(&mut responses, "422", "invalid_github_organization");
            add_code(&mut responses, "503", "app_key_missing");
        }
        "github_manifest_callback" => {
            add_code(&mut responses, "400", "invalid_github_registration");
            add_code(&mut responses, "409", "github_app_exists");
            add_code(&mut responses, "502", "github_registration_failed");
            add_code(&mut responses, "503", "app_key_missing");
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
        "list_invitations" | "list_audit" | "list_api_tokens" | "create_api_token"
        | "revoke_api_token" => {
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
        "upload_page_file" => {
            add_code(&mut responses, "400", "invalid_multipart");
            add_code(&mut responses, "404", "page_file_not_found");
            add_code(&mut responses, "413", "upload_too_large");
            add_code(&mut responses, "422", "validation_failed");
        }
        "download_page_file" => add_code(&mut responses, "404", "page_file_not_found"),
        id if notion_import_operation(id) => notion_import_errors(id, &mut responses),
        id if task_operation(id) => task_errors(id, &mut responses),
        id if page_operation(id) => page_errors(id, &mut responses),
        id if teamspace_operation(id) => teamspace_errors(id, &mut responses),
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
        || matches!(
            operation_id,
            "me" | "setup_status" | "export_global_audit" | "search_pages"
        ))
}

fn invalid_request_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_discord_event"
            | "setup_complete"
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
            | "list_api_tokens"
            | "create_api_token"
            | "revoke_api_token"
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
            | "create_task_relation"
            | "bulk_tasks"
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
            | "create_page"
            | "update_page"
            | "move_page"
            | "delete_page"
            | "restore_page"
            | "purge_page"
            | "duplicate_page"
            | "search_pages"
            | "move_page_favorite"
            | "list_page_versions"
            | "restore_page_version"
            | "create_teamspace"
            | "update_teamspace"
            | "delete_teamspace"
            | "move_teamspace"
            | "create_notion_import"
            | "get_notion_import"
            | "start_notion_import"
    )
}

fn notion_import_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_notion_import"
            | "list_notion_imports"
            | "get_notion_import"
            | "start_notion_import"
            | "cancel_notion_import"
    )
}

fn notion_import_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "notion_import_not_found");
    match operation_id {
        "create_notion_import" => {
            add_code(responses, "409", "app_key_missing");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "422", "notion_token_invalid");
            add_code(responses, "502", "notion_unavailable");
        }
        "start_notion_import" => {
            add_code(responses, "404", "page_not_found");
            add_code(responses, "404", "teamspace_not_found");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "409", "notion_import_state_conflict");
            add_code(responses, "422", "validation_failed");
            add_code(responses, "422", "notion_import_too_large");
        }
        "cancel_notion_import" => add_code(responses, "409", "notion_import_state_conflict"),
        _ => {}
    }
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
            | "list_task_relations"
            | "create_task_relation"
            | "delete_task_relation"
            | "create_task"
            | "update_task"
            | "bulk_tasks"
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
            | "delete_status"
            | "reorder_statuses"
            | "create_label"
            | "update_label"
            | "create_task"
            | "update_task"
            | "bulk_tasks"
            | "reorder_tasks"
            | "create_comment"
            | "update_comment"
            | "create_task_relation"
            | "delete_task_relation"
    ) || operation_id == "list_tasks"
    {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "create_project"
            | "create_status"
            | "create_label"
            | "create_task"
            | "create_comment"
            | "create_task_relation"
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
    if matches!(operation_id, "update_task" | "bulk_tasks") {
        add_code(responses, "409", "github_content_read_only");
    }
}

fn page_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_pages"
            | "create_page"
            | "get_page"
            | "update_page"
            | "move_page"
            | "delete_page"
            | "restore_page"
            | "list_page_trash"
            | "purge_page"
            | "empty_page_trash"
            | "duplicate_page"
            | "search_pages"
            | "list_page_favorites"
            | "add_page_favorite"
            | "remove_page_favorite"
            | "move_page_favorite"
            | "list_page_versions"
            | "get_page_version"
            | "restore_page_version"
    )
}

fn page_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "page_not_found");
    if matches!(operation_id, "create_page" | "move_page") {
        add_code(responses, "404", "teamspace_not_found");
    }
    if matches!(
        operation_id,
        "create_page" | "update_page" | "move_page" | "search_pages" | "move_page_favorite"
    ) {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "update_page" | "move_page" | "delete_page" | "restore_page" | "purge_page"
    ) {
        add_code(responses, "409", "conflict");
    }
    if operation_id == "purge_page" {
        add_code(responses, "403", "workspace_action_forbidden");
        add_code(responses, "409", "page_not_trashed");
    }
    if operation_id == "list_page_versions" {
        add_code(responses, "400", "invalid_cursor");
        add_code(responses, "422", "validation_failed");
    }
    if matches!(operation_id, "get_page_version" | "restore_page_version") {
        add_code(responses, "404", "page_version_not_found");
    }
    if operation_id == "restore_page_version" {
        add_code(responses, "409", "conflict");
    }
}

fn teamspace_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_teamspaces"
            | "create_teamspace"
            | "update_teamspace"
            | "delete_teamspace"
            | "move_teamspace"
    )
}

fn teamspace_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "teamspace_not_found");
    if matches!(
        operation_id,
        "create_teamspace" | "update_teamspace" | "move_teamspace"
    ) {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "update_teamspace" | "delete_teamspace" | "move_teamspace"
    ) {
        add_code(responses, "409", "conflict");
    }
    if operation_id == "delete_teamspace" {
        add_code(responses, "403", "workspace_action_forbidden");
        add_code(responses, "409", "teamspace_not_empty");
        add_code(responses, "422", "last_teamspace");
    }
}

fn add_download_media_types(operation_id: &str, operation: &mut utoipa::openapi::path::Operation) {
    let schema = match operation_id {
        "download_task_attachment" | "download_comment_attachment" => "AttachmentDownload",
        "download_page_file" => "PageFileDownload",
        _ => return,
    };
    let Some(RefOr::T(response)) = operation.responses.responses.get_mut("200") else {
        return;
    };
    for media_type in [
        "application/pdf",
        "image/avif",
        "image/gif",
        "image/jpeg",
        "image/png",
        "image/svg+xml",
        "image/webp",
        "text/html",
    ] {
        response.content.insert(
            media_type.to_owned(),
            Content::new(Some(Ref::from_schema_name(schema))),
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
